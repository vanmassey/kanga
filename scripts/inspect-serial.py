#!/usr/bin/env python3
"""Summarise Kanga's serial boot log without changing or running the kernel.

Usage:
    python3 scripts/inspect-serial.py build/serial.log
"""

from __future__ import annotations

import argparse
from pathlib import Path
import sys


CHECKPOINTS = (
    "Kanga kernel entry reached",
    "Interrupts: 256-entry IDT installed",
    "Memory map entries:",
    "Physical bitmap allocator:",
    "Virtual memory manager: x86-64 four-level page tables ready",
    "Virtual mapping, translation and unmapping test: passed",
    "Frame allocator integrity test: passed",
    "Frame memory test: passed",
    "Display: framebuffer text console ready",
    "Starting command shell",
    "Kanga> ",
)

FATAL_MARKERS = (
    "KERNEL PANIC",
    "TRIPLE FAULT",
    "BOOT FAILURE",
)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Check a saved Kanga serial log for expected boot checkpoints."
    )
    parser.add_argument(
        "logfile",
        nargs="?",
        type=Path,
        default=Path("build/serial.log"),
        help="serial log to inspect (default: build/serial.log)",
    )
    args = parser.parse_args()

    try:
        log = args.logfile.read_text(encoding="utf-8", errors="replace")
    except OSError as error:
        print(f"Cannot read {args.logfile}: {error}", file=sys.stderr)
        return 2

    if not log.strip():
        print(f"FAIL  {args.logfile} is empty")
        return 1

    print(f"Kanga serial log: {args.logfile}")
    print()

    missing = []
    for checkpoint in CHECKPOINTS:
        if checkpoint in log:
            print(f"PASS  {checkpoint.strip()}")
        else:
            print(f"MISS  {checkpoint.strip()}")
            missing.append(checkpoint)

    fatal = [marker for marker in FATAL_MARKERS if marker in log]
    for marker in fatal:
        print(f"FAIL  Fatal marker found: {marker}")

    print()
    if missing or fatal:
        print(
            f"Result: INCOMPLETE — {len(missing)} checkpoint(s) missing, "
            f"{len(fatal)} fatal marker(s) found."
        )
        return 1

    print(f"Result: BOOT CHECKPOINTS PRESENT — all {len(CHECKPOINTS)} checks found.")
    print("This is a log check, not proof that every device or feature works.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
