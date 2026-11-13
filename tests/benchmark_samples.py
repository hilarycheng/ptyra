"""Deterministic procfs/schema fixtures; no compositor or live process required."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import benchmark


class Samples(unittest.TestCase):
    def test_single_core_and_pss(self):
        # comm contains spaces and parentheses; child CPU fields must be excluded.
        stat = "42 (fixture ) process) " + " ".join(
            ["S", "1"] + ["0"] * 9 + ["150", "50", "900", "800"] + ["0"] * 4 + ["1234"]
        )
        self.assertEqual(benchmark.parse_stat(stat), {"ticks": 200, "start_ticks": 1234})
        self.assertEqual(benchmark.cpu_percent(200, 500, 2.0, 100), 150.0)
        for args in [(200, 100, 2, 100), (0, 1, 0, 100), (0, 1, 1, 0)]:
            self.assertEqual(benchmark.cpu_percent(*args), benchmark.NA)
        self.assertEqual(benchmark.parse_memory("Rss: 400 kB\nPss: 123 kB\nSwapPss: 9 kB\n"),
                         {"rss_bytes": 409600, "pss_bytes": 125952})
        self.assertEqual(benchmark.parse_memory("Rss: 4 kB\n"),
                         {"rss_bytes": 4096, "pss_bytes": benchmark.NA})

    def test_gpu_deduplicates_clients_and_preserves_missing(self):
        first = "drm-client-id: 7\ndrm-pdev: 0000:01:00.0\ndrm-memory-vram: 100 KiB\n"
        second = first.replace("100 KiB", "200 KiB")
        third = first.replace("id: 7", "id: 8")
        self.assertEqual(benchmark.parse_gpu([first, second, third]), {"vram": 307200})
        self.assertEqual(benchmark.parse_gpu(["pos: 0\n"]), benchmark.NA)
        self.assertEqual(benchmark.parse_gpu([first.replace("KiB", "invalid")]), benchmark.NA)
        resident = first.replace("drm-memory-", "drm-resident-").replace("100 KiB", "102400")
        self.assertEqual(benchmark.parse_gpu([resident + "drm-memory-vram: 100 KiB\n"]), {"vram": 102400})
        self.assertEqual(benchmark.parse_gpu([first, "drm-client-id: 99\n"]), benchmark.NA)

    def test_schema_missing_and_pid_reuse(self):
        before = {"pid": 42, "start_ticks": 123, "ticks": 100, "time": 1.0,
                  "pss_bytes": 1024, "rss_bytes": 2048, "gpu_memory_bytes": benchmark.NA}
        after = dict(before, ticks=150, time=2.0, pss_bytes=3072)
        report = benchmark.summarize([before, after], 100)
        self.assertEqual(report["cpu_percent_one_core"], 50.0)
        self.assertEqual(report["pss_bytes"], {"mean": 2048.0, "peak": 3072})
        self.assertEqual(report["gpu_memory_bytes"], benchmark.NA)
        self.assertEqual(benchmark.summarize([before, dict(after, start_ticks=124)], 100)["cpu_percent_one_core"], benchmark.NA)
        self.assertEqual(benchmark.summarize([before, dict(after, pss_bytes=benchmark.NA)], 100)["pss_bytes"], benchmark.NA)
        self.assertEqual(benchmark.frame_count("unrelated\nPtyra benchmark: frames=12\n"), 12)
        self.assertEqual(benchmark.frame_count(""), benchmark.NA)
        self.assertEqual(benchmark.frame_count("Ptyra benchmark: frames=1\nPtyra benchmark: frames=2\n"), benchmark.NA)
        encoded = json.dumps(report, allow_nan=False)
        self.assertIn("NOT AVAILABLE", encoded)
        self.assertNotIn("PASS", encoded)

    def test_report_schema_and_failed_case_exit_status(self):
        with tempfile.TemporaryDirectory() as folder:
            binary = Path(folder) / "binary"
            binary.write_bytes(b"fixture")
            for failed in (False, True):
                output = Path(folder) / f"report-{failed}.json"
                def case(_binary, tabs, _seconds, _warmup):
                    if failed and tabs == 5:
                        raise OSError("fixture unavailable")
                    return {"tabs": tabs, "status": "RECORDED", "assessment": "NOT EVALUATED",
                            "ptyra": benchmark.NA, "children": benchmark.NA, "frame_count": benchmark.NA}
                with patch.object(sys, "argv", ["benchmark.py", str(binary), "--output", str(output), "--seconds", "1"]), \
                     patch.object(benchmark, "run_case", side_effect=case), \
                     patch.object(benchmark, "command_text", return_value=benchmark.NA):
                    self.assertEqual(benchmark.main(), int(failed))
                    # Re-running cannot overwrite a recorded measurement.
                    with self.assertRaises(FileExistsError):
                        benchmark.main()
                report = json.loads(output.read_text())
                self.assertEqual(report["schema_version"], 1)
                self.assertEqual(report["fixture"]["tabs"], [1, 5, 10])
                self.assertEqual(report["fixture"]["scrollback_lines"], 1000)
                self.assertEqual(report["assessment"], "NOT EVALUATED")
                self.assertEqual([case["tabs"] for case in report["cases"]], [1, 5, 10])
                self.assertEqual(report["cases"][1]["status"], "ERROR" if failed else "RECORDED")
                self.assertEqual(report["source"]["revision"], benchmark.NA)
                self.assertEqual(report["source"]["dirty"], benchmark.NA)
                self.assertTrue(report["started_at_utc"].endswith("+00:00"))
                self.assertIn("clock_ticks_per_second", report["platform"])


if __name__ == "__main__":
    unittest.main()
