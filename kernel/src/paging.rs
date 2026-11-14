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
static NX_ENABLED: AtomicBool = AtomicBool::new(false);
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


/// An independently allocated PML4 root with the kernel half shared from the
/// boot address space. The lower 256 PML4 entries begin empty.
///
/// User mappings can be built and inspected while the root remains inactive.
/// CR3 switching and process-level ownership are added by later work.
pub struct AddressSpace {
    root_frame: u64,
}

impl AddressSpace {
    /// Physical address of this address space's PML4 root.
    pub const fn root_physical_address(&self) -> u64 {
        self.root_frame
    }

    /// Activate this address space by loading its PML4 physical address into CR3.
    ///
    /// Returns the previously active root so the caller can restore it before
    /// destroying this address space. The kernel half is shared with the boot
    /// address space, so the current kernel instruction and stack remain mapped.
    pub fn activate(&self) -> Result<u64, MappingError> {
        let _guard = PageTableGuard::lock();
        let previous: u64;
        // SAFETY: Reading CR3 is valid in ring 0; the low PCID bits are masked.
        unsafe {
            asm!("mov {}, cr3", out(reg) previous, options(nomem, nostack, preserves_flags));
        }
        let previous = previous & ADDRESS_MASK;
        let _ = physical_to_table(self.root_frame)?;
        // SAFETY: This object owns an aligned PML4 frame with a shared kernel
        // half. Loading it preserves the currently executing kernel mappings.
        unsafe {
            asm!("mov cr3, {}", in(reg) self.root_frame, options(nostack, preserves_flags));
        }
        Ok(previous)
    }

    /// Restore a previously saved PML4 root returned by activate.
    pub fn restore_root(root_frame: u64) -> Result<(), MappingError> {
        let _guard = PageTableGuard::lock();
        if root_frame & !ADDRESS_MASK != 0 || root_frame % PAGE_SIZE != 0 {
            return Err(MappingError::PhysicalAddressOutOfRange);
        }
        let _ = physical_to_table(root_frame)?;
        // SAFETY: The caller must supply a live root previously returned by
        // activate; validate its HHDM address before loading it into CR3.
        unsafe {
            asm!("mov cr3, {}", in(reg) root_frame, options(nostack, preserves_flags));
        }
        Ok(())
    }

    /// Count present entries in the shared higher-half kernel region.
    pub fn kernel_mapping_count(&self) -> usize {
        let Ok(root) = physical_to_table(self.root_frame) else {
            return 0;
        };
        (256..ENTRIES_PER_TABLE)
            .filter(|index| unsafe {
                core::ptr::read_volatile(core::ptr::addr_of!((*root).entries[*index]))
                    .is_present()
            })
            .count()
    }

    /// Confirm that every kernel-half PML4 entry still matches the boot root.
    pub fn kernel_mappings_match_active(&self) -> bool {
        let Ok(root) = physical_to_table(self.root_frame) else {
            return false;
        };
        let Ok(active) = active_root() else {
            return false;
        };
        (256..ENTRIES_PER_TABLE).all(|index| unsafe {
            core::ptr::read_volatile(core::ptr::addr_of!((*root).entries[index]))
                == core::ptr::read_volatile(core::ptr::addr_of!((*active).entries[index]))
        })
    }

    /// Confirm that the user half contains no mappings.
    pub fn user_half_is_empty(&self) -> bool {
        let Ok(root) = physical_to_table(self.root_frame) else {
            return false;
        };
        (0..256).all(|index| unsafe {
            !core::ptr::read_volatile(core::ptr::addr_of!((*root).entries[index])).is_present()
        })
    }

    /// Map one user page without changing the currently active CR3.
    ///
    /// The physical data frame remains owned by the caller. Intermediate
    /// page-table frames are owned by this address space and reclaimed by
    /// `destroy`.
    pub fn map_page(
        &mut self,
        virtual_address: u64,
        physical_address: u64,
        flags: PageFlags,
    ) -> Result<(), MappingError> {
        let _guard = PageTableGuard::lock();
        validate_user_address(virtual_address)?;
        if !flags.contains(PageFlags::USER) || flags.contains(PageFlags::GLOBAL) {
            return Err(MappingError::InvalidFlags);
        }
        map_page_in_root(
            physical_to_table(self.root_frame)?,
            virtual_address,
            physical_address,
            flags,
        )
    }

