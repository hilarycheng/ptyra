#!/usr/bin/env python3
"""Opt-in: taskset -c 0,1 python3 tests/native_backpressure.py target/debug/ptyra.

Uses only an owned headless Sway and raw-mode children. Requires Sway, xkbcomp,
grim and the existing test fonts. No user session or configuration is touched.
Add --ime to use a synthetic input-method-v2 peer and real text-input-v3 commits.
This tests protocol/PTY backpressure, not desktop or Fcitx5 acceptance.
"""
import json
import os
from pathlib import Path
import random
import re
import struct
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from native_fonts import configure_fonts
from native_runtime import Keyboard, nodes, string, u32, wait_until


PROBE = r'''
import fcntl,os,select,signal,struct,termios,time,tty
from pathlib import Path
root=Path(__file__).parent
try:
    (root/'first').mkdir()
    name='slow'
except FileExistsError:
    name='live'
tty.setraw(0)
def size(*unused):
    value=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\0'*8))[:2]
    temp=root/(name+'.size.tmp')
    temp.write_text(repr(value))
    temp.replace(root/(name+'.size'))
signal.signal(signal.SIGWINCH,size)
size()
(root/(name+'.ready')).touch()
end=time.monotonic()+90
while name=='slow' and not (root/'resume').exists():
    assert time.monotonic()<end,'parent did not resume child'
    time.sleep(0.01)
with open(root/(name+'.bytes'),'wb',buffering=0) as output:
    while time.monotonic()<end:
        if select.select([0],[],[],0.05)[0]:
            data=os.read(0,65536)
            if not data: break
            output.write(data)
            if name=='live':
                os.write(1,b'\x1b[H\x1b[41m            \x1b[0m')
'''


class InputMethod(Keyboard):
    """Owned compositor-side IME peer; deliberately does not grab the keyboard."""
    def __init__(self, path, keymap):
        self.ime = None
        self.active = False
        self.serial = 0
        super().__init__(path, keymap)
        seat = self.bind("wl_seat")
        manager = self.bind("zwp_input_method_manager_v2")
        self.ime = self.allocate()
        self.send(manager, 0, u32(seat, self.ime))
        self.sync()

    def sync(self):
        callback = self.allocate()
        self.send(1, 0, u32(callback))
        while True:
            while len(self.pending) < 8:
                self.receive()
            obj, header = struct.unpack_from("=II", self.pending)
            size, opcode = header >> 16, header & 65535
            assert 8 <= size <= 65535
            while len(self.pending) < size:
                self.receive()
            payload, self.pending = self.pending[8:size], self.pending[size:]
            if obj == 1 and opcode == 0:
                raise AssertionError("test compositor rejected Wayland request")
            if obj == 2 and opcode == 0:
                name, length = struct.unpack_from("=II", payload)
                self.globals[payload[8:8 + length - 1].decode()] = name
            if obj == self.ime:
                if opcode in (0, 1):  # activate/deactivate
                    self.active = opcode == 0
                elif opcode == 5:  # done: serial is the count of done events.
                    self.serial += 1
                assert opcode != 6, "owned input method is unavailable"
            if obj == callback and opcode == 0:
                return

    def commit(self, text):
        self.sync()
        assert self.active, "Ptyra did not enable text-input-v3"
        self.send(self.ime, 0, string(text))  # commit_string
        self.send(self.ime, 3, u32(self.serial))  # commit


