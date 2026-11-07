# Console and shell interface

Kanga's framebuffer console uses the existing 8x16 VGA bitmap font and ANSI
16-colour palette. The shell startup display uses the available framebuffer
width (up to 160 text columns) without reducing the font size or changing the
palette.

The command reference groups commands by purpose: files and storage, system
information, and shell controls. The `MEM` command reports the current
physical-memory configuration and live kernel-heap statistics when available.

The framebuffer text console retains the most recent 128 logical lines in a
fixed-size ring buffer. Each stored cell keeps its character and ANSI
foreground and background palette values. The buffer is statically allocated
and does not depend on the kernel heap.

- **Up / Down:** browse previously entered commands. The history retains the
  eight most recent non-empty commands that fit on one console row.
- **Page Up / Page Down:** move through terminal scrollback.
- **Backspace:** delete the last character in the current input.
- **Enter:** execute the current command.

The ring holds up to 512 columns per line and 128 lines. New output overwrites
the oldest retained line when the buffer fills. `CLS` clears the display and
its retained history.