    /// Translate a user virtual address using this inactive page-table root.
    pub fn translate(&self, virtual_address: u64) -> Result<u64, MappingError> {
        validate_user_address(virtual_address)?;
        if !INITIALISED.load(Ordering::Acquire) {
            return Err(MappingError::NotInitialised);
        }
        translate_in_root(physical_to_table(self.root_frame)?, virtual_address)
    }

    /// Check that every paging level grants user-mode access to this page.
    pub fn mapping_is_user_accessible(&self, virtual_address: u64) -> bool {
        if validate_user_address(virtual_address).is_err() {
            return false;
        }
        let Ok(mut table) = physical_to_table(self.root_frame) else {
            return false;
        };
        let indices = page_indices(virtual_address);
        for (level, index) in indices.iter().enumerate() {
            // SAFETY: table points to a valid page-table frame owned/shared by this root.
            let entry =
                unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[*index])) };
            if !entry.is_present() || !entry.is_user_accessible() {
                return false;
            }
            if level == 3 {
                return true;
            }
            if entry.is_huge() {
                return false;
            }
            let Ok(next) = table_from_physical(entry.physical_address()) else {
                return false;
            };
            table = next;
        }
        false
    }

    /// Remove a user mapping and return its physical data-frame address.
    pub fn unmap_page(&mut self, virtual_address: u64) -> Result<u64, MappingError> {
        let _guard = PageTableGuard::lock();
        validate_user_address(virtual_address)?;
        unmap_page_in_root(virtual_address, physical_to_table(self.root_frame)?)
    }

    /// Release this root and all page-table frames owned by its user half.
    ///
    /// Leaf data frames are deliberately not freed: their ownership belongs
    /// to the caller that supplied them to `map_page`.
    pub fn destroy(self) -> bool {
        let _guard = PageTableGuard::lock();
        let Ok(root) = physical_to_table(self.root_frame) else {
            return false;
        };

        // Validate the complete tree before freeing anything, so malformed or
        // unsupported huge-page entries cannot cause a partial teardown.
        for index in 0..256 {
            // SAFETY: root is this address space's exclusively owned PML4.
            let entry =
                unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*root).entries[index])) };
            if entry.is_present() && !validate_user_table_tree(entry.physical_address(), 3) {
                return false;
            }
        }

        for index in 0..256 {
            // SAFETY: root remains exclusively owned while the lock is held.
            let entry =
                unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*root).entries[index])) };
            if entry.is_present() && !release_user_table_tree(entry.physical_address(), 3) {
                return false;
            }
        }

        memory::with_global_allocator(|allocator| {
            // SAFETY: This object uniquely owns the PML4 root frame.
            unsafe { allocator.deallocate_frame(self.root_frame) }
        })
        .unwrap_or(false)
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
    NoExecuteUnavailable,
    KernelAddressProtected,
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
    NX_ENABLED.store(enable_nx_protection(), Ordering::Release);
    INITIALISED.store(true, Ordering::Release);
    true
}


/// Create an inactive address space with a private PML4 and shared kernel half.
///
/// Entries 256–511 are copied from the active boot root. Entries 0–255 remain
/// zero, so no bootloader identity or user mappings are inherited.
pub fn create_address_space() -> Result<AddressSpace, MappingError> {
    let _guard = PageTableGuard::lock();
    if !INITIALISED.load(Ordering::Acquire) {
        return Err(MappingError::NotInitialised);
    }

    let active = active_root()?;
    let frame = allocate_table_frame()?;
    let Ok(root) = physical_to_table(frame) else {
        let _ = memory::with_global_allocator(|allocator| {
            // SAFETY: The newly allocated root frame has not been published.
            unsafe { allocator.deallocate_frame(frame) }
        });
        return Err(MappingError::AddressOverflow);
    };

    // SAFETY: The frame is newly allocated, HHDM-mapped and exclusively owned.
    unsafe {
        core::ptr::write_bytes(root.cast::<u8>(), 0, PAGE_SIZE as usize);
        for index in 256..ENTRIES_PER_TABLE {
            let entry = core::ptr::read_volatile(core::ptr::addr_of!((*active).entries[index]));
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*root).entries[index]), entry);
        }
    }

    Ok(AddressSpace { root_frame: frame })
}

