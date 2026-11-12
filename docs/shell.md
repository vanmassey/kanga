# kash: the Kanga serial shell

kash is an interactive command shell that runs over COM1. It gives the 0.2.0
kernel a way to be inspected and exercised while it runs, and it makes KangaFS
usable for the first time on the development branch.

## Running it

~~~sh
make run
~~~

`scripts/run-qemu.sh` uses `-serial stdio`, so the terminal that launched QEMU
is the shell. After boot diagnostics and the scheduler self-test you will see:

~~~text
Welcome to kash, the Kanga shell. Type HELP for commands.
kash>
~~~

Headless, for example in CI:

~~~sh
python3 scripts/shell-smoke.py --verbose
~~~

## Commands

Commands are case-insensitive. Several have DOS-style and Unix-style names.

| Command | Aliases | Purpose |
| --- | --- | --- |
| `HELP` | `?` | List commands |
| `VER` | `VERSION` | Kernel version |
| `INFO` | `UNAME` | Version, architecture, uptime and memory summary |
| `UPTIME` | | Time since boot and PIT tick count |
| `DATE` | `TIME`, `CLOCK`, `RTC` | CMOS real-time clock |
| `MEM` | | Physical frame allocator usage |
| `TASKS` | | Scheduler tasks, context switches and worker tick counters |
| `DIR` | `LS` | List KangaFS files |
| `TYPE <file>` | `CAT` | Print a file |
| `WRITE <file> <text>` | | Create or replace a file |
| `APPEND <file> <text>` | | Append to a file, creating it if needed |
| `COPY <src> <dst>` | `CP` | Copy a file, replacing the destination |
| `DEL <file>` | `RM` | Delete a file |
| `FSINFO` | `DF` | Files used, bytes stored and per-file limit |
| `HEXDUMP <file>` | | Offset, hex and ASCII dump |
| `ECHO <text>` | | Print text |
| `HISTORY` | | Last eight distinct commands |
| `CLS` | `CLEAR` | Clear the terminal |
| `KANGA` | | Redraw the framebuffer mascot |
| `SELFTEST` | | Re-run the paging and heap self-tests |
| `REBOOT` | | Pulse the 8042 reset line (QEMU exits under `-no-reboot`) |
| `HALT` | | Stop the CPU |
| `POWEROFF` | | ACPI S5 request on QEMU and Bochs; otherwise halts |

KangaFS limits apply: sixteen files, names up to 24 characters made of letters,
digits, `.`, `_` and `-`, and 256 bytes per file. Contents live in RAM only.

## Line editing

| Key | Action |
| --- | --- |
| Backspace / Delete | Erase the previous character |
| Up / Down arrows | Recall older / newer history entries |
| Ctrl-U | Erase the whole line |
| Ctrl-C | Abandon the line and show a fresh prompt |
| Ctrl-L | Clear the screen and redraw the prompt and line |

Lines are limited to 128 characters. CR, LF and CRLF all end a line.

## Design

| File | Role |
| --- | --- |
| `kernel/src/console.rs` | Polled COM1 receive path (`read_byte`) |
| `kernel/src/shell.rs` | Line editor, history, command dispatch and commands |
| `kernel/src/power.rs` | Reset, halt and power-off helpers |
| `kernel/src/filesystem.rs` | KangaFS, now wired into the kernel as the shell's storage |

The shell value is owned by `idle()` in `main.rs` and runs on the bootstrap
thread. It starts once the scheduler runtime self-test has been reported, or
after one second if the scheduler is unavailable. Nothing in the shell allocates
or touches the heap; its buffers (line, history and KangaFS) live in the
`Shell` value, roughly 6 KiB.

### Why input is polled

The idle loop executes `hlt`, and the 100 Hz PIT wakes it. Each time the
bootstrap thread runs it drains the 16550 receive FIFO. Because the scheduler
shares the CPU with two spinning workers, the bootstrap thread runs about every
30 ms. That is ample for typing and QEMU applies serial flow control, but a
very fast paste into a real UART could overrun its 16-byte FIFO.

## Limitations and next steps

- Receive is polled; an IRQ4 handler with a lock-free ring buffer would remove
  the FIFO overrun risk and allow the idle thread to sleep until input arrives.
- The framebuffer shows only the mascot; there is no on-screen console or PS/2
  keyboard input on this branch.
- Commands run to completion on the bootstrap thread and cannot be interrupted
  other than by Ctrl-C at the prompt.
- No pipes, redirection, quoting or tab completion.
- File names are case-sensitive for `WRITE` and `DEL` but not for reading,
  which is inherited from KangaFS.
- `SELFTEST` re-runs boot tests that were written to run once.
