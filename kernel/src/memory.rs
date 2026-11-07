use limine::memory_map::{Entry, EntryType};

pub const PAGE_SIZE: u64 = 4096;
pub const BITMAP_WORDS: usize = 16_384;
const LOW_MEMORY_RESERVED_END: u64 = 0x10_0000;
const FRAME_TEST_PATTERN: u64 = 0x4B41_4E47_4146_524D;

/// Bitmap-backed physical frame allocator.
///
/// One bit represents one 4 KiB physical frame. A set bit means unavailable;
/// a clear bit means free. The fixed bitmap supplied by the caller determines
/// the maximum physical address range that can be managed.
pub struct BitmapFrameAllocator<'a> {
    entries: &'a [&'a Entry],
    bitmap: &'a mut [u64],
    next_hint: usize,
    free_frames: u64,
    managed_frames: u64,
    ignored_usable_frames: u64,
}

impl<'a> BitmapFrameAllocator<'a> {
    /// Initialise the bitmap from Limine's usable memory map.
    ///
    /// The first MiB is reserved. Region boundaries are rounded inward to
    /// complete pages. Memory outside the supplied bitmap is left unmanaged.
    pub fn new(entries: &'a [&'a Entry], bitmap: &'a mut [u64]) -> Self {
        bitmap.fill(u64::MAX);
        let mut allocator = Self {
            entries,
            bitmap,
            next_hint: 0,
            free_frames: 0,
            managed_frames: 0,
            ignored_usable_frames: 0,
        };

        for entry in entries {
            if entry.entry_type != EntryType::USABLE {
                continue;
            }
            let Some(region_end) = entry.base.checked_add(entry.length) else {
                continue;
            };
            let region_start = entry.base.max(LOW_MEMORY_RESERVED_END);
            let Some(rounded_start) = region_start.checked_add(PAGE_SIZE - 1) else {
                continue;
            };
            let mut frame = rounded_start & !(PAGE_SIZE - 1);
            let end = region_end & !(PAGE_SIZE - 1);

            while frame < end {
                let index = (frame / PAGE_SIZE) as u128;
                if index < (allocator.bitmap.len() as u128) * 64 {
                    let index = index as usize;
                    let mask = 1_u64 << (index % 64);
                    let word = &mut allocator.bitmap[index / 64];
                    if *word & mask != 0 {
                        *word &= !mask;
                        allocator.free_frames += 1;
                        allocator.managed_frames += 1;
                    }
                } else {
                    allocator.ignored_usable_frames =
                        allocator.ignored_usable_frames.saturating_add(1);
                }
                let Some(next) = frame.checked_add(PAGE_SIZE) else {
                    break;
                };
                frame = next;
            }
        }
        allocator
    }

    /// Allocate one free frame, returning its physical address.
    pub fn allocate_frame(&mut self) -> Option<u64> {
        let capacity = self.bitmap.len().checked_mul(64)?;
        if self.free_frames == 0 || capacity == 0 {
            return None;
        }

        for offset in 0..capacity {
            let index = (self.next_hint + offset) % capacity;
            let mask = 1_u64 << (index % 64);
            let word = &mut self.bitmap[index / 64];
            if *word & mask == 0 {
                *word |= mask;
                self.free_frames -= 1;
                self.next_hint = (index + 1) % capacity;
                return (index as u64).checked_mul(PAGE_SIZE);
            }
        }
        None
    }

    /// Allocate a physically contiguous run of frames.
    ///
    /// Returns the physical address of the first frame. The caller must
    /// deallocate every frame in the run individually when finished.
    pub fn allocate_contiguous(&mut self, count: usize) -> Option<u64> {
        let capacity = self.bitmap.len().checked_mul(64)?;
        if count == 0 || count > capacity || self.free_frames < count as u64 {
            return None;
        }

        for offset in 0..capacity {
            let start = (self.next_hint + offset) % capacity;
            if start + count > capacity {
                continue;
            }
            let available = (start..start + count)
                .all(|index| self.bitmap[index / 64] & (1_u64 << (index % 64)) == 0);
            if !available {
                continue;
            }

            for index in start..start + count {
                self.bitmap[index / 64] |= 1_u64 << (index % 64);
            }
            self.free_frames -= count as u64;
            self.next_hint = (start + count) % capacity;
            return (start as u64).checked_mul(PAGE_SIZE);
        }
        None
    }

    /// Return one frame to the allocator.
    ///
    /// # Safety
    /// The frame must be page-aligned, managed by this allocator, currently
    /// allocated, and no longer in use. It must be returned exactly once.
    pub unsafe fn deallocate_frame(&mut self, frame: u64) -> bool {
        if !self.is_managed_frame(frame) || self.free_frames == u64::MAX {
            return false;
        }
        let index = (frame / PAGE_SIZE) as usize;
        let mask = 1_u64 << (index % 64);
        let word = &mut self.bitmap[index / 64];
        if *word & mask == 0 {
            return false;
        }
        *word &= !mask;
        self.free_frames += 1;
        if index < self.next_hint {
            self.next_hint = index;
        }
        true
    }

    pub const fn free_frame_count(&self) -> u64 {
        self.free_frames
    }

    pub const fn managed_frame_count(&self) -> u64 {
        self.managed_frames
    }

    pub const fn ignored_usable_frame_count(&self) -> u64 {
        self.ignored_usable_frames
    }

    pub fn capacity_frames(&self) -> usize {
        self.bitmap.len().saturating_mul(64)
    }

    fn is_managed_frame(&self, frame: u64) -> bool {
        if frame < LOW_MEMORY_RESERVED_END || frame % PAGE_SIZE != 0 {
            return false;
        }
        let index = (frame / PAGE_SIZE) as u128;
        if index >= (self.bitmap.len() as u128) * 64 {
            return false;
        }
        self.entries.iter().any(|entry| {
            if entry.entry_type != EntryType::USABLE {
                return false;
            }
            let Some(end) = entry.base.checked_add(entry.length) else {
                return false;
            };
            frame >= entry.base.max(LOW_MEMORY_RESERVED_END)
                && frame
                    .checked_add(PAGE_SIZE)
                    .is_some_and(|frame_end| frame_end <= end)
        })
    }
}

