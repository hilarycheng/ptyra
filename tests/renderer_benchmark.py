"""Deterministic replay-report checks; no GPU or compositor."""
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import renderer_benchmark as driver


class Replay(unittest.TestCase):
    def test_exited_probe_cannot_bypass_trace_limit(self):
        def process(*_, **kwargs):
            kwargs["stdout"].write("x" * (4 * 1024 * 1024 + 1))
            return SimpleNamespace(pid=123, poll=lambda: 0, returncode=0)
        with patch.object(driver.subprocess, "Popen", process):
            result = driver.run(Path("fake-probe"), "shm", 1, 1)
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["error"], "ValueError")

    def test_trace_validation_and_comparison(self):
        lines = [f"FRAME {i} 1 640 360 0 360 {i + 10} 10 20 30" for i in range(8)]
        first = driver.parse_trace("PROBE 1 shm\n" + "\n".join(lines), "shm", 1)
        second = driver.parse_trace("PROBE 1 gles\n" + "\n".join(lines), "gles", 1)
        self.assertTrue(driver.identical([first, second]))
        second[0]["digest"] += 1
        self.assertFalse(driver.identical([first, second]))
        for bad in [lines[:-1], lines + lines, [lines[0].replace("FRAME 0", "FRAME 9")] + lines[1:],
                    [lines[0].replace("640", "99999999")] + lines[1:]]:
            with self.assertRaises(ValueError):
                driver.parse_trace("PROBE 1 shm\n" + "\n".join(bad), "shm", 1)
        self.assertFalse(driver.identical([]))


if __name__ == "__main__":
    unittest.main()
