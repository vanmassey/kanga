//! Virtually contiguous kernel heap backed by independently allocated frames.
//!
//! This explicit allocator is not yet installed as the process-wide
//! `GlobalAlloc`.

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::mem::{align_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::memory::{self, PAGE_SIZE};
use crate::paging::{self, MappingError, PageFlags};

const HEAP_BASE: u64 = 0xffff_9000_0000_0000;
const INITIAL_HEAP_PAGES: usize = 16;
const GROWTH_HEAP_PAGES: usize = 64;
const MAX_HEAP_PAGES: usize = 4096;
const INITIAL_HEAP_BYTES: usize = INITIAL_HEAP_PAGES * PAGE_SIZE as usize;
const MAX_HEAP_BYTES: usize = MAX_HEAP_PAGES * PAGE_SIZE as usize;

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
    limit: usize,
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

/// Initialise a virtual-page-backed heap with a 64 KiB initial capacity.
///
/// The heap occupies a dedicated kernel virtual range. Physical frames are
/// allocated independently and mapped on demand, so growth does not require
/// physically contiguous memory.
pub fn initialise() -> bool {
    let mut state = HEAP.lock();
    if state.is_some() {
        return false;
    }

    let Ok(start) = usize::try_from(HEAP_BASE) else {
        return false;
    };
    let Some(end) = start.checked_add(INITIAL_HEAP_BYTES) else {
        return false;
    };
    let Some(limit) = start.checked_add(MAX_HEAP_BYTES) else {
        return false;
    };
    if !map_heap_pages(start, INITIAL_HEAP_PAGES) {
        return false;
    }

    let first = start as *mut FreeBlock;
    // SAFETY: The mapped heap range is aligned, writable and large enough.
    unsafe {
        first.write(FreeBlock {
            size: INITIAL_HEAP_BYTES,
            next: ptr::null_mut(),
        });
    }

    *state = Some(HeapState {
        start,
        end,
        limit,
        free: first,
    });
    true
}

/// Map newly allocated physical frames into the heap's virtual address range.
///
/// On failure, every leaf mapping created by this call is removed and its data
/// frame is returned. Empty intermediate page tables are retained by paging.
fn map_heap_pages(start: usize, pages: usize) -> bool {
    let mut mapped = 0usize;
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;

    while mapped < pages {
        let Some(offset) = mapped.checked_mul(PAGE_SIZE as usize) else {
            break;
        };
        let Some(virtual_address) = start.checked_add(offset) else {
            break;
        };
        if !matches!(
            paging::translate(virtual_address as u64),
            Err(MappingError::NotMapped)
        ) {
            break;
        }

        let frame = memory::with_global_allocator(|allocator| allocator.allocate_frame()).flatten();
        let Some(frame) = frame else {
            break;
        };
        if paging::map_page(virtual_address as u64, frame, flags).is_err() {
            let _ = memory::with_global_allocator(|allocator| {
                // SAFETY: The frame was allocated here and mapping did not succeed.
                unsafe { allocator.deallocate_frame(frame) }
            });
            break;
        }

        // SAFETY: The page has just been mapped writable and is exclusively owned.
        unsafe {
            ptr::write_bytes(virtual_address as *mut u8, 0, PAGE_SIZE as usize);
        }
        mapped += 1;
    }

    if mapped == pages {
        return true;
    }

    while mapped > 0 {
        mapped -= 1;
        let Some(offset) = mapped.checked_mul(PAGE_SIZE as usize) else {
            return false;
        };
        let Some(virtual_address) = start.checked_add(offset) else {
            return false;
        };
        if let Ok(frame) = paging::unmap_page(virtual_address as u64) {
            let _ = memory::with_global_allocator(|allocator| {
                // SAFETY: The mapping was removed and the frame is owned by this rollback.
                unsafe { allocator.deallocate_frame(frame) }
            });
        }
    }
    false
}

/// Add another run of virtual pages to the heap, coalescing with its last
/// free block when the two regions are adjacent.
fn grow_heap() -> bool {
    let mut state = HEAP.lock();
    let Some(heap) = state.as_mut() else {
        return false;
    };
    if heap.end >= heap.limit {
        return false;
    }

    let remaining_pages = (heap.limit - heap.end) / PAGE_SIZE as usize;
    let pages = remaining_pages.min(GROWTH_HEAP_PAGES);
    let growth_start = heap.end;
    let Some(growth_bytes) = pages.checked_mul(PAGE_SIZE as usize) else {
        return false;
    };
    let Some(new_end) = growth_start.checked_add(growth_bytes) else {
        return false;
    };
    if !map_heap_pages(growth_start, pages) {
        return false;
    }

    let mut last: *mut FreeBlock = ptr::null_mut();
    let mut current = heap.free;
    while !current.is_null() {
        last = current;
        // SAFETY: Every free-list node is within the mapped heap range.
        current = unsafe { (*current).next };
    }

    if !last.is_null() && (last as usize).checked_add(unsafe { (*last).size }) == Some(growth_start)
    {
        // SAFETY: The last free block and new pages are adjacent and owned.
        unsafe {
            (*last).size += growth_bytes;
        }
    } else {
        let new_block = growth_start as *mut FreeBlock;
        // SAFETY: The newly mapped range is aligned and large enough for a node.
        unsafe {
            new_block.write(FreeBlock {
                size: growth_bytes,
                next: ptr::null_mut(),
            });
        }
        if last.is_null() {
            heap.free = new_block;
        } else {
            // SAFETY: last is the final valid node in the free list.
            unsafe {
                (*last).next = new_block;
            }
        }
    }

    heap.end = new_end;
    true
}

/// Allocate zeroed memory satisfying the requested layout; null means exhaustion.
pub fn allocate(layout: Layout) -> *mut u8 {
    if layout.size() == 0 {
        return core::ptr::NonNull::<u8>::dangling().as_ptr();
    }

    let alignment = layout.align().max(align_of::<FreeBlock>());
    let header_size = size_of::<AllocationHeader>();
    let Some(minimum_size) = layout
        .size()
        .checked_add(header_size)
        .and_then(|size| size.checked_add(alignment - 1))
    else {
        return ptr::null_mut();
    };
    if minimum_size > MAX_HEAP_BYTES {
        return ptr::null_mut();
    }

    loop {
        let mut state = HEAP.lock();
        let Some(heap) = state.as_mut() else {
            return ptr::null_mut();
        };

        let mut previous: *mut FreeBlock = ptr::null_mut();
        let mut current = heap.free;

        while !current.is_null() {
            // SAFETY: Every free-list node lies within the owned heap region.
            let block = unsafe { &mut *current };
            let block_start = current as usize;
            let Some(block_end) = block_start.checked_add(block.size) else {
                return ptr::null_mut();
            };
            let Some(mut payload) =
                align_up(block_start.saturating_add(header_size), alignment)
            else {
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

            let suffix_start =
                align_up(allocation_end, align_of::<FreeBlock>()).unwrap_or(block_end);
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

        drop(state);
        if !grow_heap() {
            return ptr::null_mut();
        }
    }
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
    let Ok(growth_layout) = Layout::from_size_align(96 * 1024, 4096) else {
        return false;
    };

    let first = allocate(first_layout);
    let second = allocate(second_layout);
    let growth = allocate(growth_layout);
    if first.is_null()
        || second.is_null()
        || growth.is_null()
        || first as usize % 64 != 0
        || second as usize % 256 != 0
        || growth as usize % 4096 != 0
    {
        if !first.is_null() {
            // SAFETY: first is a live allocation created above.
            let _ = unsafe { deallocate(first, first_layout) };
        }
        if !second.is_null() {
            // SAFETY: second is a live allocation created above.
            let _ = unsafe { deallocate(second, second_layout) };
        }
        if !growth.is_null() {
            // SAFETY: growth is a live allocation created above.
            let _ = unsafe { deallocate(growth, growth_layout) };
        }
        return false;
    }

    // SAFETY: All pointers refer to live allocations of the specified sizes.
    let zeroed = unsafe {
        (0..127).all(|index| *first.add(index) == 0)
            && (0..513).all(|index| *second.add(index) == 0)
            && (0..96 * 1024).all(|index| *growth.add(index) == 0)
    };
    // SAFETY: All three allocations are live and are returned exactly once.
    let freed_growth = unsafe { deallocate(growth, growth_layout) };
    let freed_second = unsafe { deallocate(second, second_layout) };
    let freed_first = unsafe { deallocate(first, first_layout) };

    let reused = allocate(second_layout);
    let freed_reused = if reused.is_null() {
        false
    } else {
        // SAFETY: reused is a live allocation created above.
        unsafe { deallocate(reused, second_layout) }
    };

    zeroed && freed_growth && freed_second && freed_first && !reused.is_null() && freed_reused
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
}
