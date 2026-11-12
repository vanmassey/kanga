#!/usr/bin/env python3
"""Boot Kanga under QEMU and drive the kash serial shell end to end.

Usage: python3 scripts/shell-smoke.py [--iso build/kanga.iso] [--verbose]

Exit status: 0 all checks passed, 1 a check failed, 2 the test could not run.
The transcript checker is a pure function so it can be unit tested without
QEMU (see tests/test_shell_smoke.py).
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROMPT = "kash> "
BOOT_TIMEOUT = 45.0
COMMAND_TIMEOUT = 10.0

# (command typed at the prompt, substrings its output must contain)
CASES = [
    ("help", ["kash commands", "HEXDUMP"]),
    ("ver", ["Kanga kernel"]),
    ("echo kanga rules", ["\r\nkanga rules"]),
    ("write hello.txt hello kanga", ["bytes written to hello.txt"]),
    ("type hello.txt", ["\r\nhello kanga"]),
    ("append hello.txt !", ["bytes in hello.txt"]),
    ("type hello.txt", ["\r\nhello kanga!"]),
    ("hexdump hello.txt", ["68 65 6c 6c 6f"]),
    ("copy hello.txt copy.txt", ["copied hello.txt to copy.txt"]),
    ("dir", ["hello.txt", "copy.txt", "2 file(s)"]),
    ("del hello.txt", ["deleted hello.txt"]),
    ("type hello.txt", ["no such file"]),
    ("fsinfo", ["Files: 1 of 16 used"]),
    ("tasks", ["bootstrap", "worker-a", "worker-b"]),
    ("mem", ["Physical frames"]),
    ("uptime", ["timer ticks"]),
    ("history", ["echo kanga rules"]),
    ("nosuchcommand", ["unknown command"]),
]


def check_transcript(transcript: str, cases=CASES) -> list[str]:
    """Return a list of failure descriptions; empty means every case passed."""
    failures: list[str] = []
    if "Welcome to kash" not in transcript:
        failures.append("shell banner not found")
    cursor = 0
    for command, expected in cases:
        echo = transcript.find(PROMPT + command, cursor)
        if echo < 0:
            failures.append(f"{command!r}: command was never echoed after a prompt")
            continue
        start = echo + len(PROMPT) + len(command)
        end = transcript.find(PROMPT, start)
        region = transcript[start:] if end < 0 else transcript[start:end]
        for needle in expected:
            if needle not in region:
                failures.append(f"{command!r}: output is missing {needle!r}")
        cursor = start if end < 0 else end
    return failures


class Session:
    """A QEMU process whose serial port is connected to our stdin/stdout."""

    def __init__(self, iso: Path) -> None:
        qemu = shutil.which("qemu-system-x86_64")
        if qemu is None:
            raise RuntimeError("qemu-system-x86_64 was not found on PATH")
        self.process = subprocess.Popen(
            [
                qemu,
                "-machine", "q35",
                "-cpu", "max",
                "-m", "256M",
                "-cdrom", str(iso),
                "-serial", "stdio",
                "-display", "none",
                "-vga", "std",
                "-no-reboot",
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        self._chunks: list[str] = []
        self._lock = threading.Lock()
        threading.Thread(target=self._pump, daemon=True).start()

    def _pump(self) -> None:
        assert self.process.stdout is not None
        while True:
            data = self.process.stdout.read1(4096)
            if not data:
                return
            with self._lock:
                self._chunks.append(data.decode("utf-8", errors="replace"))

    @property
    def transcript(self) -> str:
        with self._lock:
            return "".join(self._chunks)

    def wait_for_prompts(self, count: int, timeout: float) -> bool:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.transcript.count(PROMPT) >= count:
                return True
            if self.process.poll() is not None:
                return False
            time.sleep(0.05)
        return False

    def send(self, text: str) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write(text.encode("ascii"))
        self.process.stdin.flush()

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait()


def run(iso: Path, verbose: bool) -> int:
    session = Session(iso)
    try:
        if not session.wait_for_prompts(1, BOOT_TIMEOUT):
            print("FAIL  kash prompt did not appear during boot", file=sys.stderr)
            print(session.transcript[-2000:], file=sys.stderr)
            return 1
        prompts = 1
        for command, _ in CASES:
            session.send(command + "\r")
            prompts += 1
            if not session.wait_for_prompts(prompts, COMMAND_TIMEOUT):
                print(f"FAIL  no prompt after {command!r}", file=sys.stderr)
                break
        transcript = session.transcript
    finally:
        session.close()

    if verbose:
        print(transcript)
    failures = check_transcript(transcript)
    for failure in failures:
        print(f"FAIL  {failure}")
    if failures:
        print(f"{len(failures)} shell check(s) failed")
        return 1
    print(f"all {len(CASES)} kash commands behaved as expected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--iso", type=Path, default=ROOT / "build" / "kanga.iso")
    parser.add_argument("--verbose", action="store_true", help="print the transcript")
    args = parser.parse_args()

    if not args.iso.exists():
        print(f"Cannot read {args.iso}; run 'make iso' first", file=sys.stderr)
        return 2
    try:
        return run(args.iso, args.verbose)
    except RuntimeError as error:
        print(f"Cannot run: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
