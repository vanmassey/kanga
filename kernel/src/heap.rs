//! Frame-backed kernel heap using Limine's higher-half direct map.
//!
//! This explicit allocator is not yet installed as the process-wide
//! `GlobalAlloc`.

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::mem::{align_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::memory::{self, PAGE_SIZE};

const HEAP_PAGES: usize = 16;
const HEAP_BYTES: usize = HEAP_PAGES * PAGE_SIZE as usize;

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

#[repr(C)]
struct AllocationHeader {
    size: usize,
    requested_size: usize,
}

struct HeapState {
    start: usize,
    end: usize,
    free: *mut FreeBlock,
}

// SAFETY: HeapState is only accessed while HEAP's lock is held.
unsafe impl Send for HeapState {}

struct HeapLock {
    locked: AtomicBool,
    state: UnsafeCell<Option<HeapState>>,
}

// SAFETY: The lock serialises all access to the UnsafeCell.
unsafe impl Sync for HeapLock {}

impl HeapLock {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            state: UnsafeCell::new(None),
        }
    }

    fn lock(&self) -> HeapGuard<'_> {
        let flags: u64;
        // SAFETY: Called only in Kanga's privileged x86-64 kernel.
        unsafe {
            core::arch::asm!("pushfq", "pop {}", "cli", out(reg) flags);
        }

        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }

        HeapGuard {
            lock: self,
            restore_interrupts: flags & (1 << 9) != 0,
        }
    }
}

struct HeapGuard<'a> {
    lock: &'a HeapLock,
    restore_interrupts: bool,
}

impl core::ops::Deref for HeapGuard<'_> {
    type Target = Option<HeapState>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: This guard owns the heap lock.
        unsafe { &*self.lock.state.get() }
    }
}

impl core::ops::DerefMut for HeapGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: This guard owns the heap lock exclusively.
        unsafe { &mut *self.lock.state.get() }
    }
}

impl Drop for HeapGuard<'_> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        if self.restore_interrupts {
            // SAFETY: Restore IF only when it was set before locking.
            unsafe {
                core::arch::asm!("sti", options(nomem, nostack));
            }
        }
    }
}

static HEAP: HeapLock = HeapLock::new();

/// Initialise a 64 KiB heap from sixteen contiguous physical frames.
pub fn initialise(hhdm_offset: u64) -> bool {
    let mut state = HEAP.lock();
    if state.is_some() {
        return false;
    }

    let Some(physical) =
        memory::with_global_allocator(|allocator| allocator.allocate_contiguous(HEAP_PAGES))
            .flatten()
    else {
        return false;
    };

    let Some(virtual_address) = memory::physical_to_virtual(physical, hhdm_offset) else {
        release_frames(physical);
        return false;
    };
    let Ok(start) = usize::try_from(virtual_address) else {
        release_frames(physical);
        return false;
    };
    if start % align_of::<FreeBlock>() != 0 {
        release_frames(physical);
        return false;
    }
    let Some(end) = start.checked_add(HEAP_BYTES) else {
        release_frames(physical);
        return false;
    };

    // SAFETY: These newly allocated contiguous frames are writable via HHDM.
    unsafe {
        ptr::write_bytes(start as *mut u8, 0, HEAP_BYTES);
    }
    let first = start as *mut FreeBlock;
    // SAFETY: The heap region is aligned and large enough for a free-list node.
    unsafe {
        first.write(FreeBlock {
            size: HEAP_BYTES,
            next: ptr::null_mut(),
        });
    }

    *state = Some(HeapState {
        start,
        end,
        free: first,
    });
    true
}

/// Return all frames in a contiguous heap allocation to the physical allocator.
fn release_frames(first_frame: u64) {
    let _ = memory::with_global_allocator(|allocator| {
        for index in 0..HEAP_PAGES {
            let Some(offset) = (index as u64).checked_mul(PAGE_SIZE) else {
                break;
            };
            let Some(frame) = first_frame.checked_add(offset) else {
                break;
            };
            // SAFETY: Each frame belongs to the contiguous run allocated above.
            let _ = unsafe { allocator.deallocate_frame(frame) };
        }
    });
}

