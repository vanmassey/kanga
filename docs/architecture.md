# Kanga architecture

Kanga begins as a monolithic x86-64 kernel written in freestanding Rust. Its internal components will have clear interfaces so that architecture-specific code remains separate from portable kernel services.

## Implemented foundation

The current boot path is a single x86-64 kernel entry point. Limine supplies the memory map, higher-half direct-map offset and RGB framebuffer. The kernel reports boot information over COM1, performs an initial physical-frame allocation and memory access check, and uses a black framebuffer with a monochrome, DOS-style scrolling text console. The console uses compact 8x16 character cells, white fixed-width lettering and a simple text cursor. The console supports ASCII output, line wrapping, backspace, clear-screen and scrolling; keyboard input is delivered through IRQ1 from the legacy PS/2 controller. The IRQ handler only enqueues raw scancodes into a fixed-size atomic ring buffer; translation and shell processing happen in the main loop. Shell output is mirrored to COM1 for diagnostics. The kernel installs a 256-entry IDT using the active code-segment selector and configures exception gates for vectors 0–31. CPU exceptions with and without error codes have separate ABI-correct handlers; fatal exceptions are reported to serial and halt the CPU. The dual 8259 PIC is remapped to vectors 0x20–0x2F, with only IRQ1 unmasked. The keyboard IRQ gate acknowledges the master PIC after copying the scancode into the queue. APIC routing, timer IRQs and a general IRQ dispatcher remain unimplemented. There is no terminal escape-sequence support, desktop or windowing system yet.

## Planned components

| Component | Responsibility |
| --- | --- |
| Architecture | CPU setup, descriptor tables, interrupt controllers and low-level I/O |
| Boot | Bootloader information and hand-off validation |
| Memory | Physical frames, virtual address spaces and kernel heap |
| Interrupts | IDT, fatal CPU exception gates, remapped 8259 PIC and keyboard IRQ1 are implemented; APIC and general IRQ dispatch remain planned |
| Time | Hardware clock source and scheduler timer |
| Drivers | PCI discovery and device drivers |
| Tasks | Threads, processes and scheduling |
| VFS | Filesystem abstraction and mount points |
| Networking | Link, network and transport protocols |
| System calls | User/kernel boundary |

## Development sequence

1. Harden CPU and boot setup, including descriptor tables and exception diagnostics.
2. Build a general IRQ dispatcher with spurious IRQ handling; add APIC/IOAPIC support and a hardware timer.
3. Complete physical and virtual memory management, including page-table control, frame reclamation and a kernel heap.
4. Add PCI enumeration and reliable device drivers, starting with storage and virtualised QEMU devices.
5. Build a block-device layer, partition discovery, VFS and an initial filesystem.
6. Add kernel threads and scheduling, then user mode, system calls and an ELF loader.
7. Add networking in layers: Ethernet, ARP, IPv4, ICMP, UDP and TCP.
8. Build userland utilities and a more capable shell; consider a graphical environment only after the underlying systems are stable.
These are design plans, not claims that the components already exist.