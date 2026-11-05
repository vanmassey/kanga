use limine::memory_map::{Entry, EntryType};

pub const PAGE_SIZE: u64 = 4096;
const LOW_MEMORY_RESERVED_END: u64 = 0x10_0000;
const FRAME_TEST_PATTERN: u64 = 0x4B41_4E47_4146_524D;

/// Convert a physical address through Limine's higher-half direct map offset.
pub fn physical_to_virtual(physical_address: u64, hhdm_offset: u64) -> Option<u64> {
    physical_address.checked_add(hhdm_offset)
}

/// Clear an allocated frame and verify that its direct-map address is writable.
///
/// # Safety
///
/// The virtual address must refer to a valid, exclusively owned 4 KiB frame
/// mapped through the higher-half direct map.
pub unsafe fn zero_and_verify_frame(virtual_address: u64) -> bool {
    let Ok(address) = usize::try_from(virtual_address) else {
        return false;
    };

    if address % PAGE_SIZE as usize != 0 {
        return false;
    }

    let frame_ptr = address as *mut u8;
    // SAFETY: The caller guarantees exclusive ownership of this mapped frame.
    unsafe {
        core::ptr::write_bytes(frame_ptr, 0, PAGE_SIZE as usize);
    }

    let test_ptr = frame_ptr.cast::<u64>();
    // SAFETY: The frame is aligned, mapped and exclusively owned by the caller.
    unsafe {
        test_ptr.write_volatile(FRAME_TEST_PATTERN);
    }
    // SAFETY: The preceding write targets the same valid frame.
    let observed = unsafe { test_ptr.read_volatile() };
    // SAFETY: Restore the tested word to zero before handing the frame onward.
    unsafe {
        test_ptr.write_volatile(0);
    }

    observed == FRAME_TEST_PATTERN
}

/// A monotonic allocator for page-aligned frames in usable physical regions.
///
/// Frames are handed out once and are not returned. A free-list and frame
/// reclamation will be added after Kanga has a kernel heap.
pub struct FrameAllocator<'a> {
    entries: &'a [&'a Entry],
    region_index: usize,
    next_frame: u64,
    region_end: u64,
}

impl<'a> FrameAllocator<'a> {
    pub const fn new(entries: &'a [&'a Entry]) -> Self {
        Self {
            entries,
            region_index: 0,
            next_frame: 0,
            region_end: 0,
        }
    }

    /// Allocate one 4 KiB physical frame, returning its physical address.
    pub fn allocate_frame(&mut self) -> Option<u64> {
        loop {
            if self.region_end != 0
                && self
                    .next_frame
                    .checked_add(PAGE_SIZE)
                    .is_some_and(|end| end <= self.region_end)
            {
                let frame = self.next_frame;
                self.next_frame = self.next_frame.checked_add(PAGE_SIZE)?;
                return Some(frame);
            }

            let entry = *self.entries.get(self.region_index)?;
            self.region_index += 1;

            if entry.entry_type != EntryType::USABLE {
                continue;
            }

            let Some(end) = entry.base.checked_add(entry.length) else {
                continue;
            };
            let region_start = entry.base.max(LOW_MEMORY_RESERVED_END);
            let Some(aligned_start) = region_start.checked_add(PAGE_SIZE - 1) else {
                continue;
            };
            let aligned_start = aligned_start & !(PAGE_SIZE - 1);
            let Some(first_frame_end) = aligned_start.checked_add(PAGE_SIZE) else {
                continue;
            };
            if first_frame_end > end {
                continue;
            }

            self.next_frame = aligned_start;
            self.region_end = end;
        }
    }
}
