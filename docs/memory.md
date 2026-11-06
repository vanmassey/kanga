# Memory subsystem

## Boot-time memory map

Kanga requests the Limine memory map and walks the returned entries without allocating. Only entries classified as usable are candidates for physical frame allocation. The kernel reports the entry count, usable-region count and total usable bytes over COM1.

## Bitmap physical frame allocator

Kanga uses a bitmap to track 4 KiB physical frames. One bit represents one frame: a set bit means unavailable and a clear bit means free.

- The bitmap is stored in the kernel image's static `.bss` storage, not in the frames it manages.
- The current bitmap contains 16,384 64-bit words and tracks physical addresses below 4 GiB (1,048,576 frames).
- The first MiB of physical address space is reserved.
- Only complete, page-aligned frames inside Limine USABLE regions are marked free.
- Reserved, kernel, module and bootloader-reclaimable entries remain unavailable.
- Allocation uses a next-fit scan and updates one bit per allocated frame.
- Deallocation validates alignment, managed-region membership and the current allocation bit, rejecting duplicate frees.
- A contiguous-run allocator can reserve adjacent frames for future page-table and DMA-related work.
- Usable frames above the bitmap's 4 GiB limit are counted and reported, but are not allocated.

The allocator requires no heap. Allocation scans the bitmap, so it is O(number of bitmap words) in the worst case rather than O(1). It is retained as a kernel-wide service and serialises access with a spin mutex across CPUs. The lock must not be acquired from an interrupt handler that can interrupt code already holding it; an IRQ-aware locking path is still required. The deallocation function is unsafe because callers must return only frames they own and must stop using them before returning them.

## Higher-half direct map

Kanga requests Limine's higher-half direct map (HHDM). The response supplies an offset that can be added to a physical address to obtain its direct-map virtual address while the bootloader's page tables remain active.

The conversion helper checks for integer overflow. It does not itself prove that an arbitrary physical address is mapped or safe to access; callers must own the corresponding memory and ensure it is covered by the direct map.

## Boot-time allocator checks

During QEMU boot, Kanga allocates three distinct frames, verifies direct-map memory access, returns and reuses a frame, deliberately attempts a duplicate free, allocates a three-frame contiguous run, returns all diagnostic frames, and checks that the final free-frame count matches the initial count. The QEMU smoke test checks the serial output for successful allocation, memory-access, double-free rejection, contiguous allocation and allocator-integrity diagnostics.

## Remaining work

- IRQ-safe locking and allocation entry points;
- allocating frames above the current 4 GiB bitmap limit;
- a page-table abstraction and controlled mapping changes;
- mapping and unmapping virtual pages with TLB invalidation;
- a kernel heap backed by allocated frames; and
- controlled handling of bootloader-reclaimable memory after the bootloader data is no longer needed.

## Framebuffer and graphical boot screen

Kanga requests a framebuffer from Limine and uses the first supplied display. The renderer accepts 32-bit RGB framebuffers and validates dimensions and pitch. Pixel offsets use the hardware pitch, and scrolling copies one physical framebuffer row at a time using that pitch, including any row padding. The console renders an 8x16 VGA-compatible font and supports wrapping, backspace, clearing, scrolling and ANSI SGR foreground/background colours. It remains a basic text console without cursor positioning, true-colour SGR, graphics acceleration, multiple-display support or mode setting.
