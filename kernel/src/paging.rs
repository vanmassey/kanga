//! x86-64 four-level virtual memory management.
//!
//! Kanga currently boots with the page tables installed by Limine. This module
//! edits the active PML4 and allocates any missing lower-level tables from the
//! physical frame allocator. It deliberately supports 4 KiB mappings only;
//! existing 1 GiB and 2 MiB entries are detected and never overwritten.

use core::arch::asm;
use core::ops::{BitOr, BitOrAssign};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::memory::{self, PAGE_SIZE};

const ENTRIES_PER_TABLE: usize = 512;
const ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
const HUGE_PAGE: u64 = 1 << 7;
const NX: u64 = 1 << 63;

static HHDM_OFFSET: AtomicU64 = AtomicU64::new(0);
static INITIALISED: AtomicBool = AtomicBool::new(false);
static PAGE_TABLE_LOCK: AtomicBool = AtomicBool::new(false);

/// Hardware flags accepted when creating a 4 KiB page mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageFlags(u64);

impl PageFlags {
    pub const PRESENT: Self = Self(1 << 0);
    pub const WRITABLE: Self = Self(1 << 1);
    pub const USER: Self = Self(1 << 2);
    pub const WRITE_THROUGH: Self = Self(1 << 3);
    pub const CACHE_DISABLE: Self = Self(1 << 4);
    pub const ACCESSED: Self = Self(1 << 5);
    pub const DIRTY: Self = Self(1 << 6);
    pub const GLOBAL: Self = Self(1 << 8);
    pub const NO_EXECUTE: Self = Self(NX);
    const ALLOWED: u64 = 0x0000_0000_0000_0f7f | NX;

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for PageFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for PageFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// One x86-64 page-table entry (PTE).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const fn is_present(self) -> bool {
        self.0 & PageFlags::PRESENT.0 != 0
    }

    pub const fn is_writable(self) -> bool {
        self.0 & PageFlags::WRITABLE.0 != 0
    }

    pub const fn is_user_accessible(self) -> bool {
        self.0 & PageFlags::USER.0 != 0
    }

    pub const fn is_no_execute(self) -> bool {
        self.0 & NX != 0
    }

    pub const fn is_huge(self) -> bool {
        self.0 & HUGE_PAGE != 0
    }

    pub const fn physical_address(self) -> u64 {
        self.0 & ADDRESS_MASK
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    fn table(physical_address: u64, user: bool) -> Self {
        let mut value = physical_address | PageFlags::PRESENT.0 | PageFlags::WRITABLE.0;
        if user {
            value |= PageFlags::USER.0;
        }
        Self(value)
    }

    fn page(physical_address: u64, flags: PageFlags) -> Self {
        Self(physical_address | flags.0 | PageFlags::PRESENT.0)
    }
}

/// A 4 KiB-aligned x86-64 page table containing 512 entries.
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; ENTRIES_PER_TABLE],
}

impl PageTable {
    pub const fn zeroed() -> Self {
        Self {
            entries: [PageTableEntry(0); ENTRIES_PER_TABLE],
        }
    }

    pub fn entry(&self, index: usize) -> Option<PageTableEntry> {
        self.entries.get(index).copied()
    }

    pub const fn entry_count(&self) -> usize {
        ENTRIES_PER_TABLE
    }
}

/// Errors returned by virtual-memory operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingError {
    NotInitialised,
    NonCanonicalAddress,
    UnalignedAddress,
    PhysicalAddressOutOfRange,
    InvalidFlags,
    AlreadyMapped,
    NotMapped,
    HugePageConflict,
    OutOfFrames,
    AddressOverflow,
}

/// Initialise the manager with Limine's higher-half direct-map offset.
///
/// This does not replace the bootloader's active page tables. The manager uses
/// the current CR3 root and adds mappings to that address space.
pub fn initialise(hhdm_offset: u64) -> bool {
    if INITIALISED.load(Ordering::Acquire) {
        return false;
    }
    HHDM_OFFSET.store(hhdm_offset, Ordering::Relaxed);
    INITIALISED.store(true, Ordering::Release);
    true
}

