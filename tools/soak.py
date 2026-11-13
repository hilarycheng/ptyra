#!/usr/bin/env python3
"""Seeded, persistent multi-Tab soak in an owned headless Sway/Fcitx5 session."""
import argparse
from collections import deque
from datetime import datetime, timezone
import json
import math
from pathlib import Path
import platform
import random
import signal
import subprocess
import time

from benchmark import NA, command_text, sha256

SCENARIOS = ("output", "resize", "switch", "reopen", "input", "clipboard", "ime")


def run(session, seconds, seed, clock=time.monotonic, stopped=lambda: False):
    report = {"status": "ERROR", "assessment": "NOT EVALUATED", "seed": seed,
              "requested_seconds": seconds, "operations": 0, "sample_count": 0,
              "scenarios": {name: {"status": "NOT RUN", "count": 0} for name in SCENARIOS}}
    samples = deque(maxlen=4096)
    randomizer = random.Random(seed)
    started = clock()
    scenario = "setup"
    try:
        session.start()
        started = clock()  # Setup is excluded from requested soak duration.
        deadline = started + seconds
        next_sample = started
        report["status"] = "RECORDED"
        while clock() < deadline and not stopped():
            order = list(SCENARIOS)
            randomizer.shuffle(order)
            for scenario in order:
                if clock() >= deadline or stopped():
                    break
                if clock() >= next_sample:
                    samples.append(session.sample())
                    report["sample_count"] += 1
                    next_sample = clock() + 1
                session.action(scenario, min(deadline + 5, clock() + 10))
                report["operations"] += 1
                entry = report["scenarios"][scenario]
                entry.update(status="EXERCISED", count=entry["count"] + 1)
        if stopped():
            report["status"] = "STOPPED"
        samples.append(session.sample())
        report["sample_count"] += 1
    except (OSError, ValueError, AssertionError, RuntimeError, TimeoutError, subprocess.SubprocessError) as error:
        report.update(status="ERROR", failure={"scenario": scenario, "kind": type(error).__name__,
                                              "operation": report["operations"]})
        if scenario in report["scenarios"]:
            report["scenarios"][scenario]["status"] = "FAILED"
    finally:
        report["elapsed_seconds"] = clock() - started
        try:
            report["cleanup"] = bool(session.close())
        except (OSError, RuntimeError, TimeoutError, subprocess.SubprocessError):
            report["cleanup"] = False
        if not report["cleanup"]:
            report["status"] = "ERROR"
        report["samples"] = list(samples)
        report["sample_retention"] = "last 4096 samples; sample_count includes earlier samples"
    return report


def duration(value):
    number = float(value)
    if not math.isfinite(number) or not 1 <= number <= 604800:
        raise argparse.ArgumentTypeError("duration must be 1..604800 seconds")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--seconds", type=duration, default=3600)
    parser.add_argument("--seed", type=int, default=198)
    parser.add_argument("--tabs", type=int, choices=range(2, 11), default=5)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    interrupted = False

    def stop_requested(*_):
        nonlocal interrupted
        interrupted = True

    signal.signal(signal.SIGINT, stop_requested)
    signal.signal(signal.SIGTERM, stop_requested)
    from soak_session import Session
    report = {"schema_version": 1, "started_at_utc": datetime.now(timezone.utc).isoformat(),
              "binary_sha256": sha256(binary), "harness_sha256": sha256(__file__),
              "session_sha256": sha256(Path(__file__).with_name("soak_session.py")),
              "revision": command_text(["git", "rev-parse", "HEAD"]),
              "source_dirty": command_text(["git", "status", "--porcelain"]) != "",
              "platform": {"os": platform.freedesktop_os_release().get("PRETTY_NAME", NA),
                           "kernel": platform.release(), "sway": command_text(["sway", "--version"]),
                           "fcitx5": command_text(["fcitx5", "--version"]), "backend": "headless/pixman"},
              "tabs": args.tabs, "fixture": {"font_size": 20, "scrollback_lines": 128,
                  "logical_sizes": [[800, 600], [960, 640]], "scale": 1},
              "not_run": ["desktop Firefox/Kitty interoperability", "GPU/HiDPI soak", "IME restart", "pending paste cancellation"],
              "measurement_notes": ["Only fixed test traffic; no PTY/clipboard/key payload logging.",
                  "Each sample separates Ptyra, workload children and FD counts; CPU uses one core.",
                  "No automatic leak slope or resource budget verdict; inspect sampled trends."]}
    with args.output.open("x") as output:
        report["run"] = run(Session(binary, args.tabs), args.seconds, args.seed, stopped=lambda: interrupted)
        report["finished_at_utc"] = datetime.now(timezone.utc).isoformat()
        json.dump(report, output, indent=2, allow_nan=False)
        output.write("\n")
    return 0 if report["run"]["status"] == "RECORDED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