/// Verify that two independently allocated roots share only the kernel half.
pub fn address_space_self_test() -> bool {
    let first = match create_address_space() {
        Ok(space) => space,
        Err(_) => return false,
    };
    let second = match create_address_space() {
        Ok(space) => space,
        Err(_) => {
            let _ = first.destroy();
            return false;
        }
    };

    let passed = first.root_physical_address() != second.root_physical_address()
        && first.kernel_mapping_count() > 0
        && first.kernel_mapping_count() == second.kernel_mapping_count()
        && first.kernel_mappings_match_active()
        && second.kernel_mappings_match_active()
        && first.user_half_is_empty()
        && second.user_half_is_empty();

    let first_released = first.destroy();
    let second_released = second.destroy();
    passed && first_released && second_released
}

/// Verify a user mapping in an inactive root is isolated from the active root.
pub fn address_space_mapping_self_test() -> bool {
    const TEST_VIRTUAL_ADDRESS: u64 = 0x0000_2000_0000_0000;

    if !matches!(translate(TEST_VIRTUAL_ADDRESS), Err(MappingError::NotMapped)) {
        return false;
    }

    let Some(frame) =
        memory::with_global_allocator(|allocator| allocator.allocate_frame()).flatten()
    else {
        return false;
    };
    let mut space = match create_address_space() {
        Ok(space) => space,
        Err(_) => {
            let _ = memory::with_global_allocator(|allocator| {
                // SAFETY: The test frame was allocated and never mapped.
                unsafe { allocator.deallocate_frame(frame) }
            });
            return false;
        }
    };

    let flags = PageFlags::USER | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;
    if space
        .map_page(TEST_VIRTUAL_ADDRESS, frame, flags)
        .is_err()
    {
        let _ = memory::with_global_allocator(|allocator| {
            // SAFETY: Mapping failed, so the test still owns the data frame.
            unsafe { allocator.deallocate_frame(frame) }
        });
        let _ = space.destroy();
        return false;
    }

    let isolated = space.translate(TEST_VIRTUAL_ADDRESS) == Ok(frame)
        && space.mapping_is_user_accessible(TEST_VIRTUAL_ADDRESS)
        && matches!(translate(TEST_VIRTUAL_ADDRESS), Err(MappingError::NotMapped));
    let unmapped = space.unmap_page(TEST_VIRTUAL_ADDRESS) == Ok(frame);
    let absent_after_unmap = matches!(
        space.translate(TEST_VIRTUAL_ADDRESS),
        Err(MappingError::NotMapped)
    );
    let returned = if unmapped {
        memory::with_global_allocator(|allocator| {
            // SAFETY: The user mapping was removed and the test retains the data frame.
            unsafe { allocator.deallocate_frame(frame) }
        })
        .unwrap_or(false)
    } else {
        false
    };
    let destroyed = space.destroy();

    isolated && unmapped && absent_after_unmap && returned && destroyed
}

/// Report whether hardware no-execute protection was successfully enabled.
pub fn no_execute_enabled() -> bool {
    NX_ENABLED.load(Ordering::Acquire)
}

