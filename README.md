# Kanga

Kanga is an independent x86-64 operating-system project. It is not a Linux distribution and is not a continuation of another kernel.

The kernel is written in Rust with no_std and uses the Limine boot protocol.

## Current state

Kanga currently:

- boots through Limine on x86-64;
- validates the Limine base revision;
- initialises a COM1 serial console;
- reports kernel identity and boot progress;
- reads and summarises the Limine physical memory map;
- allocates a page-aligned 4 KiB physical frame from usable memory;
- requests the higher-half direct map and converts the frame address; and
- clears the allocated frame and verifies memory access with a write/read test;
- requests a Limine RGB framebuffer and presents a black-and-white DOS-style scrolling text console; and
- accepts interrupt-driven PS/2 keyboard input through IRQ1 in a small interactive shell, with shell output mirrored to the serial console.

The framebuffer renderer currently supports 32-bit RGB modes and uses a compact fixed-width bitmap font in 8x16 text cells. The console uses a black background, white text and a simple underline cursor; it supports ASCII output, wrapping, backspace, clearing and scrolling. The shell supports DOS-style `DIR`, `CLS`, `VER` and `ECHO`, alongside `HELP`, `INFO`, `MEM` and `HALT`. Commands are case-insensitive. `DIR` honestly reports that no filesystem is mounted yet; it does not invent directory contents. The kernel now installs a 256-entry IDT with gates for CPU exceptions 0–31 and fatal handlers that report an exception (and error code where applicable) over serial before halting. The 8259 PIC is remapped to vectors 0x20–0x2F, with only keyboard IRQ1 unmasked. The IRQ handler copies scancodes into a fixed-size lock-free queue; translation and shell processing happen outside interrupt context. Other IRQs remain masked; APIC support, timer interrupts, terminal escape sequences and a desktop are not implemented. The physical frame allocator is an initial monotonic allocator. It reserves the first MiB of physical address space and does not yet support freeing or reclaiming frames. See docs/memory.md for the current design and limitations.

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

The resulting image is `build/kanga.iso`.

Run it in QEMU:

~~~sh
make run
~~~

This opens a QEMU window showing Kanga's monochrome DOS-style text console. Serial diagnostics and shell output remain connected to the terminal.

The ISO build downloads a pinned Limine 12.9.6 binary release and verifies its SHA-256 checksum before use.

## Automated validation

GitHub Actions checks formatting, builds the ISO, boots it in QEMU, injects the `version` command through QEMU's virtual keyboard, and verifies the response `Kanga OS 0.1.0` over the serial console. This exercises the PS/2 IRQ1 input path as well as the boot sequence.

## Project principles

- Keep Kanga's implementation and development history independent.
- Prefer explicit interfaces and small, testable subsystems.
- Do not claim hardware support until it is implemented and tested.
- Treat QEMU success as a baseline, not proof of physical-hardware compatibility.

## License

GPL-2.0-only. See LICENSE.
