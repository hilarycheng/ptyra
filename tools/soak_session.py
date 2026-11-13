"""Owned Sway/Fcitx5 session and fixed raw-PTY workload. Persist only metadata."""
import json
import os
from pathlib import Path
import re
import select
import signal
import subprocess
import sys
import tempfile
import time
import tty

from benchmark import NA, REPO, cpu_percent, parse_stat, sample, stop

sys.path.insert(0, str(REPO / "tests"))
from native_runtime import Keyboard, nodes
from native_fonts import configure_fonts

PASTE = "A中\nB".encode()


def save(path, value):
    pending = path.with_suffix(".pending")
    pending.write_text(json.dumps(value))
    pending.replace(path)


def child():
    root = Path(os.environ["PTYRA_SOAK_ROOT"])
    pid = os.getpid()
    status = root / f"state-{pid}"
    command = root / f"command-{pid}"
    tty.setraw(0)

    def write(data):
        while data:
            data = data[os.write(1, data):]

    def receive(expected=None):
        data = b""
        deadline = time.monotonic() + 10
        while ((expected is not None and data != expected)
               or (expected is None and not data.endswith(b"R"))):
            if len(data) > 8192 or time.monotonic() >= deadline:
                raise TimeoutError()
            if select.select([0], [], [], 0.05)[0]:
                chunk = os.read(0, 8192)
                if not chunk:
                    raise RuntimeError()
                data += chunk
                if expected is not None and not expected.startswith(data):
                    raise RuntimeError()
        if expected is None and not re.fullmatch(rb"\x1b\[\d+;\d+R", data):
            raise RuntimeError()

    sequence = 0
    try:
        write(f"\x1b]2;soak-{pid}\x07\x1b[?25l\x1b[?2004h\x1b[6n".encode())
        receive()
        save(status, {"sequence": 0, "phase": "ready"})
        while True:
            if select.select([0], [], [], 0.025)[0]:
                raise RuntimeError()  # Unsolicited/cross-Tab/duplicate input is a failure.
            if not command.exists():
                continue
            request = json.loads(command.read_text())
            if request["sequence"] <= sequence:
                continue
            sequence = request["sequence"]
            action = request["action"]
            save(status, {"sequence": sequence, "phase": "waiting"})
            if action == "output":
                write("A中e\u0301\r\n".encode() * 200 + b"\x1b[6n")
                receive()
            elif action in ("input", "clipboard", "ime"):
                receive({"input": b"a", "clipboard": b"\x1b[200~" + PASTE + b"\x1b[201~",
                         "ime": "日".encode()}[action])
            elif action != "size":
                raise ValueError()
            columns, rows = os.get_terminal_size(0)
            save(status, {"sequence": sequence, "phase": "done", "columns": columns, "rows": rows})
    except (OSError, ValueError, RuntimeError, TimeoutError):
        save(status, {"sequence": sequence, "phase": "error"})
        raise SystemExit(1) from None


