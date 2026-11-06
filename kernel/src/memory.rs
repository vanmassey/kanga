use limine::memory_map::{Entry, EntryType};

pub const PAGE_SIZE: u64 = 4096;
const LOW_MEMORY_RESERVED_END: u64 = 0x10_0000;
const FRAME_TEST_PATTERN: u64 = 0x4B41_4E47_4146_524D;

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

/// Intrusive singly linked list of free 4 KiB physical frames.
///
/// The first eight bytes of each free frame store the physical address of the
/// next free frame. Zero marks the end of the list. This needs no kernel heap.
pub struct FrameAllocator<'a> {
    entries: &'a [&'a Entry],
    hhdm_offset: u64,
    head: u64,
    free_frames: u64,
}

impl<'a> FrameAllocator<'a> {
    /// Build the free list from complete frames in Limine USABLE regions.
    ///
    /// The first MiB is excluded. Region boundaries are rounded inward to
    /// complete pages. The constructor writes a link into every free frame.
    ///
    /// # Safety
    /// hhdm_offset must map all supplied USABLE frames to writable memory.
    /// The memory map must remain valid for this allocator's lifetime.
    pub unsafe fn new(entries: &'a [&'a Entry], hhdm_offset: u64) -> Self {
        let mut allocator = Self {
            entries,
            hhdm_offset,
            head: 0,
            free_frames: 0,
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
                // SAFETY: The constructor contract guarantees writable HHDM
                // mappings and exclusive ownership of every USABLE frame.
                if unsafe { allocator.write_next(frame, allocator.head) } {
                    allocator.head = frame;
                    allocator.free_frames = allocator.free_frames.saturating_add(1);
                }
                let Some(next) = frame.checked_add(PAGE_SIZE) else {
                    break;
                };
                frame = next;
            }
        }
        allocator
    }

    /// Pop and return one free frame, or None when the list is empty.
    pub fn allocate_frame(&mut self) -> Option<u64> {
        if self.head == 0 {
            return None;
        }
        let frame = self.head;
        let next = self.read_next(frame)?;
        self.head = next;
        self.free_frames = self.free_frames.saturating_sub(1);
        Some(frame)
    }

    /// Push a previously allocated frame onto the free list.
    ///
    /// # Safety
    /// frame must be page-aligned, belong to a managed USABLE region, be
    /// currently allocated, and no longer be used by any other subsystem.
    /// It must not already be present in the free list.
    pub unsafe fn deallocate_frame(&mut self, frame: u64) -> bool {
        if !self.is_managed_frame(frame) || self.free_frames == u64::MAX {
            return false;
        }
        // SAFETY: The caller guarantees exclusive ownership of this frame.
        if !unsafe { self.write_next(frame, self.head) } {
            return false;
        }
        self.head = frame;
        self.free_frames += 1;
        true
    }

    pub const fn free_frame_count(&self) -> u64 {
        self.free_frames
    }

    fn is_managed_frame(&self, frame: u64) -> bool {
        if frame < LOW_MEMORY_RESERVED_END || frame % PAGE_SIZE != 0 {
            return false;
        }
        self.entries.iter().any(|entry| {
            if entry.entry_type != EntryType::USABLE {
                return false;
            }
            let Some(end) = entry.base.checked_add(entry.length) else {
                return false;
            };
            let start = entry.base.max(LOW_MEMORY_RESERVED_END);
            frame >= start
                && frame
                    .checked_add(PAGE_SIZE)
                    .is_some_and(|frame_end| frame_end <= end)
        })
    }

    /// # Safety
    /// frame must be a writable, exclusively owned HHDM frame.
    unsafe fn write_next(&self, frame: u64, next: u64) -> bool {
        let Some(virtual_address) = physical_to_virtual(frame, self.hhdm_offset) else {
            return false;
        };
        let Ok(address) = usize::try_from(virtual_address) else {
        return false;
    };
        if address % core::mem::align_of::<u64>() != 0 {
            return false;
        }
        // SAFETY: The caller guarantees a writable aligned word in this frame.
        unsafe { (address as *mut u64).write_volatile(next) };
        true
    }

    fn read_next(&self, frame: u64) -> Option<u64> {
        let virtual_address = physical_to_virtual(frame, self.hhdm_offset)?;
        let address = usize::try_from(virtual_address).ok()?;
        if address % core::mem::align_of::<u64>() != 0 {
            return None;
        }
        // SAFETY: The head is a frame inserted by this allocator and remains
        // mapped through the HHDM; its first word stores the next physical frame.
        Some(unsafe { (address as *const u64).read_volatile() })
    }
}
