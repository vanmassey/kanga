# Memory subsystem

## Boot-time memory map

Kanga requests the Limine memory map and walks the returned entries without allocating. Only entries classified as usable are candidates for physical frame allocation. The kernel reports the entry count, usable-region count and total usable bytes over COM1.

## Bitmap physical frame allocator

Kanga uses a bitmap to track 4 KiB physical frames. One bit represents one frame: a set bit means unavailable and a clear bit means free.

- The bitmap is stored in the kernel image's static `.bss` storage, not in the frames it manages.
- The bitmap contains 262,144 64-bit words and tracks physical addresses below 64 GiB (16,777,216 frames). Its static `.bss` footprint is 2 MiB.
- The first MiB of physical address space is reserved.
- Only complete, page-aligned frames inside Limine USABLE regions are marked free.
- Reserved, kernel, module and bootloader-reclaimable entries remain unavailable.
- Allocation uses a next-fit scan and updates one bit per allocated frame.
- Deallocation validates alignment, managed-region membership and the current allocation bit, rejecting duplicate frees.
- A contiguous-run allocator can reserve adjacent frames for future page-table and DMA-related work.
- Usable frames at or above the bitmap's 64 GiB limit are counted and reported, but are not allocated.

The allocator requires no heap. Allocation scans the bitmap, so it is O(number of bitmap words) in the worst case rather than O(1). It is retained as a kernel-wide service and serialises access with a spin mutex across CPUs. The mutex saves the local interrupt-enable state, disables maskable interrupts before waiting for the lock, and restores the previous state after releasing it. This prevents a local maskable interrupt from deadlocking by re-entering the allocator. NMI handlers must not acquire this lock. The deallocation function is unsafe because callers must return only frames they own and must stop using them before returning them.

## Higher-half direct map

Kanga requests Limine's higher-half direct map (HHDM). The response supplies an offset that can be added to a physical address to obtain its direct-map virtual address while the bootloader's page tables remain active.

The conversion helper checks for integer overflow. It does not itself prove that an arbitrary physical address is mapped or safe to access; callers must own the corresponding memory and ensure it is covered by the direct map.

## x86-64 virtual memory manager

The virtual memory manager in `kernel/src/paging.rs` operates on the active four-level x86-64 page-table hierarchy supplied at boot. It reads CR3 to locate the PML4 and uses the HHDM to access physical page-table frames.

- `PageTableEntry` represents a hardware entry and exposes its present, writable, user, no-execute, huge-page and physical-address fields.
- `PageTable` is a 4 KiB-aligned structure containing 512 entries.
- `PageFlags` provides present, writable, user, write-through, cache-disable, accessed, dirty, global and no-execute flags.
- Missing intermediate tables are allocated as physical frames, zeroed and linked into the active hierarchy.
- `map_page` installs a 4 KiB mapping after checking canonical virtual addresses, alignment, physical-address width and flags.
- `unmap_page` clears a 4 KiB leaf mapping and returns its physical frame address. It does not free that frame.
- `translate` walks the hierarchy and translates 4 KiB, 2 MiB and 1 GiB mappings.
- Mapping changes invalidate the affected virtual address with INVLPG.
- Existing huge-page entries are detected; the 4 KiB mapping API will not overwrite them.

The manager is initialised after the physical frame allocator and HHDM are ready. A boot-time self-test creates a temporary mapping, verifies translation, removes the mapping and returns its data frame. The intermediate page-table frames created by that test are retained.

The manager's page-table lock and the physical allocator's lock disable local maskable interrupts while held, preventing ordinary IRQ handlers from pre-empting a lock holder and re-entering either subsystem. They remain spin locks for cross-CPU exclusion; NMI handlers must not acquire them. The manager currently edits the active boot address space; it does not yet create independent process address spaces or reclaim empty page-table frames. The no-execute flag also depends on the CPU's EFER.NXE feature being enabled; callers must not request it until that architectural setup is implemented.

## Boot-time allocator checks

During QEMU boot, Kanga allocates three distinct frames, verifies direct-map memory access, returns and reuses a frame, deliberately attempts a duplicate free, allocates a three-frame contiguous run, returns all diagnostic frames, and checks that the final free-frame count matches the initial count. The virtual-memory self-test additionally exercises page-table creation, mapping, translation and unmapping. The QEMU smoke test checks the serial output for successful diagnostics.

## Remaining work

- extending physical-frame management beyond the current 64 GiB bitmap limit;
- independent address spaces and safe CR3 switching;
- safe reclamation of empty page-table frames;
- architectural setup and validation for the no-execute feature;
- reclaiming unused heap pages and their data frames; and
- controlled handling of bootloader-reclaimable memory after the bootloader data is no longer needed.

## Framebuffer and graphical boot screen

Kanga requests a framebuffer from Limine and uses the first supplied display. The renderer accepts 32-bit RGB framebuffers and validates dimensions and pitch. Pixel offsets use the hardware pitch, and scrolling copies one physical framebuffer row at a time using that pitch, including any row padding. The console renders an 8x16 VGA-compatible font and supports wrapping, backspace, clearing, scrolling and ANSI SGR foreground/background colours. It remains a basic text console without cursor positioning, true-colour SGR, graphics acceleration, multiple-display support or mode setting.

## Kernel heap

The kernel heap reserves a dedicated 16 MiB virtual range and initially maps sixteen 4 KiB pages (64 KiB). Each page is backed by an independently allocated physical frame, so the heap does not require physically contiguous memory. When an allocation cannot fit, the heap maps another 64 pages (256 KiB), up to its 16 MiB limit. Mapping is checked against the active address space before use; partial growth is rolled back by removing leaf mappings and returning their data frames. The paging manager currently retains empty intermediate page tables, including after rollback.

The allocator provides zero-initialising allocation, alignment support, free-list splitting, deallocation and adjacent-block coalescing. The `statistics()` interface returns a lock-consistent `HeapStats` snapshot containing mapped capacity, total free bytes, largest free block, free-block count, occupied bytes and fragmented free bytes. The boot-time self-test includes a 96 KiB allocation to force heap growth, checks page alignment and zeroing, rejects a mismatched deallocation layout, exercises reuse, and verifies that freeing the test allocations coalesces the heap back into one free block.

This is deliberately not registered as Rust's process-wide `GlobalAlloc` yet. Kernel subsystems must opt in explicitly until allocation is safe in all interrupt, panic and early-boot contexts. The heap has a fixed 16 MiB virtual limit for now; demand growth, physical-frame reclamation and safe page-table reclamation are separate concerns.
