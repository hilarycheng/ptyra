#!/usr/bin/env python3
"""Compare fixed offscreen raster/transfer replays, excluding Wayland presentation."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time

from benchmark import NA, REPO, command_text, sample, sha256, stop, summarize

FIELDS = ("sequence", "scale", "width", "height", "dirty_start", "dirty_end", "digest",
          "raster_ns", "submit_ns", "verify_ns")


def parse_trace(text, backend, rounds):
    lines = text.splitlines()
    if not lines or lines[0] != f"PROBE 1 {backend}":
        raise ValueError("probe header mismatch")
    frames = []
    for line in lines[1:]:
        if line.startswith("DEVICE "):
            continue
        parts = line.split()
        if len(parts) != 11 or parts[0] != "FRAME":
            raise ValueError("invalid frame record")
        frame = dict(zip(FIELDS, map(int, parts[1:])))
        if (frame["sequence"] != len(frames) or frame["scale"] not in (1, 2)
                or min(frame.values()) < 0 or frame["width"] == 0 or frame["height"] == 0
                or frame["width"] * frame["height"] > 4 * 1024 * 1024
                or not 0 <= frame["dirty_start"] <= frame["dirty_end"] <= frame["height"]):
            raise ValueError("frame bounds or order mismatch")
        frames.append(frame)
    if len(frames) != rounds * 8:
        raise ValueError("incomplete replay")
    return frames


def identical(traces):
    def inputs(trace):
        return [tuple(frame[key] for key in FIELDS[:7]) for frame in trace]
    return bool(traces) and all(inputs(trace) == inputs(traces[0]) for trace in traces[1:])


def run(binary, backend, rounds, timeout):
    result = {"backend": backend, "status": "ERROR", "assessment": "NOT EVALUATED"}
    process = None
    with tempfile.TemporaryDirectory(prefix="ptyra-renderer-") as folder:
        path = Path(folder) / "trace"
        samples = []
        try:
            with path.open("w") as output:
                process = subprocess.Popen([str(binary), backend, str(rounds)], stdout=output,
                                           stderr=subprocess.DEVNULL, stdin=subprocess.DEVNULL)
                started = time.monotonic()
                while process.poll() is None:
                    samples.append(sample(process.pid))
                    if time.monotonic() - started > timeout:
                        raise TimeoutError("renderer replay timed out")
                    if path.stat().st_size > 4 * 1024 * 1024:
                        raise ValueError("renderer trace exceeds cap")
                    time.sleep(0.02)
                result["exit_code"] = process.returncode
            if path.stat().st_size > 4 * 1024 * 1024:
                raise ValueError("renderer trace exceeds cap")
            text = path.read_text()
            result["device"] = [line[7:][:512] for line in text.splitlines() if line.startswith("DEVICE ")]
            if process.returncode != 0:
                raise RuntimeError("renderer replay failed")
            frames = parse_trace(text, backend, rounds)
            result.update(status="RECORDED", correctness="PASS", frames=frames,
                          frame_timings_ns={key: {"mean": sum(f[key] for f in frames) / len(frames),
                                                 "peak": max(f[key] for f in frames)}
                                            for key in FIELDS[7:]})
        except (OSError, ValueError, RuntimeError, TimeoutError) as error:
            result["error"] = type(error).__name__  # Never preserve child stderr or payloads.
        finally:
            stop(process)
        result["samples"] = samples
        result["resources"] = summarize(samples, os.sysconf("SC_CLK_TCK")) if samples else NA
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--backends", nargs="+", choices=("shm", "gles"), default=["shm", "shm"])
    parser.add_argument("--rounds", type=int, choices=range(1, 1001), metavar="1..1000", default=10)
    parser.add_argument("--timeout", type=int, choices=range(1, 3601), metavar="1..3600", default=120)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    report = {"schema_version": 1, "started_at_utc": datetime.now(timezone.utc).isoformat(),
              "assessment": "NOT EVALUATED", "scope": "offscreen raster and buffer transfer; no compositor or vsync",
              "platform": {"os": platform.freedesktop_os_release().get("PRETTY_NAME", NA),
                           "kernel": platform.release(), "cpu_affinity": sorted(os.sched_getaffinity(0))},
              "binary_sha256": sha256(binary), "revision": command_text(["git", "rev-parse", "HEAD"]),
              "source_dirty": command_text(["git", "status", "--porcelain"]) != "",
              "fixture": {"version": 1, "rounds": args.rounds, "frames_per_round": 8,
                          "logical_sizes": [[640, 360], [480, 300]], "scales": [1, 2],
                          "font_size": 20, "scrollback_lines": 32,
                          "hashes": {str(path): sha256(REPO / path) for path in [
                              Path("src/runtime/renderer_probe.rs"), Path("tools/renderer_benchmark.py"),
                              *[Path("tests/fixtures") / f"fallback-{name}.ttf" for name in ("primary", "cjk", "mark")]]}},
              "measurement_notes": ["Both backends share CPU glyph rasterization and identical fixtures.",
                  "PSS/RSS/GPU and single-core CPU cover setup, replay, exact readback and full-frame validation.",
                  "Per-frame raster/submit exclude validation; GLES submit synchronizes GPU completion.",
                  "Cold scale/cache transitions remain in measurements. No warmup or performance verdict.",
                  "Synthetic fonts, no Tab bar/IME/selection, no presentation latency or desktop acceptance."]}
    with args.output.open("x") as output:
        report["cases"] = [run(binary, backend, args.rounds, args.timeout) for backend in args.backends]
        recorded = all(case["status"] == "RECORDED" for case in report["cases"])
        report["identical_frames"] = recorded and identical([case["frames"] for case in report["cases"]])
        report["finished_at_utc"] = datetime.now(timezone.utc).isoformat()
        json.dump(report, output, indent=2, allow_nan=False)
        output.write("\n")
    return 0 if report["identical_frames"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