/// Allocate zeroed memory satisfying the requested layout; null means exhaustion.
pub fn allocate(layout: Layout) -> *mut u8 {
    if layout.size() == 0 {
        return core::ptr::NonNull::<u8>::dangling().as_ptr();
    }

    let mut state = HEAP.lock();
    let Some(heap) = state.as_mut() else {
        return ptr::null_mut();
    };

    let alignment = layout.align().max(align_of::<FreeBlock>());
    let header_size = size_of::<AllocationHeader>();
    let mut previous: *mut FreeBlock = ptr::null_mut();
    let mut current = heap.free;

    while !current.is_null() {
        // SAFETY: Every free-list node lies within the owned heap region.
        let block = unsafe { &mut *current };
        let block_start = current as usize;
        let Some(block_end) = block_start.checked_add(block.size) else {
            return ptr::null_mut();
        };
        let Some(mut payload) = align_up(block_start.saturating_add(header_size), alignment) else {
            return ptr::null_mut();
        };
        let mut allocation_start = payload - header_size;

        while allocation_start > block_start
            && allocation_start - block_start < size_of::<FreeBlock>()
        {
            let Some(next_payload) = payload.checked_add(alignment) else {
                return ptr::null_mut();
            };
            payload = next_payload;
            allocation_start = payload - header_size;
        }

        let Some(allocation_end) = payload.checked_add(layout.size()) else {
            return ptr::null_mut();
        };
        if allocation_end > block_end {
            previous = current;
            current = block.next;
            continue;
        }

        let suffix_start = align_up(allocation_end, align_of::<FreeBlock>()).unwrap_or(block_end);
        let has_suffix = block_end.saturating_sub(suffix_start) >= size_of::<FreeBlock>();
        let suffix = if has_suffix {
            suffix_start as *mut FreeBlock
        } else {
            ptr::null_mut()
        };
        let next = block.next;

        if allocation_start == block_start {
            if suffix.is_null() {
                if previous.is_null() {
                    heap.free = next;
                } else {
                    // SAFETY: previous is a valid free-list node.
                    unsafe {
                        (*previous).next = next;
                    }
                }
            } else {
                // SAFETY: The suffix remains within the original free block.
                unsafe {
                    suffix.write(FreeBlock {
                        size: block_end - suffix_start,
                        next,
                    });
                }
                if previous.is_null() {
                    heap.free = suffix;
                } else {
                    // SAFETY: previous is a valid free-list node.
                    unsafe {
                        (*previous).next = suffix;
                    }
                }
            }
        } else {
            block.size = allocation_start - block_start;
            if !suffix.is_null() {
                // SAFETY: The suffix remains within the original free block.
                unsafe {
                    suffix.write(FreeBlock {
                        size: block_end - suffix_start,
                        next,
                    });
                    block.next = suffix;
                }
            }
        }

        let allocated_size = if has_suffix {
            suffix_start - allocation_start
        } else {
            block_end - allocation_start
        };
        // SAFETY: The header is aligned and immediately precedes the payload.
        unsafe {
            (allocation_start as *mut AllocationHeader).write(AllocationHeader {
                size: allocated_size,
                requested_size: layout.size(),
            });
        }
        // SAFETY: The returned allocation is exclusively owned and in bounds.
        unsafe {
            ptr::write_bytes(payload as *mut u8, 0, layout.size());
        }
        return payload as *mut u8;
    }

    ptr::null_mut()
}

/// Return a live allocation to the heap.
///
/// # Safety
/// Pointer must come from allocate, be live, and be returned exactly once.
pub unsafe fn deallocate(pointer: *mut u8, layout: Layout) -> bool {
    if pointer.is_null() || layout.size() == 0 {
        return false;
    }

    let mut state = HEAP.lock();
    let Some(heap) = state.as_mut() else {
        return false;
    };
    let header_address = (pointer as usize).saturating_sub(size_of::<AllocationHeader>());
    if header_address < heap.start || header_address >= heap.end {
        return false;
    }

    // SAFETY: Caller guarantees pointer is a live allocation returned by allocate.
    let header = unsafe { &*(header_address as *const AllocationHeader) };
    let size = header.size;
    if header.requested_size != layout.size() {
        return false;
    }
    let Some(allocation_end) = header_address.checked_add(size) else {
        return false;
    };
    if size < size_of::<AllocationHeader>() || allocation_end > heap.end {
        return false;
    }

    let node = header_address as *mut FreeBlock;
    let mut previous: *mut FreeBlock = ptr::null_mut();
    let mut current = heap.free;
    while !current.is_null() && (current as usize) < header_address {
        previous = current;
        // SAFETY: current is a valid free-list node.
        current = unsafe { (*current).next };
    }

    // Reject overlap, including a duplicate free.
    if (!previous.is_null() && (previous as usize) + unsafe { (*previous).size } > header_address)
        || (!current.is_null() && allocation_end > current as usize)
    {
        return false;
    }

    // SAFETY: The live allocation is now being converted to a free-list node.
    unsafe {
        node.write(FreeBlock {
            size,
            next: current,
        });
    }
    if previous.is_null() {
        heap.free = node;
    } else {
        // SAFETY: previous is a valid free-list node.
        unsafe {
            (*previous).next = node;
        }
    }

    if !current.is_null() && allocation_end == current as usize {
        // SAFETY: The two free blocks are adjacent.
        unsafe {
            (*node).size += (*current).size;
            (*node).next = (*current).next;
        }
    }
    if !previous.is_null() && (previous as usize) + unsafe { (*previous).size } == header_address {
        // SAFETY: The two free blocks are adjacent.
        unsafe {
            (*previous).size += (*node).size;
            (*previous).next = (*node).next;
        }
    }

    true
}

/// Exercise aligned allocation, zeroing, reuse and reclamation during boot.
pub fn self_test() -> bool {
    let Ok(first_layout) = Layout::from_size_align(127, 64) else {
        return false;
    };
    let Ok(second_layout) = Layout::from_size_align(513, 256) else {
        return false;
    };

    let first = allocate(first_layout);
    let second = allocate(second_layout);
    if first.is_null() || second.is_null() || first as usize % 64 != 0 || second as usize % 256 != 0
    {
        if !first.is_null() {
            // SAFETY: first is a live allocation created above.
            let _ = unsafe { deallocate(first, first_layout) };
        }
        if !second.is_null() {
            // SAFETY: second is a live allocation created above.
            let _ = unsafe { deallocate(second, second_layout) };
        }
        return false;
    }

    // SAFETY: Both pointers refer to live allocations of the specified sizes.
    let zeroed = unsafe {
        (0..127).all(|index| *first.add(index) == 0)
            && (0..513).all(|index| *second.add(index) == 0)
    };
    // SAFETY: Both pointers are live and are returned exactly once.
    let freed_second = unsafe { deallocate(second, second_layout) };
    // SAFETY: first is still a live allocation.
    let freed_first = unsafe { deallocate(first, first_layout) };

    let reused = allocate(second_layout);
    let freed_reused = if reused.is_null() {
        false
    } else {
        // SAFETY: reused is a live allocation created above.
        unsafe { deallocate(reused, second_layout) }
    };

    zeroed && freed_second && freed_first && !reused.is_null() && freed_reused
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
}