def main():
    binary = str(Path(sys.argv[1]).resolve())
    ime = len(sys.argv) > 2 and sys.argv[2] == "--ime"
    processes = []
    keyboard = None
    with tempfile.TemporaryDirectory(prefix="ptyra-backpressure-") as folder:
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
            keyboard = (InputMethod if ime else Keyboard)(display, keymap.read_bytes())
            app = start([binary, "--config", str(root / "test.toml")], "app",
                        dict(env, PTYRA_TRACE_INPUT="1"))

            def log():
                return (root / "app.log").read_text()

            def live():
                assert app.poll() is None, "Ptyra exited under input backpressure: " + log()[-3000:]
                return True

            def ipc(*args):
                return subprocess.check_output(["swaymsg", "-s", str(ipc_path), *args], text=True)

            def counts():
                live()
                matches = re.findall(r"press=(\d+) release=(\d+) repeat=(\d+) queued=(\d+) written=(\d+)", log())
                return tuple(map(int, matches[-1])) if matches else (0, 0, 0, 0, 0)

            def read(name):
                path = root / name
                return path.read_bytes() if path.exists() else b""

            def shortcut(code, mask=5):
                keyboard.modifiers(mask)
                keyboard.tap(code)
                keyboard.modifiers(0)
                keyboard.sync()

            wait_until(lambda: any(n.get("pid") == app.pid for n in nodes(json.loads(ipc("-t", "get_tree")))))
            wait_until(lambda: live() and (root / "slow.ready").exists())
            if ime:
                wait_until(lambda: (keyboard.sync(), keyboard.active)[1])
                chunks = []
                for index in range(48):
                    # Fixed-size, distinguishable UTF-8 commits must be all-or-nothing.
                    chunk = f"{index:04d}" + "日中" * 512
                    keyboard.commit(chunk)
                    keyboard.tap(42)  # Shift gives a counts barrier without PTY bytes.
                    keyboard.sync()
                    wait_until(lambda: counts()[1] == index + 1)
                    chunks.append(chunk.encode())
                    if "input rejected" in log().lower():
                        break
                payload = b"".join(chunks)
                accepted = len(payload) - len(chunks[-1])
                assert 65536 <= accepted < len(payload), (accepted, len(payload))
                assert counts()[3] == 0, "barrier keys unexpectedly entered the PTY"
            else:
                # Nonperiodic bytes detect duplication, truncation and reordering.
                rng = random.Random(178)
                alphabet = "qwertyuiopasdfghjklzxcvbnm"
                codes = dict(zip("qwertyuiop", range(16, 26)))
                codes.update(zip("asdfghjkl", range(30, 39)))
                codes.update(zip("zxcvbnm", range(44, 51)))
                text = "".join(rng.choice(alphabet) for _ in range(128 * 1024))
                sent = 0
                while sent < len(text):
                    for character in text[sent:sent + 128]:
                        keyboard.tap(codes[character])
                    sent += 128
                    keyboard.sync()
                    wait_until(lambda: counts()[1] == sent)
                    if "input rejected" in log().lower():
                        break
                press, release, repeat, accepted, _ = counts()
                assert (press, release, repeat) == (sent, sent, 0), counts()
                assert 65536 <= accepted < sent, counts()
                payload = text[:sent].encode()
            assert "input rejected" in log().lower(), log()[-2000:]
            # Metadata-only notice must also be visibly rendered, not just logged.
            yellow = bytes.fromhex("c4a000")
            wait_until(lambda: live() and yellow * 12 in subprocess.check_output(["grim", "-t", "ppm", "-"], env=env))
            assert payload[:80] not in log().encode(), "input payload leaked into diagnostics"
            kind = "IME" if ime else "keyboard"
            print(f"PASS: bounded queue visibly rejects {kind} input without exit ({accepted}/{len(payload)} accepted)", flush=True)
            if not ime:
                before = counts()
                keyboard.key(30)
                keyboard.sync()
                wait_until(lambda: counts()[0] == before[0] + 1)
                notice_count = log().count("Input rejected:")
                time.sleep(0.9)  # Longer than Sway's default repeat delay.
                assert counts()[2:4] == before[2:4], counts()
                assert log().count("Input rejected:") == notice_count
                keyboard.key(30, False)
                keyboard.sync()
                wait_until(lambda: counts()[1] == before[1] + 1)
                print("PASS: rejected held key cancels repeat without extra input or notice flood", flush=True)

            shortcut(20)  # Ctrl+Shift+T: a second independent child.
            wait_until(lambda: live() and (root / "live.ready").exists())
            keyboard.text("other-tab")
            wait_until(lambda: live() and read("live.bytes") == b"other-tab")
            wait_until(lambda: live() and bytes.fromhex("cc0000") * 12 in subprocess.check_output(["grim", "-t", "ppm", "-"], env=env))
            old_size = read("live.size")
            ipc("output * mode 640x480")
            wait_until(lambda: live() and read("live.size") not in (b"", old_size))
            keyboard.text("-resized")
            wait_until(lambda: live() and read("live.bytes") == b"other-tab-resized")
            print("PASS: second Tab keyboard, PTY output, rendered frame and resize stay live", flush=True)

            (root / "resume").touch()
            wait_until(lambda: live() and len(read("slow.bytes")) >= accepted)
            assert read("slow.bytes") == payload[:accepted], "accepted bytes lost, duplicated or reordered"
            # Switching cannot redirect queued input; rejected keys must never reappear later.
            shortcut(109, 4)  # Ctrl+PageDown: back to the original child.
            if ime:
                wait_until(lambda: (keyboard.sync(), keyboard.active)[1])
                keyboard.commit("復原")
                keyboard.sync()
                suffix = "復原".encode()
            else:
                keyboard.text("after-drain")
                suffix = b"after-drain"
            expected = payload[:accepted] + suffix
            wait_until(lambda: live() and len(read("slow.bytes")) >= len(expected))
            time.sleep(0.15)
            assert read("slow.bytes") == expected
            assert read("live.bytes") == b"other-tab-resized"
            print("PASS: accepted bytes drain exactly once in order to their original Tab; later input recovers", flush=True)
            ipc('[app_id="ptyra"] kill')
            assert app.wait(timeout=5) == 0, log()
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
