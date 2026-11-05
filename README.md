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
- requests a Limine RGB framebuffer, draws a graphical boot splash, then switches to a scrolling framebuffer text console; and\n- accepts interrupt-driven PS/2 keyboard input through IRQ1 in a small interactive shell, with shell output mirrored to the serial console.

The framebuffer renderer currently supports 32-bit RGB modes and uses a small built-in bitmap font. Its text console supports ASCII output, wrapping, backspace, clearing and scrolling. The shell supports `help`, `clear`, `info`, `version`, `mem` and `halt`. The kernel now installs a 256-entry IDT with gates for CPU exceptions 0–31 and fatal handlers that report an exception (and error code where applicable) over serial before halting. The 8259 PIC is remapped to vectors 0x20–0x2F, with only keyboard IRQ1 unmasked. The IRQ handler copies scancodes into a fixed-size lock-free queue; translation and shell processing happen outside interrupt context. Other IRQs remain masked; APIC support, timer interrupts, terminal escape sequences and a desktop are not implemented. The physical frame allocator is an initial monotonic allocator. It reserves the first MiB of physical address space and does not yet support freeing or reclaiming frames. See docs/memory.md for the current design and limitations.

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

This opens a graphical QEMU window. Kanga displays its boot splash and then an interactive text console; serial diagnostics and shell output remain connected to the terminal.

The ISO build downloads a pinned Limine 12.9.6 binary release and verifies its SHA-256 checksum before use.

## Automated validation

GitHub Actions checks formatting, builds the ISO, boots it in QEMU and verifies the kernel entry point, IDT installation, memory map, frame allocation, HHDM conversion, frame memory test, framebuffer console and shell startup on the serial console.

## Project principles

- Keep Kanga's implementation and development history independent.
- Prefer explicit interfaces and small, testable subsystems.
- Do not claim hardware support until it is implemented and tested.
- Treat QEMU success as a baseline, not proof of physical-hardware compatibility.

## License

GPL-2.0-only. See LICENSE.
