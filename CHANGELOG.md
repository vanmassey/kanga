# Changelog

All notable Kanga changes are recorded here.

## [0.1.0] - Unreleased

First experimental milestone of the independent Kanga x86-64 operating-system project.

### Added

- Limine-based x86-64 boot path with boot diagnostics over COM1.
- Limine memory-map and higher-half direct-map requests.
- Static bitmap physical-frame allocator for 4 KiB frames below 4 GiB, including individual and contiguous allocation, deallocation validation and boot-time integrity checks.
- Shared frame-allocator service.
- 256-entry IDT and fatal CPU exception handlers for vectors 0–31.
- Legacy 8259 PIC initialisation, PS/2 keyboard IRQ1 and 100 Hz PIT timer IRQ0.
- CMOS real-time clock support, uptime reporting and a month calendar command.
- RGB framebuffer text console with an 8x16 VGA-compatible font and ANSI 16-colour SGR rendering.
- 128-line console scrollback using Up Arrow and Down Arrow.
- DOS-style interactive shell and serial output mirroring.
- KangaFS fixed-capacity RAM filesystem with directory listing, file creation, reading and deletion.
- QEMU automated boot, shell, memory allocator, framebuffer, colour, scrollback and filesystem checks.
- BIOS/UEFI ISO build scripts using a pinned Limine release with SHA-256 verification.

### Limitations

- KangaFS is volatile RAM storage; there is no persistent disk filesystem.
- No virtual-memory manager, kernel heap, multitasking, user mode or system-call interface.
- No PCI discovery, general IRQ dispatcher, APIC/IOAPIC, networking, USB stack or desktop.
- The physical-frame bitmap manages frames below 4 GiB.
- The shared allocator spin lock is not safe to acquire from an interrupt handler that can interrupt a lock holder.
- QEMU testing does not establish physical-hardware compatibility.

[0.1.0]: https://github.com/vanmassey/kanga/releases/tag/v0.1.0