pub fn physical_to_virtual(physical_address: u64, hhdm_offset: u64) -> Option<u64> {
    physical_address.checked_add(hhdm_offset)
}

/// # Safety
/// virtual_address must be an exclusively owned, writable 4 KiB HHDM frame.
pub unsafe fn zero_and_verify_frame(virtual_address: u64) -> bool {
    let Ok(address) = usize::try_from(virtual_address) else {
        return false;
    };
    if address % PAGE_SIZE as usize != 0 {
        return false;
    }
    let frame_ptr = address as *mut u8;
    // SAFETY: The caller guarantees exclusive ownership of this mapped frame.
    unsafe { core::ptr::write_bytes(frame_ptr, 0, PAGE_SIZE as usize) };
    let test_ptr = frame_ptr.cast::<u64>();
    // SAFETY: The frame is aligned, mapped and exclusively owned by the caller.
    unsafe { test_ptr.write_volatile(FRAME_TEST_PATTERN) };
    // SAFETY: The preceding write targets the same valid frame.
    let observed = unsafe { test_ptr.read_volatile() };
    // SAFETY: Restore the tested word to zero before handing the frame onward.
    unsafe { test_ptr.write_volatile(0) };
    observed == FRAME_TEST_PATTERN
}

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

/// Serialises access to the shared frame allocator across CPUs.
struct SpinMutex<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: The lock grants exclusive access to the protected value.
unsafe impl<T: Send> Sync for SpinMutex<T> {}

impl<T> SpinMutex<T> {
    const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    fn lock(&self) -> SpinMutexGuard<'_, T> {
        // Disable local maskable interrupts before taking the lock. Otherwise
        // an IRQ handler could re-enter the allocator and spin forever on the
        // lock held by the interrupted code.
        let interrupt_flags: u64;
        unsafe {
            core::arch::asm!("pushfq", "pop {}", "cli", out(reg) interrupt_flags);
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
        SpinMutexGuard {
            mutex: self,
            restore_interrupts: interrupt_flags & (1 << 9) != 0,
        }
    }
}

struct SpinMutexGuard<'a, T> {
    mutex: &'a SpinMutex<T>,
    restore_interrupts: bool,
}

impl<T> core::ops::Deref for SpinMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // SAFETY: This guard owns the lock.
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> core::ops::DerefMut for SpinMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: This guard owns the lock exclusively.
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Drop for SpinMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.store(false, Ordering::Release);
        if self.restore_interrupts {
            // SAFETY: Restore IF only if it was set when this guard was created.
            unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
        }
    }
}

static GLOBAL_FRAME_ALLOCATOR: SpinMutex<Option<BitmapFrameAllocator<'static>>> =
    SpinMutex::new(None);

/// Initialise the shared allocator once. Limine's memory-map response and the
/// static bitmap both remain valid for the lifetime of the kernel.
pub fn initialise_global(entries: &'static [&'static Entry], bitmap: &'static mut [u64]) -> bool {
    let mut allocator = GLOBAL_FRAME_ALLOCATOR.lock();
    if allocator.is_some() {
        return false;
    }
    *allocator = Some(BitmapFrameAllocator::new(entries, bitmap));
    true
}

/// Run a kernel subsystem operation against the shared allocator.
pub fn with_global_allocator<R>(
    operation: impl FnOnce(&mut BitmapFrameAllocator<'static>) -> R,
) -> Option<R> {
    let mut allocator = GLOBAL_FRAME_ALLOCATOR.lock();
    allocator.as_mut().map(operation)
}
