# Console scrollback

Kanga's framebuffer text console retains the most recent 128 logical lines in a
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
