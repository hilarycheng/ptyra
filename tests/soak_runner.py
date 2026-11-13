"""Fake process/clock coverage for deterministic soak lifecycle reporting."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import soak


class Fake:
    def __init__(self, failure=None, cleanup=True):
        self.time = 0
        self.actions = []
        self.failure = failure
        self.cleaned = False
        self.cleanup = cleanup

    def start(self):
        if self.failure == "start":
            raise RuntimeError("must not copy payload into report")

    def action(self, name, deadline):
        self.actions.append(name)
        self.time += 1
        if self.failure == "timeout":
            raise TimeoutError("payload")
        if self.failure == "crash":
            raise RuntimeError("payload")

    def sample(self):
        return {"time": self.time, "fd_count": 5, "children": 5}

    def close(self):
        self.cleaned = True
        return self.cleanup


class Runner(unittest.TestCase):
    def test_repeatable_and_cleanup(self):
        first, second = Fake(), Fake()
        report = soak.run(first, 20, 196, clock=lambda: first.time)
        again = soak.run(second, 20, 196, clock=lambda: second.time)
        self.assertEqual(first.actions, second.actions)
        self.assertEqual(report, again)
        self.assertTrue(first.cleaned)
        self.assertEqual(report["status"], "RECORDED")
        self.assertTrue(all(value["status"] == "EXERCISED" for value in report["scenarios"].values()))

    def test_failure_timeout_stop_and_unrun(self):
        for failure in ("start", "timeout", "crash"):
            fake = Fake(failure)
            report = soak.run(fake, 20, 1, clock=lambda: fake.time)
            self.assertEqual(report["status"], "ERROR")
            self.assertTrue(fake.cleaned)
            self.assertNotIn("payload", str(report))
            self.assertTrue(any(v["status"] == "NOT RUN" for v in report["scenarios"].values()))
        fake = Fake()
        report = soak.run(fake, 20, 1, clock=lambda: fake.time, stopped=lambda: len(fake.actions) == 2)
        self.assertEqual(report["status"], "STOPPED")
        self.assertTrue(fake.cleaned)
        fake = Fake(cleanup=False)
        self.assertEqual(soak.run(fake, 1, 1, clock=lambda: fake.time)["status"], "ERROR")

    def test_sample_retention_is_bounded(self):
        fake = Fake()
        report = soak.run(fake, 5000, 1, clock=lambda: fake.time)
        self.assertEqual(len(report["samples"]), 4096)
        self.assertEqual(report["sample_count"], 5001)


if __name__ == "__main__":
    unittest.main()
