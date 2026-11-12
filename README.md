# Kanga

Kanga is an independent x86-64 operating-system project. It is not a Linux distribution and is not a continuation of another kernel.

The kernel is written in Rust with `no_std` and uses the Limine boot protocol.

## Current state

Kanga currently provides:

- x86-64 BIOS/UEFI boot through Limine, with base-revision validation;
- COM1 serial diagnostics and mirrored shell output;
- Limine memory-map reporting and a bitmap physical-frame allocator for page-aligned 4 KiB frames below 4 GiB;
- a higher-half direct-map address conversion helper and boot-time frame read/write checks;
- an x86-64 four-level virtual memory manager using the active CR3 page-table hierarchy;
- page-table entry and page-table types, 4 KiB mapping and unmapping, virtual-to-physical translation, and TLB invalidation;
- boot-time virtual mapping, translation and unmapping validation;
- a 32-bit RGB framebuffer text console below the boot mascot, using 8x16 cells and a compact built-in ASCII font;
- a black console background, light-grey default text, and ANSI SGR standard/bright 16-colour foreground/background rendering;
- line wrapping, backspace, clear-screen and framebuffer scrolling;
- interrupt-driven PS/2 keyboard input through IRQ1, including Shift, Caps Lock, Backspace and arrow keys;
- a 256-entry IDT with fatal CPU exception handlers for vectors 0–31;
- a remapped legacy 8259 PIC, with keyboard IRQ1 and PIT timer IRQ0 enabled;
- a 100 Hz PIT tick counter, uptime reporting and a CMOS real-time clock reader; and
- KangaFS, a bounded RAM filesystem with a root directory, sixteen file slots, filenames up to 24 characters and file contents up to 256 bytes.

The shell includes `HELP`, `DIR`, `CLS`, `VER`, `ECHO`, `INFO`, `MEM`, `TIME`, `DATE` (also `CLOCK`/`RTC`), `UPTIME`, `ABOUT`, `CAL`, `HALT`, `FSINFO`, `WRITE`, `TYPE`, `COPY` and `DEL`. Commands are case-insensitive. KangaFS contents exist only in RAM and are lost on reboot.

The shell retains the eight most recent commands. Use **Up Arrow** and **Down Arrow** to browse command history. The framebuffer renderer supports ASCII text, line wrapping, scrolling, backspace, clear-screen and basic ANSI SGR palette colours. It does not yet provide output scrollback, arbitrary cursor positioning or 24-bit true colour.

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

This opens a QEMU window showing the Kanga mascot above the Kash framebuffer shell. Type directly into the QEMU window. Shell output is also mirrored to the host terminal over COM1 for diagnostics.

The ISO build downloads a pinned Limine 12.9.6 binary release and verifies its SHA-256 checksum before use.

## Automated validation

GitHub Actions checks shell-script syntax and Rust formatting, builds the bootable ISO, and boots Kanga in QEMU. It injects shell commands through the virtual keyboard and verifies boot diagnostics, physical and virtual memory allocator checks, clock output, RAM filesystem operations and shell output over serial. It captures live, HELP, scrollback and ANSI-colour framebuffer screenshots, checks that the images are non-blank, and verifies the expected ANSI palette colours. The workflow uploads the ISO, serial log and screenshots as an artifact.

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

Kanga 0.1.0 is an experimental kernel milestone, not a general-purpose operating system. The current 0.2.0 development branch has an initial virtual-memory manager, but it does not yet provide independent process address spaces, safe page-table reclamation, a kernel heap, multitasking, user mode or a system-call interface. Other missing components include a persistent disk filesystem, PCI device discovery, a general IRQ dispatcher, APIC/IOAPIC support, networking, USB stack, desktop or graphical window system. The physical-frame bitmap manages addresses below 4 GiB. The physical allocator and page-table manager use spin locks that must not be acquired from an interrupt handler that can interrupt a lock holder. The no-execute PTE flag must not be used until EFER.NXE is enabled. QEMU validation does not establish compatibility with physical machines; hardware support must be tested separately.

## Development roadmap

Development is moving towards **Kanga 0.2.0**. The next milestone focuses on kernel reliability, interrupt-safe synchronisation, completing virtual memory and a kernel heap, PCI device discovery, and storage/filesystem foundations.

See the [Kanga 0.2.0 roadmap](ROADMAP.md) for planned work and completion criteria. These are development goals, not claims that the features are already implemented.

## Contributing

Contributions in code, testing and documentation are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and check the [open issues](https://github.com/vanmassey/kanga/issues) before starting work. For substantial architectural changes, open an issue first so the design can be discussed.

## Project principles

- Keep Kanga's implementation and development history independent.
- Prefer explicit interfaces and small, testable subsystems.
- Do not claim hardware support until it is implemented and tested.
- Treat QEMU success as a baseline, not proof of physical-hardware compatibility.

## License

GPL-2.0-only. See [LICENSE](LICENSE).
