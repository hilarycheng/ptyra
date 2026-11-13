#!/usr/bin/env python3
"""Owned headless Sway benchmark. Results are measurements, never acceptance PASS.

python3 tools/benchmark.py target/release/ptyra --output /tmp/ptyra-benchmark.json
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import select
import signal
import subprocess
import sys
import tempfile
import time
import tty

NA = "NOT AVAILABLE"
REPO = Path(__file__).resolve().parents[1]
sys.dont_write_bytecode = True


def parse_stat(text):
    fields = text.rsplit(")", 1)[1].split()
    return {"ticks": int(fields[11]) + int(fields[12]), "start_ticks": int(fields[19])}


def cpu_percent(before, after, elapsed, ticks_per_second):
    if after < before or elapsed <= 0 or ticks_per_second <= 0:
        return NA
    return 100.0 * (after - before) / ticks_per_second / elapsed


def parse_memory(text):
    fields = dict(re.findall(r"^(Rss|Pss):\s+(\d+) kB$", text, re.MULTILINE))
    return {name.lower() + "_bytes": int(fields[name]) * 1024 if name in fields else NA
            for name in ("Rss", "Pss")}


def parse_gpu(texts):
    clients = {}
    for text in texts:
        fields = dict(line.split(":", 1) for line in text.splitlines() if ":" in line)
        if "drm-client-id" not in fields:
            continue
        # Kernel fdinfo identifies a client by device plus client-id, not by FD number.
        key = (fields.get("drm-pdev", "").strip(), fields["drm-client-id"].strip())
        client = clients.setdefault(key, {})
        for name, value in fields.items():
            if name.startswith(("drm-resident-", "drm-memory-")):
                match = re.fullmatch(r"\s*(\d+)(?:\s+(KiB|MiB))?\s*", value)
                if not match:
                    return NA
                size = int(match[1]) * {None: 1, "KiB": 1024, "MiB": 1024**2}[match[2]]
                region = name.split("-", 2)[2]
                client[region] = max(client.get(region, 0), size)
    if not clients:
        return NA
    regions = {region for client in clients.values() for region in client}
    if not regions or any(set(client) != regions for client in clients.values()):
        return NA
    return {region: sum(client[region] for client in clients.values()) for region in sorted(regions)}


def sample(pid):
    base = Path(f"/proc/{pid}")
    result = dict(pid=pid, time=time.monotonic(), ticks=NA, start_ticks=NA,
                  rss_bytes=NA, pss_bytes=NA, gpu_memory_bytes=NA)
    try:
        result.update(parse_stat((base / "stat").read_text()))
    except (OSError, ValueError, IndexError):
        return result
    try:
        result.update(parse_memory((base / "smaps_rollup").read_text()))
    except OSError:
        pass
    try:
        result["gpu_memory_bytes"] = parse_gpu([path.read_text() for path in (base / "fdinfo").iterdir()])
    except OSError:
        pass
    try:
        if parse_stat((base / "stat").read_text())["start_ticks"] != result["start_ticks"]:
            result.update(ticks=NA, start_ticks=NA, rss_bytes=NA, pss_bytes=NA, gpu_memory_bytes=NA)
    except (OSError, ValueError, IndexError):
        result.update(ticks=NA, start_ticks=NA, rss_bytes=NA, pss_bytes=NA, gpu_memory_bytes=NA)
    return result


def stats(values):
    if not values or any(value == NA for value in values):
        return NA
    return {"mean": sum(values) / len(values), "peak": max(values)}


def summarize(samples, ticks_per_second):
    first, last = samples[0], samples[-1]
    stable = all(s["pid"] == first["pid"] and s["start_ticks"] == first["start_ticks"]
                 and s["start_ticks"] != NA and s["ticks"] != NA for s in samples)
    cpu = cpu_percent(first["ticks"], last["ticks"], last["time"] - first["time"], ticks_per_second) if stable else NA
    gpu = [s["gpu_memory_bytes"] for s in samples]
    regions = set(gpu[0]) if isinstance(gpu[0], dict) else set()
    gpu_summary = ({region: stats([value[region] for value in gpu]) for region in sorted(regions)}
                   if stable and regions and all(isinstance(value, dict) and set(value) == regions for value in gpu) else NA)
    return {"pid": first["pid"], "cpu_percent_one_core": cpu,
            "pss_bytes": stats([s["pss_bytes"] for s in samples]) if stable else NA,
            "rss_bytes": stats([s["rss_bytes"] for s in samples]) if stable else NA,
            "gpu_memory_bytes": gpu_summary}


def frame_count(text):
    values = re.findall(r"^Ptyra benchmark: (?:version=\S+ )?frames=(\d+)$", text, re.MULTILINE)
    return int(values[0]) if len(values) == 1 else NA


def sha256(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def command_text(args):
    try:
        return subprocess.check_output(args, cwd=REPO, text=True, stderr=subprocess.DEVNULL, timeout=5).strip()
    except (OSError, subprocess.SubprocessError):
        return NA


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def workload_child():
    # Fixed startup output, hidden cursor, then an idle child without startup files or polling.
    tty.setraw(0)
    payload = b"\x1b[?25l" + "A中e\u0301\r\n".encode() * 200 + b"\x1b[6n"
    while payload:
        payload = payload[os.write(1, payload):]
    reply = b""
    deadline = time.monotonic() + 5
    while not reply.endswith(b"R"):
        remaining = deadline - time.monotonic()
        if remaining <= 0 or len(reply) > 64 or not select.select([0], [], [], remaining)[0]:
            raise RuntimeError("workload output was not acknowledged")
        chunk = os.read(0, 64)
        if not chunk:
            raise RuntimeError("workload PTY closed")
        reply += chunk
    if not re.fullmatch(rb"\x1b\[\d+;\d+R", reply):
        raise RuntimeError("unexpected workload acknowledgement")
    Path(os.environ["PTYRA_BENCHMARK_READY"], str(os.getpid())).touch()
    while True:
        signal.pause()


def run_case(binary, tabs, seconds, warmup):
    sys.path.insert(0, str(REPO / "tests"))
    from native_runtime import Keyboard, nodes, wait_until
    from native_fonts import configure_fonts
    app = sway = keyboard = None
    result = {"tabs": tabs, "status": "ERROR", "assessment": "NOT EVALUATED",
              "ptyra": NA, "children": NA, "frame_count": NA,
              "binary_version": NA,
              "frame_count_scope": "submitted buffers over process lifetime, including startup and warmup"}
    with tempfile.TemporaryDirectory(prefix="ptyra-benchmark-") as folder:
        root = Path(folder)
        ready = root / "ready"
        ready.mkdir()
        env = dict(os.environ, XDG_RUNTIME_DIR=folder, XDG_CONFIG_HOME=str(root / "config"),
                   XDG_CACHE_HOME=str(root / "cache"), WLR_BACKENDS="headless", WLR_RENDERER="pixman",
                   WLR_LIBINPUT_NO_DEVICES="1", WLR_HEADLESS_OUTPUTS="1",
                   PTYRA_BENCHMARK_FRAMES="1", PTYRA_BENCHMARK_READY=str(ready), PYTHONDONTWRITEBYTECODE="1")
        for name in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "SWAYSOCK", "WAYLAND_DEBUG", "DISPLAY",
                     "PTYRA_TRACE_INPUT", "PTYRA_TRACE_POINTER", "PTYRA_TRACE_CLIPBOARD"):
            env.pop(name, None)
        (root / "sway.conf").write_text("output * mode 1920x1080 scale 1\nseat seat0 fallback true\ndefault_border none\n")
        (root / "test.toml").write_text("scrollback_lines=1000\n[bindings]\nnew_tab='Ctrl+Shift+4'\n")
        # These checked-in synthetic fonts make font choice independent of the host's font cache.
        configure_fonts(root, env)
        result["config_toml"] = (root / "test.toml").read_text()
        shell = root / "workload"
        shell.write_text("#!" + sys.executable + "\nimport runpy\nrunpy.run_path(" + repr(str(Path(__file__).resolve())) + ")[\"workload_child\"]()\n")
        shell.chmod(0o700)
        env["SHELL"] = str(shell)
        try:
            with open(root / "sway.log", "w") as log:
                sway = subprocess.Popen(["sway", "-c", str(root / "sway.conf")], env=env,
                                        stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            ipc_path = wait_until(lambda: next(root.glob("sway-ipc*"), None))
            display = wait_until(lambda: next((p for p in root.glob("wayland-*") if not p.name.endswith(".lock")), None))
            env["WAYLAND_DISPLAY"] = display.name
            keyboard = Keyboard(display, (REPO / "tests/fixtures/keyboard.xkb").read_bytes())
            with open(root / "app.log", "w") as log:
                app = subprocess.Popen([str(binary), "--config", str(root / "test.toml")], env=env,
                                       stdin=subprocess.DEVNULL, stdout=log, stderr=log)

            def ipc(*args):
                return subprocess.check_output(["swaymsg", "-s", str(ipc_path), *args], text=True, timeout=5)

            def window():
                if app.poll() is not None:
                    raise RuntimeError("Ptyra exited before measurement")
                return next((n for n in nodes(json.loads(ipc("-t", "get_tree"))) if n.get("app_id") == "ptyra"), None)

            wait_until(window)
            ipc('[app_id="ptyra"] focus')
            for expected in range(1, tabs + 1):
                if expected > 1:
                    keyboard.modifiers(5)
                    keyboard.tap(5)  # Ctrl+Shift+4, present in the checked-in keymap fixture.
                    keyboard.modifiers(0)
                    keyboard.sync()
                wait_until(lambda: len(list(ready.iterdir())) == expected)
            children = sorted(int(path.name) for path in ready.iterdir())
            child_path = Path(f"/proc/{app.pid}/task/{app.pid}/children")
            if sorted(map(int, child_path.read_text().split())) != children:
                raise RuntimeError("unexpected workload child set")
            geometry = window()["rect"]
            outputs = json.loads(ipc("-t", "get_outputs"))
            if (geometry["width"], geometry["height"]) != (1920, 1080) or len(outputs) != 1 or outputs[0]["scale"] != 1:
                raise RuntimeError("unexpected benchmark window or scale")
            result["window"] = {"width": geometry["width"], "height": geometry["height"], "scale": 1}
            time.sleep(warmup)
            snapshots = []
            start = time.monotonic()
            while True:
                if app.poll() is not None or sorted(map(int, child_path.read_text().split())) != children:
                    raise RuntimeError("process set changed during measurement")
                snapshots.append({"ptyra": sample(app.pid), "children": [sample(pid) for pid in children]})
                remaining = seconds - (time.monotonic() - start)
                if remaining <= 0:
                    break
                time.sleep(min(1.0, remaining))
            ticks = os.sysconf("SC_CLK_TCK")
            result.update(status="RECORDED", elapsed_seconds=time.monotonic() - start,
                          samples=snapshots, ptyra=summarize([s["ptyra"] for s in snapshots], ticks),
                          children=[summarize([s["children"][i] for s in snapshots], ticks) for i in range(tabs)])
            ipc('[app_id="ptyra"] kill')
            if app.wait(timeout=5) != 0:
                raise RuntimeError("Ptyra failed during close")
            log = (root / "app.log").read_text()
            result["frame_count"] = frame_count(log)
            versions = re.findall(r"^Ptyra benchmark: version=(\S+) frames=\d+$", log, re.MULTILINE)
            result["binary_version"] = versions[0] if len(versions) == 1 else NA
            if any(Path(f"/proc/{pid}").exists() for pid in children):
                raise RuntimeError("workload child remained after close")
        except (OSError, ValueError, AssertionError, RuntimeError, subprocess.SubprocessError) as error:
            result.update(status="ERROR", error=type(error).__name__ + ": " + str(error))
        finally:
            result["ready_tabs"] = len(list(ready.iterdir()))
            stop(app)
            if keyboard:
                keyboard.close()
            stop(sway)
    return result


def duration(value):
    number = float(value)
    if not math.isfinite(number) or not 0.1 <= number <= 3600:
        raise argparse.ArgumentTypeError("duration must be between 0.1 and 3600 seconds")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--seconds", type=duration, default=60.0)
    parser.add_argument("--warmup", type=duration, default=5.0)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    source_status = command_text(["git", "status", "--porcelain"])
    report = {"schema_version": 1, "started_at_utc": datetime.now(timezone.utc).isoformat(),
              "platform": {"kernel": platform.release(), "architecture": platform.machine(),
                           "os": platform.freedesktop_os_release().get("PRETTY_NAME", NA),
                           "sway": command_text(["sway", "--version"]), "backend": "headless/pixman",
                           "cpu_affinity": sorted(os.sched_getaffinity(0)), "clock_ticks_per_second": os.sysconf("SC_CLK_TCK")},
              "binary": {"path": str(binary), "sha256": sha256(binary)},
              "source": {"revision": command_text(["git", "rev-parse", "HEAD"]),
                         "dirty": source_status != "" if source_status != NA else NA,
                         "harness_sha256": sha256(__file__)},
              "fixture": {"window": [1920, 1080], "scale": 1, "font_family": "fallbackprimary",
                          "font_size": 20, "fonts": {name: sha256(REPO / "tests/fixtures" / name) for name in
                                                      ("fallback-primary.ttf", "fallback-cjk.ttf", "fallback-mark.ttf", "demo.ttf")},
                          "workload": "idle-v1: 200 fixed A/CJK/combining lines per Tab, hidden cursor, then signal.pause",
                          "scrollback_lines": 1000, "tabs": [1, 5, 10],
                          "warmup_seconds": args.warmup, "sample_seconds": args.seconds},
              "assessment": "NOT EVALUATED", "cases": []}
    report["measurement_notes"] = [
        "CPU uses delta(utime+stime)/CLK_TCK/monotonic_seconds*100; never divides by CPU count.",
        "PSS/RSS are sampled from smaps_rollup; summaries contain arithmetic mean and peak.",
        "GPU memory is DRM fdinfo resident bytes per region; duplicated client FDs count once.",
        "Shared GPU allocations can appear in different clients; compositor memory is not attributed to Ptyra.",
        "Synthetic fonts and headless/pixman are a harness baseline, not desktop or AMD/Mesa acceptance.",
    ]
    # Exclusive report creation avoids overwriting an earlier measurement or following a symlink.
    with args.output.open("x") as output:
        for tabs in (1, 5, 10):
            print(f"Measuring {tabs} Tabs...", file=sys.stderr, flush=True)
            try:
                case = run_case(binary, tabs, args.seconds, args.warmup)
            except (OSError, ValueError, AssertionError, subprocess.SubprocessError) as error:
                case = {"tabs": tabs, "status": "ERROR", "assessment": "NOT EVALUATED",
                        "ptyra": NA, "children": NA, "frame_count": NA, "error": type(error).__name__ + ": " + str(error)}
            report["cases"].append(case)
        report["finished_at_utc"] = datetime.now(timezone.utc).isoformat()
        json.dump(report, output, indent=2, allow_nan=False)
        output.write("\n")
    return 1 if any(case["status"] == "ERROR" for case in report["cases"]) else 0


if __name__ == "__main__":
    raise SystemExit(main())
