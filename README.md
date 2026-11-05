# Kanga

Kanga is an independent x86-64 operating-system project. It is not a Linux distribution and is not a continuation of another kernel.

The kernel is planned in Rust with no_std and the Limine boot protocol.

## Initial milestone

Establish a reproducible freestanding build, a validated bootloader hand-off, serial diagnostics and a QEMU boot test. Memory management, interrupts, drivers, storage, processes and networking will be added as separate, testable subsystems.

## Build

The project uses the nightly Rust toolchain, rust-src and the x86_64-unknown-none target. The build command will be documented alongside the initial kernel implementation.

## Principles

- Keep Kanga's implementation and development history independent.
- Prefer explicit interfaces and small, testable subsystems.
- Do not claim hardware support until it is implemented and tested.
- Treat QEMU success as a baseline, not proof of physical-hardware compatibility.

## License

GPL-2.0-only. See LICENSE.