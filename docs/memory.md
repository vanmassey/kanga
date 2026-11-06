# Memory subsystem

## Boot-time memory map

Kanga requests the Limine memory map and walks the returned entries without allocating. Only entries classified as usable are candidates for physical frame allocation. The kernel reports the entry count, usable-region count and total usable bytes over COM1.

## Physical frame allocator

Kanga uses an intrusive singly linked free list for 4 KiB physical frames.

- Each free frame stores the physical address of the next free frame in its first eight bytes.
- The allocator stores the physical address of the list head and a count of free frames.
- Allocation removes the head frame and reads its next pointer through the higher-half direct map.
- Deallocation writes the old head into the returned frame and makes that frame the new head.
- Both operations are O(1).
- The list is built from complete, page-aligned frames in Limine USABLE regions only.
- The first MiB of physical address space is excluded.
- Kernel, module, reserved and bootloader-reclaimable memory-map entries are not added.
- The allocator does not require a kernel heap or separately allocated metadata.

The allocator's metadata lives inside the free frames themselves. While a frame is free, its first eight bytes belong to the allocator and must not be used by another subsystem. Once allocated, the frame is owned by the caller and its contents may be used normally.

The allocator currently assumes single-CPU access. A lock or other synchronisation must be added before multiple CPUs can allocate or free frames concurrently. The deallocation function is unsafe because callers must return only a frame that they own, exactly once, and must stop using it before returning it.

## Higher-half direct map

Kanga requests Limine's higher-half direct map (HHDM). The response supplies an offset that can be added to a physical address to obtain its direct-map virtual address while the bootloader's page tables remain active.

The conversion helper checks for integer overflow. It does not itself prove that an arbitrary physical address is mapped or safe to access; callers must own the corresponding memory and ensure it is covered by the direct map.

## Boot-time allocator checks

During QEMU boot, Kanga builds the free list, allocates three frames and checks that their addresses are distinct, verifies direct-map memory access, returns the middle frame and checks that the next allocation reuses it, then returns the diagnostic frames and reports the final free-frame count. The QEMU smoke test checks the serial output for successful allocation, memory-access, deallocation/reuse and integrity diagnostics.

## Remaining work

- synchronisation for multiple CPUs;
- tracking allocated frames to detect double frees and invalid ownership;
- support for allocating contiguous runs of frames;
- controlled handling of bootloader-reclaimable memory after the bootloader data is no longer needed;
- a page-table abstraction and controlled mapping changes; and
- a kernel heap backed by allocated frames.

## Framebuffer and graphical boot screen

Kanga requests a framebuffer from Limine and uses the first supplied display. The initial renderer accepts 32-bit RGB framebuffers, validates the dimensions and pitch, and draws a simple status screen directly into the mapped framebuffer. A compact built-in 5-by-7 bitmap font renders the boot labels without a heap or external font dependency.

The renderer is deliberately small. It does not yet provide a terminal, scrolling, keyboard input, graphics acceleration, multiple-display support or mode setting. The serial console remains the authoritative diagnostic output. Unsupported framebuffer formats are reported over serial and do not prevent the kernel from continuing to its idle loop.
