"""Host-side tests for the pure CMOS RTC decoding logic."""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
RTC_SOURCE = ROOT / "kernel" / "src" / "rtc.rs"


class RtcDecodeTests(unittest.TestCase):
    def test_rtc_decode_unit_tests(self):
        rustc = shutil.which("rustc")
        if rustc is None:
            self.skipTest("rustc is not installed; Rust unit tests run in the CI toolchain")

        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "rtc-decode-tests"
            compile_result = subprocess.run(
                [
                    rustc,
                    "--edition=2021",
                    "--test",
                    str(RTC_SOURCE),
                    "-o",
                    str(binary),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(
                compile_result.returncode,
                0,
                compile_result.stdout + compile_result.stderr,
            )

            test_result = subprocess.run(
                [str(binary)],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(
                test_result.returncode,
                0,
                test_result.stdout + test_result.stderr,
            )
            self.assertIn("8 passed", test_result.stdout)


if __name__ == "__main__":
    unittest.main()