/// Map one 4 KiB virtual page to a physical frame in the active address space.
pub fn map_page(
    virtual_address: u64,
    physical_address: u64,
    flags: PageFlags,
) -> Result<(), MappingError> {
    let _guard = PageTableGuard::lock();
    validate_addresses(virtual_address, physical_address)?;
    if !INITIALISED.load(Ordering::Acquire) {
        return Err(MappingError::NotInitialised);
    }
    if flags.0 & !PageFlags::ALLOWED != 0 {
        return Err(MappingError::InvalidFlags);
    }

    let indices = page_indices(virtual_address);
    let mut table = active_root()?;
    let user = flags.contains(PageFlags::USER);

    for index in indices.iter().take(3) {
        let entry = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[*index])) };
        if entry.is_present() {
            if entry.is_huge() {
                return Err(MappingError::HugePageConflict);
            }
            let mut promoted = entry.raw() | PageFlags::WRITABLE.0;
            if user {
                promoted |= PageFlags::USER.0;
            }
            if !flags.contains(PageFlags::NO_EXECUTE) {
                promoted &= !NX;
            }
            if promoted != entry.raw() {
                // SAFETY: This is a valid intermediate entry in the active hierarchy.
                unsafe {
                    core::ptr::write_volatile(
                        core::ptr::addr_of_mut!((*table).entries[*index]),
                        PageTableEntry(promoted),
                    );
                }
            }
            table = table_from_physical(entry.physical_address())?;
        } else {
            let frame = allocate_table_frame()?;
            let child = physical_to_table(frame)?;
            // SAFETY: The frame was freshly allocated and is exclusively owned.
            unsafe { core::ptr::write_bytes(child.cast::<u8>(), 0, PAGE_SIZE as usize) };
            let new_entry = PageTableEntry::table(frame, user);
            // SAFETY: This table is active and the entry is currently not present.
            unsafe {
                core::ptr::write_volatile(
                    core::ptr::addr_of_mut!((*table).entries[*index]),
                    new_entry,
                );
            }
            table = child;
        }
    }

    let leaf_index = indices[3];
    // SAFETY: table points to a valid, mapped page-table frame.
    let leaf = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[leaf_index])) };
    if leaf.is_present() {
        return Err(MappingError::AlreadyMapped);
    }

    let new_leaf = PageTableEntry::page(physical_address, flags);
    // SAFETY: The leaf entry belongs to the active page-table hierarchy.
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!((*table).entries[leaf_index]),
            new_leaf,
        );
    }
    flush_tlb(virtual_address);
    Ok(())
}

/// Remove a 4 KiB mapping and return the physical frame address.
///
/// Intermediate page tables are intentionally retained. Reclaiming them
/// safely requires tracking table ownership and proving that they are empty.
pub fn unmap_page(virtual_address: u64) -> Result<u64, MappingError> {
    let _guard = PageTableGuard::lock();
    if !is_canonical(virtual_address) {
        return Err(MappingError::NonCanonicalAddress);
    }
    if virtual_address % PAGE_SIZE != 0 {
        return Err(MappingError::UnalignedAddress);
    }
    if !INITIALISED.load(Ordering::Acquire) {
        return Err(MappingError::NotInitialised);
    }

    let indices = page_indices(virtual_address);
    let mut table = active_root()?;
    for index in indices.iter().take(3) {
        // SAFETY: table points to a valid, mapped page-table frame.
        let entry = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[*index])) };
        if !entry.is_present() {
            return Err(MappingError::NotMapped);
        }
        if entry.is_huge() {
            return Err(MappingError::HugePageConflict);
        }
        table = table_from_physical(entry.physical_address())?;
    }

    let leaf_index = indices[3];
    // SAFETY: table points to a valid, mapped page-table frame.
    let leaf = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[leaf_index])) };
    if !leaf.is_present() {
        return Err(MappingError::NotMapped);
    }
    // SAFETY: This clears only the requested leaf mapping.
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!((*table).entries[leaf_index]),
            PageTableEntry::default(),
        );
    }
    flush_tlb(virtual_address);
    Ok(leaf.physical_address())
}

/// Exercise creation, translation and removal of one temporary mapping.
///
/// The test uses a reserved high user-space address and returns its data frame
/// after unmapping. Intermediate page-table frames are retained because the
/// manager does not yet reclaim empty page tables.
pub fn self_test() -> bool {
    const TEST_VIRTUAL_ADDRESS: u64 = 0x0000_4000_0000_0000;

    if !matches!(translate(TEST_VIRTUAL_ADDRESS), Err(MappingError::NotMapped)) {
        return false;
    }

    let Some(frame) = memory::with_global_allocator(|allocator| allocator.allocate_frame()).flatten()
    else {
        return false;
    };

    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;
    if map_page(TEST_VIRTUAL_ADDRESS, frame, flags).is_err() {
        let _ = memory::with_global_allocator(|allocator| {
            // SAFETY: The frame was allocated by this test and was not mapped.
            unsafe { allocator.deallocate_frame(frame) }
        });
        return false;
    }

    let translated = translate(TEST_VIRTUAL_ADDRESS) == Ok(frame);
    let unmapped = unmap_page(TEST_VIRTUAL_ADDRESS) == Ok(frame);
    let returned = memory::with_global_allocator(|allocator| {
        // SAFETY: The test removed the mapping and retains ownership of the frame.
        unsafe { allocator.deallocate_frame(frame) }
    })
    .unwrap_or(false);

    translated && unmapped && returned
}

