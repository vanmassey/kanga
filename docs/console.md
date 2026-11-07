# Console and shell interface

Kanga's framebuffer console uses the existing 8x16 VGA bitmap font and ANSI
16-colour palette. The shell keeps that font and palette while arranging its
startup display into a width-aware, bordered system-console layout. The
startup panel uses the available framebuffer width (up to 160 text columns)
rather than shrinking the text or forcing a narrow fixed-width layout.

The command reference groups commands by purpose: files and storage, system
information, and shell controls. The `MEM` command reports the current
physical-memory configuration and live kernel-heap statistics when available.

The framebuffer text console retains the most recent 128 logical lines in a
fixed-size ring buffer. Each stored cell keeps its character and ANSI foreground
and background palette values. The buffer is statically allocated and does not
depend on the kernel heap.

Use **Up Arrow** to move towards older output and **Down Arrow** to return to
the live prompt. New output continues behind a historical view without moving
the visible history. Typing, Backspace, or Enter returns to the live prompt
before editing or submitting a command.

The ring holds up to 512 columns per line and 128 lines. New output overwrites
the oldest retained line when the buffer fills. `CLS` clears the display and
its retained history.


## Kanga mascot and shell messages

The startup console includes a small ASCII Kanga mascot:

```text
 /\_/\
( o.o )
 >-v-<
 /   \
(_____)
```

Selected recoverable shell errors use kangaroo-themed wording while retaining
the useful diagnostic detail. For example, an unknown command is reported as
having possibly hopped off, and missing files are described as having hopped
out of KangaFS. These messages do not change command handling or error codes.
