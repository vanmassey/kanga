# Kanga architecture

Kanga begins as a monolithic x86-64 kernel written in freestanding Rust. Its internal components will have clear interfaces so that architecture-specific code remains separate from portable kernel services.

## Implemented foundation

The current boot path is a single x86-64 kernel entry point. Limine supplies the memory map, higher-half direct-map offset and RGB framebuffer. The kernel reports boot information over COM1, performs an initial physical-frame allocation and memory access check, and draws a non-interactive framebuffer splash screen. The splash screen is diagnostic output, not yet a terminal or windowing system.

## Planned components

| Component | Responsibility |
| --- | --- |
| Architecture | CPU setup, descriptor tables, interrupt controllers and low-level I/O |
| Boot | Bootloader information and hand-off validation |
| Memory | Physical frames, virtual address spaces and kernel heap |
| Interrupts | Exception and interrupt dispatch |
| Time | Hardware clock source and scheduler timer |
| Drivers | PCI discovery and device drivers |
| Tasks | Threads, processes and scheduling |
| VFS | Filesystem abstraction and mount points |
| Networking | Link, network and transport protocols |
| System calls | User/kernel boundary |

## Development sequence

1. Establish a repeatable freestanding build and boot pipeline.
2. Capture and validate the bootloader memory map.
3. Implement physical frame allocation and page-table management.
4. Add exception handling, interrupt controllers and a timer.
5. Introduce kernel allocation, device discovery and storage.
6. Add scheduling, user mode and system calls.
7. Build filesystem and networking interfaces.
8. Add a text console and keyboard input, then build a user environment and graphical stack.

These are design plans, not claims that the components already exist.