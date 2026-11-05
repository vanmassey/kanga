#!/usr/bin/env python3
"""Validate the framebuffer screenshot produced by the QEMU integration test."""
import pathlib
import sys

data = pathlib.Path(sys.argv[1]).read_bytes()
if not data.startswith(b"P6"):
    raise SystemExit("framebuffer screenshot is not a binary PPM (P6)")
parts = data.split(maxsplit=4)
if len(parts) != 5:
    raise SystemExit("malformed PPM header")
width, height, maximum = map(int, parts[1:4])
pixels = parts[4]
if maximum != 255 or len(pixels) != width * height * 3:
    raise SystemExit("unexpected PPM pixel format or truncated screenshot")
if width < 640 or height < 400:
    raise SystemExit(f"framebuffer unexpectedly small: {width}x{height}")
lit = sum(1 for i in range(0, len(pixels), 3)
          if pixels[i] > 80 or pixels[i + 1] > 80 or pixels[i + 2] > 80)
if lit < 100:
    raise SystemExit(f"framebuffer appears blank: only {lit} lit pixels")
print(f"Framebuffer screenshot validated: {width}x{height}, {lit} lit pixels")
