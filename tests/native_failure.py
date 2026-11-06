#!/usr/bin/env python3
"""Owned native test: taskset -c 0,1 python3 -B tests/native_failure.py BINARY [osc|combining].

Private Sway, Fontconfig and raw children only, no user configuration or sessions.
PTY I/O fault injection is covered by deterministic adapter tests, not this test.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from native_fonts import configure_fonts
from native_runtime import Keyboard, nodes, wait_until

PROBE = r'''
import fcntl,os,select,signal,struct,termios,time,tty
from pathlib import Path
root=Path(__file__).parent
index=1
while True:
    try:
        (root/str(index)).mkdir()
        break
    except FileExistsError: index+=1
base=root/str(index)
tty.setraw(0)
(base/'pid').write_text(str(os.getpid()))
def size(*unused):
    value=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\0'*8))[:2]
    temp=base/'size.tmp'
    temp.write_text(repr(value))
    temp.replace(base/'size')
signal.signal(signal.SIGWINCH,size)
size()
os.write(1,b'\x1b]2;owned-'+str(index).encode()+b'\x07\x1b[H\x1b[41m            \x1b[0m')
(base/'ready').touch()
end=time.monotonic()+90
with open(base/'bytes','wb',buffering=0) as output:
    while time.monotonic()<end:
        if (base/'fault').exists():
            kind=(base/'fault').read_text()
            payload=(b'\x1b]2;PRIVATE_BAD_PAYLOAD_'+b'x'*8192 if kind=='osc'
                     else b'a'+ '\u0301'.encode()*65)
            try: os.write(1,payload)
            except OSError: pass
            (base/'fault').unlink()
        if (base/'eof').exists(): break
        if select.select([0],[],[],0.02)[0]:
            data=os.read(0,65536)
            if not data: break
            output.write(data)
            os.write(1,b'\x1b[H\x1b[42m            \x1b[0m')
'''


def main():
    binary = str(Path(sys.argv[1]).resolve())
    fault = sys.argv[2] if len(sys.argv) > 2 else "osc"
    assert fault in ("osc", "combining")
    processes = []
    keyboard = None
    with tempfile.TemporaryDirectory(prefix="ptyra-failure-") as folder:
        root = Path(folder)
        config = root / "sway.conf"
        config.write_text("output * mode 800x600\nseat seat0 fallback true\n")
        keymap = root / "keymap.xkb"
        source = 'xkb_keymap { xkb_keycodes { include "evdev+aliases(qwerty)" }; xkb_types { include "complete" }; xkb_compatibility { include "complete" }; xkb_symbols { include "pc+us+inet(evdev)" }; };'
        subprocess.run(["xkbcomp", "-xkb", "-", str(keymap)], input=source.encode(), capture_output=True, check=True)
        env = dict(os.environ, XDG_RUNTIME_DIR=folder, XDG_CONFIG_HOME=str(root / "config"),
                   XDG_CACHE_HOME=str(root / "cache"), WLR_BACKENDS="headless",
                   WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1")
        for key in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "SWAYSOCK", "WAYLAND_DEBUG", "DISPLAY",
                    "PTYRA_TRACE_INPUT", "PTYRA_TRACE_POINTER", "PTYRA_TRACE_CLIPBOARD"):
            env.pop(key, None)
        (root / "test.toml").write_text("")
        configure_fonts(root, env)
        probe = root / "probe.py"
        probe.write_text("#!" + sys.executable + "\n" + PROBE)
        probe.chmod(0o700)
        env["SHELL"] = str(probe)

        def start(args, name, process_env):
            with open(root / (name + ".log"), "w") as log:
                process = subprocess.Popen(args, env=process_env, stdin=subprocess.DEVNULL,
                                           stdout=log, stderr=log)
            processes.append(process)
            return process

        try:
            start(["sway", "-c", str(config)], "sway", env)
            ipc_path = wait_until(lambda: next(root.glob("sway-ipc*"), None))
            display = wait_until(lambda: next((p for p in root.glob("wayland-*") if not p.name.endswith(".lock")), None))
            env["WAYLAND_DISPLAY"] = display.name
            keyboard = Keyboard(display, keymap.read_bytes())
            app = start([binary, "--config", str(root / "test.toml")], "app",
                        env)

            def log():
                return (root / "app.log").read_text()

            def live():
                assert app.poll() is None, "Ptyra exited globally under bad stream: " + log()[-3000:]
                return True

            def ipc(*args):
                return subprocess.check_output(["swaymsg", "-s", str(ipc_path), *args], text=True)

            def read(index, name):
                path = root / str(index) / name
                return path.read_bytes() if path.exists() else b""

            def title():
                live()
                return next((n.get("name") for n in nodes(json.loads(ipc("-t", "get_tree")))
                             if n.get("pid") == app.pid), None)

            def shortcut(code, mask=5):
                keyboard.modifiers(mask)
                keyboard.tap(code)
                keyboard.modifiers(0)
                keyboard.sync()

            def ready(index):
                wait_until(lambda: live() and read(index, "pid") and (root / str(index) / "ready").exists())
                wait_until(lambda: title() == f"owned-{index}")
                wait_until(lambda: bytes.fromhex("cc0000") * 12 in screenshot())

            def children():
                return Path(f"/proc/{app.pid}/task/{app.pid}/children").read_text().split()

            def gone(index):
                pid = read(index, "pid").decode()
                wait_until(lambda: live() and pid not in children() and not Path(f"/proc/{pid}").exists())

            def fd_count():
                # Old compositor-owned resize buffers can retire asynchronously.
                return sum(not os.readlink(path).startswith("/memfd:ptyra-frame")
                           for path in Path(f"/proc/{app.pid}/fd").iterdir())

            def screenshot():
                return subprocess.check_output(["grim", "-t", "ppm", "-"], env=env)

            def fail(index):
                (root / str(index) / "fault").write_text(fault)
                gone(index)

            def diagnostic_pixels():
                _, dimensions, _, pixels = screenshot().split(b"\n", 3)
                width, height = map(int, dimensions.split())
                yellow = bytes.fromhex("c4a000")
                black = bytes.fromhex("181818")
                for y in range(height):
                    row = pixels[y * width * 3:(y + 1) * width * 3]
                    cells = [row[x:x + 3] for x in range(0, len(row), 3)]
                    yellow_cells = [x for x, cell in enumerate(cells) if cell == yellow]
                    if yellow_cells and black in cells[yellow_cells[0]:yellow_cells[-1]]:
                        return True
                return False

            def stopped():
                wait_until(lambda: title() == "Stopped: stream error")
                # The pre-failure red screen must be discarded, not left under a notice.
                wait_until(lambda: bytes.fromhex("cc0000") * 12 not in screenshot())
                wait_until(diagnostic_pixels)
                assert "PRIVATE_BAD_PAYLOAD" not in log()

            ready(1)
            initial_fds = fd_count()
            fail(1)
            stopped()
            assert children() == []
            failed_fds = fd_count()
            assert failed_fds < initial_fds, (initial_fds, failed_fds)
            # A stale PTY readiness source must not cause an idle busy loop.
            def ticks():
                fields = Path(f"/proc/{app.pid}/stat").read_text().rsplit(")", 1)[1].split()
                return int(fields[11]) + int(fields[12])
            before = ticks()
            time.sleep(0.5)
            assert ticks() - before < os.sysconf("SC_CLK_TCK") // 5, "failed Tab spins while idle"
            live()
            print(f"PASS: active {fault} failure retains last Tab, redacts screen, reaps child/FD and stays idle", flush=True)

            keyboard.text("discarded")
            shortcut(20)
            ready(2)
            keyboard.text("live")
            wait_until(lambda: read(2, "bytes") == b"live")
            wait_until(lambda: bytes.fromhex("4e9a06") * 12 in screenshot())
            old_size = read(2, "size")
            ipc("output * mode 640x480")
            wait_until(lambda: live() and read(2, "size") not in (b"", old_size))
            baseline_live_fds = fd_count()
            shortcut(20)
            ready(3)
            shortcut(104, 4)
            wait_until(lambda: title() == "owned-2")
            fail(3)
            assert title() == "owned-2"
            assert fd_count() == baseline_live_fds, (baseline_live_fds, fd_count())
            keyboard.text("-after-hidden")
            wait_until(lambda: read(2, "bytes") == b"live-after-hidden")
            shortcut(109, 4)
            stopped()
            shortcut(17)
            wait_until(lambda: title() == "owned-2")
            print("PASS: hidden failure preserves other Tab input/output/frame/resize and exact non-frame FD count", flush=True)

            for index in (4, 5, 6):
                shortcut(20)
                ready(index)
                assert read(index, "bytes") == b""
                fail(index)
                stopped()
                keyboard.text("never-reroute")
                shortcut(17)
                wait_until(lambda: title() == "owned-2")
                assert fd_count() == baseline_live_fds, (baseline_live_fds, fd_count())
                assert children() == [read(2, "pid").decode()]
                assert read(2, "bytes") == b"live-after-hidden"
            print("PASS: repeated failures close/reopen without rerouting input, child or FD leaks", flush=True)

            (root / "2" / "eof").touch()
            gone(2)
            stopped()
            assert fd_count() == failed_fds
            shortcut(17)
            assert app.wait(timeout=5) == 0, log()
            print("PASS: regular EOF closes its Tab; explicit final failed-Tab close exits cleanly", flush=True)
        except Exception:
            print("Owned process exit codes:", [process.poll() for process in processes], file=sys.stderr)
            for name in ("sway", "app"):
                path = root / (name + ".log")
                if path.exists():
                    print(path.read_text()[-5000:], file=sys.stderr)
            raise
        finally:
            if keyboard:
                keyboard.close()
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)


if __name__ == "__main__":
    main()
