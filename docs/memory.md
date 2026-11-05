# Memory subsystem

## Boot-time memory map

Kanga requests the Limine memory map and walks the returned entries without allocating. Only entries classified as usable are candidates for physical frame allocation. The kernel reports the entry count, usable-region count and total usable bytes over COM1.

## Physical frame allocator

The initial allocator is monotonic:

- frame size is 4 KiB;
- returned addresses are page-aligned physical addresses;
- only usable memory-map entries are considered;
- the first MiB of physical address space is excluded;
- arithmetic that could overflow is checked; and
- allocated frames are not returned to the allocator.

This is a bootstrap allocator, not a complete physical memory manager. It has no free operation, ownership tracking, SMP lock or reclamation of bootloader-reclaimable memory.

## Higher-half direct map

Kanga requests Limine's higher-half direct map (HHDM). The response supplies an offset that can be added to a physical address to obtain its direct-map virtual address while the bootloader's page tables remain active.

The conversion helper checks for integer overflow. It does not itself prove that an arbitrary physical address is mapped or safe to access; callers must own the corresponding memory and ensure it is covered by the direct map.

## Boot-time frame check

During QEMU boot, Kanga:

1. obtains a usable physical frame;
2. converts it to a direct-map virtual address;
3. clears the 4 KiB frame;
4. writes and reads a test pattern; and
5. restores the test word to zero.

The QEMU smoke test checks for the successful frame-allocation, HHDM-address and memory-test messages.

## Remaining work

- frame deallocation and reclamation;
- allocator synchronisation for multiple CPUs;
- a page-table abstraction and controlled mapping changes;
- a kernel heap backed by allocated frames; and
- tests for fragmented memory maps and allocation exhaustion.


## Framebuffer and graphical boot screen

Kanga requests a framebuffer from Limine and uses the first supplied display. The
initial renderer accepts 32-bit RGB framebuffers, validates the dimensions and
pitch, and draws a simple status screen directly into the mapped framebuffer.
A compact built-in 5-by-7 bitmap font renders the boot labels without a heap or
external font dependency.

The renderer is deliberately small. It does not yet provide a terminal, scrolling,
keyboard input, graphics acceleration, multiple-display support or mode setting.
The serial console remains the authoritative diagnostic output. Unsupported
framebuffer formats are reported over serial and do not prevent the kernel from
continuing to its idle loop.
