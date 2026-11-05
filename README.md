# Kanga

Kanga is an independent x86-64 operating-system project. It is not a Linux distribution and is not a continuation of another kernel.

The kernel is written in Rust with no_std and uses the Limine boot protocol.

## Current state

Kanga currently:

- boots through Limine on x86-64;
- validates the Limine base revision;
- initialises a COM1 serial console;
- reports kernel identity and boot progress;
- reads and summarises the Limine physical memory map; and
- allocates its first 4 KiB physical frame from usable memory.

The physical frame allocator is an initial monotonic allocator. It reserves the first MiB of physical address space and does not yet support freeing or reclaiming frames.

## Build requirements

- Rust nightly, with rust-src and the x86_64-unknown-none target (requested by rust-toolchain.toml)
- make and a C compiler, to build Limine's host utility
- curl and sha256sum
- xorriso
- QEMU x86-64 system emulator (for running the image)

## Build and run

Build only the kernel:

~~~sh
make build
~~~

Create a bootable BIOS/UEFI ISO:

~~~sh
make iso
~~~

The resulting image is build/kanga.iso.

Run it in QEMU:

~~~sh
make run
~~~

The ISO build downloads a pinned Limine 12.9.6 binary release and verifies its SHA-256 checksum before use.

## Automated validation

GitHub Actions checks formatting, builds the ISO, boots it in QEMU and verifies that the kernel entry point and memory-map reporting are visible on the serial console.

## Project principles

- Keep Kanga's implementation and development history independent.
- Prefer explicit interfaces and small, testable subsystems.
- Do not claim hardware support until it is implemented and tested.
- Treat QEMU success as a baseline, not proof of physical-hardware compatibility.

## License

GPL-2.0-only. See LICENSE.