/// Enable EFER.NXE only after confirming that the processor advertises NX.
///
/// Returning false leaves the paging manager usable for ordinary mappings;
/// callers must not request NO_EXECUTE mappings in that configuration.
fn enable_nx_protection() -> bool {
    // SAFETY: CPUID is available in long mode and these leaves are queried only
    // after checking the maximum supported extended leaf.
    let maximum = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    if maximum < 0x8000_0001 {
        return false;
    }
    // SAFETY: The extended feature leaf is supported by the preceding check.
    let features = core::arch::x86_64::__cpuid(0x8000_0001);
    if features.edx & (1 << 20) == 0 {
        return false;
    }

    const EFER_MSR: u32 = 0xC000_0080;
    let low: u32;
    let high: u32;
    // SAFETY: NX support implies the architectural EFER MSR is available.
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") EFER_MSR,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack)
        );
    }
    let efer = u64::from(low) | (u64::from(high) << 32);
    let enabled_efer = efer | (1 << 11);
    // SAFETY: Preserve every existing EFER bit and set only the documented NXE bit.
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") EFER_MSR,
            in("eax") enabled_efer as u32,
            in("edx") (enabled_efer >> 32) as u32,
            options(nomem, nostack)
        );
    }

    let verified_low: u32;
    let verified_high: u32;
    // SAFETY: Read back the same architectural EFER MSR to verify the change.
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") EFER_MSR,
            out("eax") verified_low,
            out("edx") verified_high,
            options(nomem, nostack)
        );
    }
    (u64::from(verified_low) | (u64::from(verified_high) << 32)) & (1 << 11) != 0
}

/// Map one 4 KiB virtual page to a physical frame in the active address space.
pub fn map_page(
    virtual_address: u64,
    physical_address: u64,
    flags: PageFlags,
) -> Result<(), MappingError> {
    let _guard = PageTableGuard::lock();
    let root = active_root()?;
    map_page_in_root(root, virtual_address, physical_address, flags)
}

/// Remove a 4 KiB mapping from the active address space and return its frame.
pub fn unmap_page(virtual_address: u64) -> Result<u64, MappingError> {
    let _guard = PageTableGuard::lock();
    let root = active_root()?;
    unmap_page_in_root(virtual_address, root)
}

/// Exercise creation, translation and removal of one temporary mapping.
///
/// The test uses a reserved high user-space address and returns its data frame
/// after unmapping. Intermediate page-table frames are retained because the
/// manager does not yet reclaim empty page tables.
pub fn self_test() -> bool {
    const TEST_VIRTUAL_ADDRESS: u64 = 0x0000_4000_0000_0000;

    if !matches!(
        translate(TEST_VIRTUAL_ADDRESS),
        Err(MappingError::NotMapped)
    ) {
        return false;
    }

    let Some(frame) =
        memory::with_global_allocator(|allocator| allocator.allocate_frame()).flatten()
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
    // Never return the frame to the allocator if the mapping may still exist.
    let returned = if unmapped {
        memory::with_global_allocator(|allocator| {
            // SAFETY: The test removed the mapping and retains ownership of the frame.
            unsafe { allocator.deallocate_frame(frame) }
        })
        .unwrap_or(false)
    } else {
        false
    };

    translated && unmapped && returned
}

/// Translate a mapped virtual address in the active address space.
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
    translate_in_root(active_root()?, virtual_address)
}


/// Map a page in a supplied root. The caller must hold PAGE_TABLE_LOCK.
fn map_page_in_root(
    mut table: *mut PageTable,
    virtual_address: u64,
    physical_address: u64,
    flags: PageFlags,
) -> Result<(), MappingError> {
    validate_addresses(virtual_address, physical_address)?;
    if !INITIALISED.load(Ordering::Acquire) {
        return Err(MappingError::NotInitialised);
    }
    if flags.0 & !PageFlags::ALLOWED != 0 {
        return Err(MappingError::InvalidFlags);
    }
    if flags.contains(PageFlags::NO_EXECUTE) && !no_execute_enabled() {
        return Err(MappingError::NoExecuteUnavailable);
    }

    let indices = page_indices(virtual_address);
    let user = flags.contains(PageFlags::USER);

    for index in indices.iter().take(3) {
        // SAFETY: table points to a valid page-table frame.
        let entry =
            unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[*index])) };
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
                // SAFETY: This is a valid intermediate entry in this hierarchy.
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
            let child = match physical_to_table(frame) {
                Ok(child) => child,
                Err(error) => {
                    let _ = memory::with_global_allocator(|allocator| {
                        // SAFETY: The new table frame has not been published.
                        unsafe { allocator.deallocate_frame(frame) }
                    });
                    return Err(error);
                }
            };
            // SAFETY: The frame was freshly allocated and is exclusively owned.
            unsafe { core::ptr::write_bytes(child.cast::<u8>(), 0, PAGE_SIZE as usize) };
            let new_entry = PageTableEntry::table(frame, user);
            // SAFETY: This table is valid and the entry is currently not present.
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
    let leaf =
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[leaf_index])) };
    if leaf.is_present() {
        return Err(MappingError::AlreadyMapped);
    }

    let new_leaf = PageTableEntry::page(physical_address, flags);
    // SAFETY: The leaf entry belongs to the selected page-table hierarchy.
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!((*table).entries[leaf_index]),
            new_leaf,
        );
    }
    flush_tlb(virtual_address);
    Ok(())
}