class Session:
    def __init__(self, binary, tabs):
        self.binary, self.tabs = binary, tabs
        self.temporary = tempfile.TemporaryDirectory(prefix="ptyra-soak-")
        self.root = Path(self.temporary.name)
        self.processes, self.pids, self.identities = [], [], {}
        self.app = self.keyboard = None
        self.ipc_path = None
        self.sequence = self.active = self.resizes = 0
        self.previous_samples = {}
        self.deadline = time.monotonic() + 60

    def wait(self, check):
        while time.monotonic() < self.deadline:
            if self.app is not None and self.app.poll() is not None:
                raise RuntimeError("Ptyra exited")
            value = check()
            if value:
                return value
            time.sleep(0.025)
        raise TimeoutError("scenario deadline")

    def spawn(self, args, **kwargs):
        process = subprocess.Popen(args, env=self.env, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kwargs)
        self.processes.append(process)
        return process

    def ipc(self, *args):
        try:
            return subprocess.check_output(["swaymsg", "-s", str(self.ipc_path), *args],
                                           text=True, stderr=subprocess.DEVNULL,
                                           timeout=max(0.1, min(20, self.deadline - time.monotonic())))
        except subprocess.SubprocessError as error:
            raise RuntimeError("owned compositor unavailable") from error

    def children(self):
        return list(map(int, Path(f"/proc/{self.app.pid}/task/{self.app.pid}/children").read_text().split()))

    def state(self, pid):
        path = self.root / f"state-{pid}"
        return json.loads(path.read_text()) if path.exists() else {}

    def key(self, code, modifiers=0):
        self.keyboard.modifiers(modifiers)
        self.keyboard.tap(code)
        self.keyboard.modifiers(0)
        self.keyboard.sync()

    def active_title(self):
        return next((node.get("name") for node in nodes(json.loads(self.ipc("-t", "get_tree")))
                     if node.get("pid") == self.app.pid), None)

    def add_tab(self, first=False):
        if not first:
            self.key(20, 5)
        added = self.wait(lambda: set(self.children()) - set(self.pids))
        if len(added) != 1:
            raise RuntimeError("unexpected child set")
        pid = added.pop()
        self.wait(lambda: self.state(pid).get("phase") == "ready")
        self.pids.append(pid)
        self.identities[pid] = parse_stat(Path(f"/proc/{pid}/stat").read_text())["start_ticks"]
        self.active = len(self.pids) - 1
        self.wait(lambda: self.active_title() == f"soak-{pid}")

    def remote(self):
        try:
            return subprocess.check_output(["fcitx5-remote"], env=self.env, text=True,
                                           stderr=subprocess.DEVNULL, timeout=2).strip()
        except subprocess.CalledProcessError:
            return ""  # Fcitx may not have registered its private bus service yet.
        except subprocess.SubprocessError as error:
            raise RuntimeError("owned IME unavailable") from error

    def english(self):
        mode = self.wait(lambda: value if (value := self.remote()) in ("1", "2") else None)
        if mode == "2":
            self.key(57, 4)
            self.wait(lambda: self.remote() == "1")

    def start(self):
        root = self.root
        self.env = dict(os.environ, XDG_RUNTIME_DIR=str(root), XDG_CONFIG_HOME=str(root / "config"),
                        XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
                        WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_HEADLESS_OUTPUTS="1",
                        WLR_LIBINPUT_NO_DEVICES="1", PTYRA_SOAK_ROOT=str(root), PYTHONDONTWRITEBYTECODE="1")
        for name in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "SWAYSOCK", "DISPLAY", "WAYLAND_DEBUG",
                     "DBUS_SESSION_BUS_ADDRESS", "PTYRA_TRACE_INPUT", "PTYRA_TRACE_POINTER", "PTYRA_TRACE_CLIPBOARD"):
            self.env.pop(name, None)
        config = root / "config/fcitx5"
        config.mkdir(parents=True)
        (config / "profile").write_text("[Groups/0]\nName=Test\nDefault Layout=us\nDefaultIM=cangjie3\n"
            "[Groups/0/Items/0]\nName=keyboard-us\nLayout=\n[Groups/0/Items/1]\nName=cangjie3\nLayout=\n[GroupOrder]\n0=Test\n")
        (config / "config").write_text("[Hotkey/TriggerKeys]\n0=Control+space\n[Behavior]\nActiveByDefault=False\n")
        (root / "bus.conf").write_text('<busconfig><type>session</type><listen>unix:path=' + str(root / "bus") +
            '</listen><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/>'
            '<allow own="*"/></policy></busconfig>')
        self.spawn(["dbus-daemon", "--config-file=" + str(root / "bus.conf"), "--nofork"])
        self.wait((root / "bus").exists)
        self.env["DBUS_SESSION_BUS_ADDRESS"] = "unix:path=" + str(root / "bus")
        (root / "sway.conf").write_text("output * mode 800x600 scale 1\nseat seat0 fallback true\ndefault_border none\n")
        self.spawn(["sway", "-c", str(root / "sway.conf")])
        self.ipc_path = self.wait(lambda: next(root.glob("sway-ipc*"), None))
        display = self.wait(lambda: next((p for p in root.glob("wayland-*") if not p.name.endswith(".lock")), None))
        self.env["WAYLAND_DISPLAY"] = display.name
        self.ipc("-t", "get_version")
        keymap = root / "keymap.xkb"
        source = 'xkb_keymap { xkb_keycodes { include "evdev+aliases(qwerty)" }; xkb_types { include "complete" }; xkb_compatibility { include "complete" }; xkb_symbols { include "pc+us+inet(evdev)" }; };'
        try:
            subprocess.run(["xkbcomp", "-xkb", "-", str(keymap)], input=source.encode(),
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=5)
        except subprocess.SubprocessError as error:
            raise RuntimeError("keymap setup failed") from error
        self.keyboard = Keyboard(display, keymap.read_bytes())
        self.spawn(["fcitx5", "-D", "--disable", "all", "--enable",
                    "keyboard,table,punctuation,pinyinhelper,wayland,waylandim,classicui,dbus,dbusfrontend"])
        (root / "test.toml").write_text("scrollback_lines=128\n")
        configure_fonts(root, self.env)
        shell = root / "workload"
        shell.write_text("#!" + sys.executable + "\nimport runpy,sys\nsys.path.insert(0," + repr(str(REPO / "tools")) +
                         ")\nrunpy.run_path(" + repr(str(Path(__file__).resolve())) + ")[\"child\"]()\n")
        shell.chmod(0o700)
        self.env["SHELL"] = str(shell)
        self.app = self.spawn([str(self.binary), "--config", str(root / "test.toml")])
        self.wait(self.active_title)
        self.ipc('[app_id="ptyra"] focus')
        for index in range(self.tabs):
            self.add_tab(first=index == 0)
        self.english()

    def request(self, pid, action):
        self.sequence += 1
        save(self.root / f"command-{pid}", {"sequence": self.sequence, "action": action})
        return self.sequence

    def completed(self, pid, sequence):
        state = self.state(pid)
        if state.get("phase") == "error":
            raise RuntimeError("workload validation failed")
        return state if state.get("sequence") == sequence and state.get("phase") == "done" else None

    def action(self, name, deadline):
        self.deadline = deadline
        self.english()
        pid = self.pids[self.active]
        if name == "output":
            pending = [(p, self.request(p, "output")) for p in self.pids]
            for p, sequence in pending:
                self.wait(lambda: self.completed(p, sequence))
        elif name == "resize":
            self.resizes += 1
            self.ipc("output * mode " + ("960x640" if self.resizes % 2 else "800x600"))
            time.sleep(0.1)
            sizes = []
            for p in self.pids:
                sequence = self.request(p, "size")
                state = self.wait(lambda: self.completed(p, sequence))
                sizes.append((state["columns"], state["rows"]))
            if len(set(sizes)) != 1 or min(sizes[0]) <= 0:
                raise RuntimeError("PTY geometry mismatch")
        elif name == "switch":
            self.key(109, 4)
            self.active = (self.active + 1) % len(self.pids)
            self.wait(lambda: self.active_title() == f"soak-{self.pids[self.active]}")
        elif name == "reopen":
            self.key(17, 5)
            self.wait(lambda: pid not in self.children() and not Path(f"/proc/{pid}").exists())
            self.pids.remove(pid)
            self.identities.pop(pid)
            for prefix in ("state", "command"):
                (self.root / f"{prefix}-{pid}").unlink(missing_ok=True)
            self.add_tab()
        else:
            sequence = self.request(pid, name)
            self.wait(lambda: self.state(pid).get("sequence") == sequence)
            if name == "input":
                self.key(30)
            elif name == "clipboard":
                owner = subprocess.Popen(["wl-copy", "--foreground", "--type", "text/plain;charset=utf-8"],
                                         env=self.env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                try:
                    owner.stdin.write(PASTE)
                    owner.stdin.close()
                    def offered():
                        try:
                            return "text/plain" in subprocess.check_output(["wl-paste", "--list-types"],
                                env=self.env, text=True, stderr=subprocess.DEVNULL, timeout=2)
                        except subprocess.SubprocessError:
                            return False
                    self.wait(offered)
                    self.key(47, 5)
                    self.wait(lambda: self.completed(pid, sequence))
                finally:
                    stop(owner)
            elif name == "ime":
                self.key(57, 4)
                self.wait(lambda: self.remote() == "2")
                self.key(30)
                time.sleep(0.15)
                self.key(57)
            self.wait(lambda: self.completed(pid, sequence))
            self.english()
        time.sleep(0.05)
        if set(self.children()) != set(self.pids) or any(self.state(p).get("phase") == "error" for p in self.pids):
            raise RuntimeError("child isolation failure")

    def sample(self):
        def measured(pid):
            current = sample(pid)
            try:
                current["fd_count"] = len(list(Path(f"/proc/{pid}/fd").iterdir()))
            except OSError:
                current["fd_count"] = NA
            previous = self.previous_samples.get(pid)
            current["cpu_percent_one_core"] = (cpu_percent(previous["ticks"], current["ticks"],
                current["time"] - previous["time"], os.sysconf("SC_CLK_TCK")) if previous
                and previous["start_ticks"] == current["start_ticks"] and previous["ticks"] != NA
                and current["ticks"] != NA else NA)
            return current
        ptyra = measured(self.app.pid)
        children = [measured(pid) for pid in self.pids]
        self.previous_samples = {entry["pid"]: entry for entry in [ptyra, *children]}
        return {"time": time.monotonic(), "ptyra": ptyra, "children": children}

    def close(self):
        clean = self.app is None or self.app.poll() is None
        self.deadline = time.monotonic() + 10
        if self.app is not None and self.app.poll() is None:
            try:
                self.ipc('[app_id="ptyra"] kill')
                clean = self.app.wait(timeout=5) == 0
            except (OSError, RuntimeError, subprocess.SubprocessError):
                clean = False
        for process in reversed(self.processes):
            try:
                stop(process)
            except (OSError, subprocess.SubprocessError):
                clean = False
        if self.keyboard:
            self.keyboard.close()
        for pid, identity in self.identities.items():
            try:
                if parse_stat(Path(f"/proc/{pid}/stat").read_text())["start_ticks"] == identity:
                    clean = False
                    os.kill(pid, signal.SIGTERM)  # Only this runner's verified workload child.
                    end = time.monotonic() + 2
                    while Path(f"/proc/{pid}").exists() and time.monotonic() < end:
                        time.sleep(0.025)
                    if Path(f"/proc/{pid}").exists() and parse_stat(Path(f"/proc/{pid}/stat").read_text())["start_ticks"] == identity:
                        os.kill(pid, signal.SIGKILL)
            except FileNotFoundError:
                pass
        self.temporary.cleanup()
        return clean
