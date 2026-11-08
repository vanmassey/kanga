"""Host-side tests for the Kanga serial log inspector."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
INSPECTOR = ROOT / "scripts" / "inspect-serial.py"

GOOD_LOG = """Kanga kernel entry reached
Interrupts: 256-entry IDT installed
Memory map entries: 4
Physical bitmap allocator: ready
Virtual memory manager: x86-64 four-level page tables ready
Virtual mapping, translation and unmapping test: passed
Frame allocator integrity test: passed
Frame memory test: passed
Kernel heap initialisation: passed
Kernel initialisation complete
Kernel idle loop reached
"""


class SerialInspectorTests(unittest.TestCase):
    def run_inspector(self, contents=None, *, create_file=True):
        with tempfile.TemporaryDirectory() as directory:
            log_path = Path(directory) / "serial.log"
            if create_file:
                log_path.write_text(contents or "", encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(INSPECTOR), str(log_path)],
                capture_output=True,
                text=True,
                check=False,
            )

    def test_all_checkpoints_pass(self):
        result = self.run_inspector(GOOD_LOG)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("all 10 checks found", result.stdout)
        self.assertNotIn("MISS", result.stdout)

    def test_missing_checkpoint_is_reported(self):
        result = self.run_inspector(GOOD_LOG.replace("Frame memory test: passed\n", ""))
        self.assertEqual(result.returncode, 1)
        self.assertIn("MISS  Frame memory test: passed", result.stdout)
        self.assertIn("1 checkpoint(s) missing", result.stdout)

    def test_fatal_marker_fails_even_when_checkpoints_exist(self):
        result = self.run_inspector(GOOD_LOG + "KERNEL PANIC: test\n")
        self.assertEqual(result.returncode, 1)
        self.assertIn("Fatal marker found: KERNEL PANIC", result.stdout)

    def test_empty_log_is_rejected(self):
        result = self.run_inspector("")
        self.assertEqual(result.returncode, 1)
        self.assertIn("is empty", result.stdout)

    def test_missing_file_returns_usage_error(self):
        result = self.run_inspector(create_file=False)
        self.assertEqual(result.returncode, 2)
        self.assertIn("Cannot read", result.stderr)


if __name__ == "__main__":
    unittest.main()