/// Remove a page from a supplied root. The caller must hold PAGE_TABLE_LOCK.
fn unmap_page_in_root(
    virtual_address: u64,
    mut table: *mut PageTable,
) -> Result<u64, MappingError> {
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
    for index in indices.iter().take(3) {
        // SAFETY: table points to a valid page-table frame.
        let entry =
            unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[*index])) };
        if !entry.is_present() {
            return Err(MappingError::NotMapped);
        }
        if entry.is_huge() {
            return Err(MappingError::HugePageConflict);
        }
        table = table_from_physical(entry.physical_address())?;
    }

    let leaf_index = indices[3];
    // SAFETY: table points to a valid page-table frame.
    let leaf =
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[leaf_index])) };
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

/// Translate a virtual address in a supplied root.
fn translate_in_root(
    mut table: *mut PageTable,
    virtual_address: u64,
) -> Result<u64, MappingError> {
    let indices = page_indices(virtual_address);
    for (level, index) in indices.iter().enumerate() {
        // SAFETY: table points to a valid page-table frame.
        let entry =
            unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[*index])) };
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

fn validate_user_address(virtual_address: u64) -> Result<(), MappingError> {
    if !is_canonical(virtual_address) {
        return Err(MappingError::NonCanonicalAddress);
    }
    if virtual_address >= (1_u64 << 47) {
        return Err(MappingError::KernelAddressProtected);
    }
    Ok(())
}

/// Check an owned user page-table subtree without modifying it.
fn validate_user_table_tree(frame: u64, level: usize) -> bool {
    let Ok(table) = physical_to_table(frame) else {
        return false;
    };
    if level == 1 {
        return true;
    }
    (0..ENTRIES_PER_TABLE).all(|index| {
        // SAFETY: table is a valid page-table frame in this owned subtree.
        let entry =
            unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[index])) };
        !entry.is_present()
            || (!entry.is_huge()
                && validate_user_table_tree(entry.physical_address(), level - 1))
    })
}

/// Free page-table frames in an owned subtree, never freeing mapped data pages.
fn release_user_table_tree(frame: u64, level: usize) -> bool {
    let Ok(table) = physical_to_table(frame) else {
        return false;
    };
    if level > 1 {
        for index in 0..ENTRIES_PER_TABLE {
            // SAFETY: table is a valid page-table frame in this owned subtree.
            let entry =
                unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*table).entries[index])) };
            if entry.is_present()
                && !release_user_table_tree(entry.physical_address(), level - 1)
            {
                return false;
            }
        }
    }
    memory::with_global_allocator(|allocator| {
        // SAFETY: This frame is an intermediate page table owned by the address space.
        unsafe { allocator.deallocate_frame(frame) }
    })
    .unwrap_or(false)
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

struct PageTableGuard {
    restore_interrupts: bool,
}

impl PageTableGuard {
    fn lock() -> Self {
        // Prevent a local interrupt handler from re-entering the page-table
        // manager while this CPU owns its lock.
        let interrupt_flags: u64;
        unsafe {
            asm!("pushfq", "pop {}", "cli", out(reg) interrupt_flags);
        }

        while PAGE_TABLE_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while PAGE_TABLE_LOCK.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        Self {
            restore_interrupts: interrupt_flags & (1 << 9) != 0,
        }
    }
}

impl Drop for PageTableGuard {
    fn drop(&mut self) {
        PAGE_TABLE_LOCK.store(false, Ordering::Release);
        if self.restore_interrupts {
            // SAFETY: Restore IF only if it was set before acquiring the lock.
            unsafe { asm!("sti", options(nomem, nostack)) };
        }
    }
}
