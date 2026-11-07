# Kanga architecture

Kanga is a monolithic x86-64 kernel written in freestanding Rust. Its internal components are being developed behind explicit interfaces so architecture-specific code can remain separate from portable kernel services.

## Implemented foundation

The boot path is a single x86-64 kernel entry point. Limine supplies the memory map, higher-half direct-map offset and RGB framebuffer. Kanga reports boot information over COM1 and performs boot-time physical-frame allocation and direct-map memory-access checks.

The framebuffer console supports 32-bit RGB modes and renders an 8x16 VGA-compatible font. It uses a black background and light-grey default text, with ANSI SGR standard and bright 16-colour foreground/background palettes. It supports ASCII output, line wrapping, backspace, clear-screen, a prompt cursor and a fixed 128-line scrollback ring. Up Arrow moves towards older output; Down Arrow returns towards the live prompt. The renderer interprets SGR colour sequences only: cursor positioning, erase commands, true-colour and other terminal control sequences are not implemented.

The PS/2 keyboard is delivered through legacy IRQ1. The interrupt handler copies raw scancodes into a fixed-size atomic queue; translation and shell processing happen in the main loop. Shell output is mirrored to COM1 for diagnostics.

The kernel installs a 256-entry IDT with ABI-correct exception gates for CPU exception vectors 0–31. Fatal exceptions are reported over serial before the processor halts. The dual 8259 PIC is remapped to vectors 0x20–0x2F, with IRQ0 (PIT) and IRQ1 (keyboard) unmasked. The PIT channel 0 runs at 100 Hz; its handler increments an atomic tick counter and acknowledges the master PIC. The shell reports uptime and timer ticks. A CMOS RTC reader waits for update cycles to finish, takes matching snapshots, handles BCD/binary and 12/24-hour modes, validates calendar fields and exposes the clock through DATE. APIC routing and a general IRQ dispatcher remain unimplemented.

The physical frame allocator uses a static bitmap for page-aligned 4 KiB frames below 4 GiB. It reserves the first MiB, supports individual and contiguous allocations, rejects duplicate frees and performs boot-time allocation, reuse, contiguous-run and memory-access checks. The shared allocator service serialises access with a spin lock. This lock is not IRQ-safe: code must not acquire it from an interrupt handler that could interrupt a lock holder.

The virtual-memory subsystem uses the active CR3 root and a four-level x86-64 page-table walk. It provides PTE and page-table types, creates missing intermediate tables using the physical frame allocator, maps and unmaps 4 KiB pages, translates virtual addresses (including existing huge-page mappings), and invalidates the TLB after changes. A boot-time self-test exercises a temporary mapping. The page-table lock is also not IRQ-safe. Independent address spaces, safe page-table reclamation, and a kernel heap are not implemented. See [memory.md](memory.md) for details and limitations.

KangaFS is a fixed-capacity in-memory filesystem with a root directory, sixteen file slots, filenames up to 24 characters and file contents up to 256 bytes. Its contents are volatile and are lost when the kernel stops or reboots.

## Planned components

| Component | Responsibility |
| --- | --- |
| Architecture | CPU setup, descriptor tables, interrupt controllers and low-level I/O |
| Memory | Independent virtual address spaces, frame reclamation and a kernel heap |
| Interrupts | General IRQ dispatch, spurious IRQ handling and APIC/IOAPIC support |
| Drivers | PCI discovery and device drivers |
| Tasks | Kernel threads, processes and scheduling |
| VFS | Filesystem abstraction, block devices, partitions and persistent filesystems |
| Networking | Link, network and transport protocols |
| System calls | User/kernel boundary |

## Development sequence

1. Harden CPU and boot setup, including descriptor tables and exception diagnostics.
2. Build a general IRQ dispatcher with spurious IRQ handling; add APIC/IOAPIC support when appropriate.
3. Complete physical and virtual memory management, including independent address spaces, safe page-table reclamation and a kernel heap.
4. Add PCI enumeration and reliable device drivers, starting with storage and virtualised QEMU devices.
5. Build a block-device layer, partition discovery, VFS and an initial persistent filesystem.
6. Add kernel threads and scheduling, then user mode, system calls and an ELF loader.
7. Add networking in layers: Ethernet, ARP, IPv4, ICMP, UDP and TCP.
8. Build userland utilities and a more capable shell; consider a graphical environment only after the underlying systems are stable.

These are design plans, not claims that the components already exist.
