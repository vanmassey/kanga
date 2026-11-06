# Kanga

Kanga is an independent x86-64 operating-system project. It is not a Linux distribution and is not a continuation of another kernel.

The kernel is written in Rust with `no_std` and uses the Limine boot protocol.

## Current state

Kanga currently provides:

- x86-64 BIOS/UEFI boot through Limine, with base-revision validation;
- COM1 serial diagnostics and mirrored shell output;
- Limine memory-map reporting and a bitmap physical-frame allocator for page-aligned 4 KiB frames below 4 GiB;
- a higher-half direct-map address conversion helper and boot-time frame read/write checks;
- a 32-bit RGB framebuffer console with an 8x16 VGA-compatible font;
- a black console background, light-grey default text, and ANSI SGR standard/bright 16-colour foreground and background rendering;
- line wrapping, backspace, clear-screen, a prompt cursor, and 128 lines of scrollback;
- interrupt-driven PS/2 keyboard input through IRQ1;
- a 256-entry IDT with fatal CPU exception handlers for vectors 0–31;
- a remapped legacy 8259 PIC, with keyboard IRQ1 and PIT timer IRQ0 enabled;
- a 100 Hz PIT tick counter, uptime reporting and a CMOS real-time clock reader; and
- KangaFS, a bounded RAM filesystem with a root directory, sixteen file slots, filenames up to 24 characters and file contents up to 256 bytes.

The shell includes `HELP`, `DIR`, `CLS`, `VER`, `ECHO`, `INFO`, `MEM`, `TIME`, `DATE` (also `CLOCK`/`RTC`), `UPTIME`, `ABOUT`, `CAL`, `HALT`, `FSINFO`, `WRITE`, `TYPE` and `DEL`. Commands are case-insensitive. KangaFS contents exist only in RAM and are lost on reboot.

The console retains up to 128 logical lines. Use **Up Arrow** to browse older output and **Down Arrow** to return towards the live prompt. Typing, Backspace or Enter returns to the live prompt. The renderer currently supports ANSI SGR palette colours only; cursor movement, terminal erase commands and 24-bit true colour are not implemented.

## Build requirements

- Rust nightly, with `rust-src` and the `x86_64-unknown-none` target (requested by `rust-toolchain.toml`)
- `make` and a C compiler, to build Limine's host utility
- `curl` and `sha256sum`
- `xorriso`
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

This opens a QEMU window showing Kanga's framebuffer text console. Serial diagnostics and shell output remain connected to the terminal.

The ISO build downloads a pinned Limine 12.9.6 binary release and verifies its SHA-256 checksum before use.

## Automated validation

GitHub Actions checks shell-script syntax and Rust formatting, builds the bootable ISO, and boots Kanga in QEMU. It injects shell commands through the virtual keyboard and verifies boot diagnostics, memory allocator checks, clock output, RAM filesystem operations and shell output over serial. It captures live, HELP, scrollback and ANSI-colour framebuffer screenshots, checks that the images are non-blank, and verifies the expected ANSI palette colours. The workflow uploads the ISO, serial log and screenshots as an artifact.

### Inspecting a saved serial log

The host-side serial log inspector checks for expected boot checkpoints and obvious fatal markers. It does not start QEMU or modify the kernel:

~~~sh
python3 scripts/inspect-serial.py build/serial.log
~~~

Its standard-library tests can be run independently:

~~~sh
python3 -m unittest discover -s tests -v
~~~

A successful inspection only confirms that the listed messages appear in the saved log; it is not a substitute for reviewing the log, checking the framebuffer, or testing the kernel in QEMU.

## Known limitations

Kanga 0.1.0 is an experimental kernel milestone, not a general-purpose operating system. It has no virtual-memory manager or kernel heap, process or thread scheduler, user mode, system-call interface, persistent disk filesystem, PCI device discovery, general IRQ dispatcher, APIC/IOAPIC support, networking, USB stack, desktop or graphical window system. The physical-frame bitmap manages addresses below 4 GiB. The shared allocator uses a spin lock that must not be acquired from an interrupt handler that can interrupt a lock holder. QEMU validation does not establish compatibility with physical machines; hardware support must be tested separately.

## Project principles

- Keep Kanga's implementation and development history independent.
- Prefer explicit interfaces and small, testable subsystems.
- Do not claim hardware support until it is implemented and tested.
- Treat QEMU success as a baseline, not proof of physical-hardware compatibility.

## License

GPL-2.0-only. See [LICENSE](LICENSE).
