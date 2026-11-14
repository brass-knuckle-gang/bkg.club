"""Run the badge controller and ZIP regressions against the shipped JavaScript."""

from pathlib import Path
import subprocess
import unittest


class BadgeGeneratorTests(unittest.TestCase):
    def test_badge_controller(self):
        tests = Path(__file__).with_name("badges.test.cjs")
        result = subprocess.run(["node", "--test", str(tests)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
