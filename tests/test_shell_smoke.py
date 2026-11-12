"""Host-side tests for the kash shell smoke-test transcript checker."""

from importlib import util
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "shell-smoke.py"

spec = util.spec_from_file_location("shell_smoke", SCRIPT)
shell_smoke = util.module_from_spec(spec)
spec.loader.exec_module(shell_smoke)


def build_transcript(cases=None, drop_output=None):
    """Synthesise what a healthy serial session looks like."""
    cases = shell_smoke.CASES if cases is None else cases
    lines = ["Kernel idle loop reached", "", "Welcome to Kanga DOS"]
    text = "\r\n".join(lines) + "\r\n" + shell_smoke.PROMPT
    for command, expected in cases:
        text += command + "\r\n"
        for needle in expected:
            if needle != drop_output:
                text += needle.lstrip("\r\n") + "\r\n"
        text += shell_smoke.PROMPT
    return text


class TranscriptCheckerTests(unittest.TestCase):
    def test_healthy_transcript_passes(self):
        self.assertEqual(shell_smoke.check_transcript(build_transcript()), [])

    def test_missing_banner_is_reported(self):
        transcript = build_transcript().replace("Welcome to Kanga DOS", "Welcome")
        failures = shell_smoke.check_transcript(transcript)
        self.assertIn("shell banner not found", failures)

    def test_missing_output_is_reported(self):
        transcript = build_transcript(drop_output="Files: 1 of 16 used")
        failures = shell_smoke.check_transcript(transcript)
        self.assertEqual(len(failures), 1, failures)
        self.assertIn("'fsinfo'", failures[0])
        self.assertIn("Files: 1 of 16 used", failures[0])

    def test_unechoed_command_is_reported(self):
        transcript = build_transcript().replace(
            shell_smoke.PROMPT + "tasks", shell_smoke.PROMPT + "tsks"
        )
        failures = shell_smoke.check_transcript(transcript)
        self.assertTrue(any("'tasks'" in item and "echoed" in item for item in failures))

    def test_output_must_follow_its_own_command(self):
        cases = [("one", ["alpha"]), ("two", ["beta"])]
        # "alpha" appears only after the second prompt, so case one must fail.
        transcript = "Welcome to kash\r\nkash> one\r\nkash> two\r\nalpha\r\nbeta\r\nkash> "
        failures = shell_smoke.check_transcript(transcript, cases)
        self.assertEqual(len(failures), 1, failures)
        self.assertIn("'one'", failures[0])

    def test_repeated_commands_are_matched_in_order(self):
        cases = [("type f", ["first"]), ("type f", ["second"])]
        transcript = (
            "Welcome to kash\r\nkash> type f\r\nfirst\r\nkash> type f\r\nsecond\r\nkash> "
        )
        self.assertEqual(shell_smoke.check_transcript(transcript, cases), [])

    def test_every_case_has_a_command_and_expectations_list(self):
        for command, expected in shell_smoke.CASES:
            self.assertTrue(command.strip())
            self.assertIsInstance(expected, list)


if __name__ == "__main__":
    unittest.main()
