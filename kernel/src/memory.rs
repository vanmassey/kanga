use limine::memory_map::{Entry, EntryType};

pub const PAGE_SIZE: u64 = 4096;
const LOW_MEMORY_RESERVED_END: u64 = 0x10_0000;

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