/// Translate a mapped virtual address to its physical address.
///
/// The returned address includes the original page offset. Existing 1 GiB
/// and 2 MiB boot mappings are understood; unmapped addresses return an error.
pub fn translate(virtual_address: u64) -> Result<u64, MappingError> {
    if !is_canonical(virtual_address) {
        return Err(MappingError::NonCanonicalAddress);
    }
    if !INITIALISED.load(Ordering::Acquire) {
        return Err(MappingError::NotInitialised);
    }

    let indices = page_indices(virtual_address);
    let mut table = active_root()?;
    for (level, index) in indices.iter().enumerate() {
        // SAFETY: table points to a valid, mapped page-table frame.
        let entry = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[*index])) };
        if !entry.is_present() {
            return Err(MappingError::NotMapped);
        }
        if entry.is_huge() {
            return match level {
                1 => Ok((entry.physical_address() & !((1 << 30) - 1))
                    | (virtual_address & ((1 << 30) - 1))),
                2 => Ok((entry.physical_address() & !((1 << 21) - 1))
                    | (virtual_address & ((1 << 21) - 1))),
                _ => Err(MappingError::HugePageConflict),
            };
        }
        if level == 3 {
            return Ok(entry.physical_address() | (virtual_address & (PAGE_SIZE - 1)));
        }
        table = table_from_physical(entry.physical_address())?;
    }
    Err(MappingError::NotMapped)
}

fn validate_addresses(virtual_address: u64, physical_address: u64) -> Result<(), MappingError> {
    if !is_canonical(virtual_address) {
        return Err(MappingError::NonCanonicalAddress);
    }
    if virtual_address % PAGE_SIZE != 0 || physical_address % PAGE_SIZE != 0 {
        return Err(MappingError::UnalignedAddress);
    }
    if physical_address & !ADDRESS_MASK != 0 {
        return Err(MappingError::PhysicalAddressOutOfRange);
    }
    Ok(())
}

const fn is_canonical(address: u64) -> bool {
    let sign = (address >> 47) & 1;
    let upper = address >> 48;
    (sign == 0 && upper == 0) || (sign == 1 && upper == 0xffff)
}

const fn page_indices(address: u64) -> [usize; 4] {
    [
        ((address >> 39) & 0x1ff) as usize,
        ((address >> 30) & 0x1ff) as usize,
        ((address >> 21) & 0x1ff) as usize,
        ((address >> 12) & 0x1ff) as usize,
    ]
}

fn active_root() -> Result<*mut PageTable, MappingError> {
    let root: u64;
    // SAFETY: Reading CR3 is valid in the privileged x86-64 kernel context.
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    table_from_physical(root & ADDRESS_MASK)
}

fn table_from_physical(physical_address: u64) -> Result<*mut PageTable, MappingError> {
    physical_to_table(physical_address)
}

fn physical_to_table(physical_address: u64) -> Result<*mut PageTable, MappingError> {
    let offset = HHDM_OFFSET.load(Ordering::Acquire);
    let virtual_address = memory::physical_to_virtual(physical_address, offset)
        .ok_or(MappingError::AddressOverflow)?;
    let address = usize::try_from(virtual_address).map_err(|_| MappingError::AddressOverflow)?;
    if address % PAGE_SIZE as usize != 0 {
        return Err(MappingError::UnalignedAddress);
    }
    Ok(address as *mut PageTable)
}

fn allocate_table_frame() -> Result<u64, MappingError> {
    let frame = memory::with_global_allocator(|allocator| allocator.allocate_frame())
        .flatten()
        .ok_or(MappingError::OutOfFrames)?;
    Ok(frame)
}

fn flush_tlb(virtual_address: u64) {
    // SAFETY: INVLPG is available in the x86-64 kernel privilege level.
    unsafe {
        asm!(
            "invlpg [{}]",
            in(reg) virtual_address,
            options(nostack, preserves_flags)
        );
    }
}

struct PageTableGuard;

impl PageTableGuard {
    fn lock() -> Self {
        while PAGE_TABLE_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while PAGE_TABLE_LOCK.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        Self
    }
}

impl Drop for PageTableGuard {
    fn drop(&mut self) {
        PAGE_TABLE_LOCK.store(false, Ordering::Release);
    }
}
