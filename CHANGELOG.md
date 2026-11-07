# Changelog

All notable Kanga changes are recorded here.

## Unreleased (0.2.0 development)

### Added

- Initial x86-64 four-level virtual memory manager using the active CR3 page-table hierarchy.
- Page-table entry and 4 KiB-aligned page-table types, hardware mapping flags, 4 KiB map/unmap operations, virtual-to-physical translation and TLB invalidation.
- Allocation and zero-initialisation of missing intermediate page tables using the physical frame allocator.
- Boot-time virtual mapping, translation and unmapping self-test.

### Limitations

- The manager edits the active boot address space; independent address spaces and CR3 switching are not implemented.
- Empty page-table frames are not reclaimed.
- Page-table and physical allocator spin locks are not IRQ-safe.
- The no-execute flag requires EFER.NXE to be enabled before use.
- The physical frame bitmap still manages addresses below 4 GiB only.

## [0.1.0] - 2026-11-06

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
