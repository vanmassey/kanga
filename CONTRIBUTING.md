# Contributing to Kanga OS

Thanks for your interest in Kanga! 🦘 Kanga is an early-stage, experimental x86-64 operating system written in Rust. Contributions of all sizes are welcome: bug reports, documentation fixes, tests, and kernel code.

By taking part, you agree to be respectful and constructive in all discussions. Assume good faith, keep feedback about the code rather than the person, and remember that many contributors are here to learn.

## Ways to contribute

- **Report bugs.** Boot failures, crashes, wrong output and unexpected behaviour are all useful.
- **Test on real or virtual hardware.** QEMU is the main test target. Reports from other emulators or physical machines are especially valuable, since QEMU success does not guarantee hardware compatibility.
- **Improve documentation.** Fixes to `README.md`, `docs/` and code comments are always welcome.
- **Fix issues or add features.** See the areas listed below.

## Reporting bugs

Please open a GitHub issue and include:

1. The Kanga version or commit SHA.
2. How you ran it (QEMU version and flags, or hardware model, BIOS or UEFI).
3. What you expected and what actually happened.
4. The serial log (`serial.log`) and, if relevant, a screenshot.
5. Steps to reproduce.

Search existing issues first to avoid duplicates.

## Getting set up

Build and boot instructions live in [`README.md`](README.md), which is the source of truth. In short, you will need:

- The Rust toolchain pinned in `rust-toolchain.toml` (rustup picks it up automatically).
- The tools the `Makefile` and `scripts/` need to produce a bootable Limine ISO.
- QEMU to boot the result.

Please build the ISO and boot it in QEMU before opening a pull request. Architecture notes are in [`docs/architecture.md`](docs/architecture.md), with more in `docs/console.md` and `docs/memory.md`.

## Making changes

1. **Fork** the repository and create a branch from `main`, for example `fix-keyboard-repeat` or `add-pci-scan`.
2. **Keep changes focused.** One logical change per pull request is much easier to review. Avoid mixing refactors, formatting and new features.
3. **Match the existing style.** Run `cargo fmt` before committing; CI checks formatting.
4. **Write clear commit messages.** Use a short imperative summary line, such as "Redraw only the exposed row when scrolling", with detail in the body when the reason is not obvious.
5. **Document as you go.** Update `docs/`, the `CHANGELOG.md` and built-in `HELP` text when behaviour changes.

### Kernel code guidelines

- Kanga is `no_std`. Keep dependencies to a minimum and justify any new one.
- Prefer safe Rust. Where `unsafe` is necessary (port I/O, page tables, raw framebuffer access), keep the block as small as possible and add a `// SAFETY:` comment explaining why it is sound.
- Be mindful of interrupt context. The current spin lock is **not** IRQ-safe, so do not take a lock in a handler that thread-level code may also hold.
- Log useful boot and runtime diagnostics to the serial port; CI relies on the serial log.
- Do not introduce unrelated changes to kernel code in documentation or release-only pull requests.

## Testing

Every push is checked by the **Kanga boot diagnostics validation** GitHub Actions workflow, which:

- validates the shell scripts,
- checks Rust formatting,
- builds the bootable ISO,
- boots it in QEMU,
- exercises keyboard IRQ input, and
- uploads the ISO, serial log and framebuffer screenshots as artifacts.

Pull requests should pass this workflow. If it fails, check the logs and artifacts first. If you believe the failure is not caused by your change, say so in the pull request. If you change behaviour the tests cover (boot output, the help screen, scrollback, colours), explain how the tests or expectations should change.

## Submitting a pull request

1. Push your branch to your fork and open a pull request against `main`.
2. Describe **what** changed and **why**, and link any related issue.
3. Note how you tested it (QEMU configuration, BIOS or UEFI, any hardware).
4. Be ready to iterate. Review comments are about making the project better, not about you.

Please do not force-push over a branch while a reviewer is mid-review unless asked; add follow-up commits instead and squash at merge time if requested.

## Areas where help is wanted

Kanga 0.1.0 is deliberately small. Known gaps include:

- A kernel heap and general-purpose virtual memory manager
- Persistent storage for KangaFS (it currently lives in RAM)
- IRQ-safe locking and a more modern interrupt controller than the legacy PIC
- PCI device discovery
- User mode, system calls and a scheduler
- Frame allocation beyond the 4 GiB limit
- More automated tests and better documentation

If you plan something large, **open an issue first** so we can discuss the design before you invest the time.

## Licence

Kanga is distributed under the **GNU General Public License, version 2**. By submitting a contribution, you agree that your work is licensed under the same terms (see [`LICENSE`](LICENSE)). Only submit code you have the right to contribute, and do not copy code from sources with incompatible licences.

Thank you for helping Kanga grow!