#!/usr/bin/env python3
"""Opt-in native test: python3 tests/native_runtime.py target/debug/ptyra /tmp/ptyra.png.

Requires Sway, grim, xkbcomp and Fontconfig. Uses only an owned headless session.
The small Wayland client injects fixed test keys through virtual-keyboard-v1.
"""
import array
import json
import os
import re
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import time


def wait_until(check, seconds=5):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = check()
        if value:
            return value
        time.sleep(0.025)
    raise AssertionError("native test timed out")


def u32(*values):
    return struct.pack("=" + "I" * len(values), *values)


def string(value):
    value = value.encode() + b"\0"
    return u32(len(value)) + value + bytes((-len(value)) % 4)


class Keyboard:
    def __init__(self, path, keymap):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(5)
        self.sock.connect(str(path))
        self.next_id = 3
        self.pending = b""
        self.globals = {}
        self.send(1, 1, u32(2))  # wl_display.get_registry
        self.sync()
        seat = self.bind("wl_seat")
        manager = self.bind("zwp_virtual_keyboard_manager_v1")
        self.keyboard = self.allocate()
        self.send(manager, 0, u32(seat, self.keyboard))
        fd = os.memfd_create("ptyra-test-keymap", os.MFD_CLOEXEC)
        try:
            os.write(fd, keymap + b"\0")
            self.send(self.keyboard, 0, u32(1, len(keymap) + 1), fd)
        finally:
            os.close(fd)
        self.sync()

    def allocate(self):
        value = self.next_id
        self.next_id += 1
        return value

    def send(self, obj, opcode, payload=b"", fd=None):
        message = u32(obj, ((len(payload) + 8) << 16) | opcode) + payload
        ancillary = [] if fd is None else [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array("i", [fd]))]
        count = self.sock.sendmsg([message], ancillary)
        if count < len(message):
            self.sock.sendall(message[count:])

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
                interface = payload[8:8 + length - 1].decode()
                self.globals[interface] = name
            if obj == callback and opcode == 0:
                return

    def receive(self):
        data, ancillary, _, _ = self.sock.recvmsg(65536, socket.CMSG_SPACE(16))
        assert data, "compositor disconnected"
        for level, kind, value in ancillary:
            if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                fds = array.array("i")
                fds.frombytes(value)
                for fd in fds:
                    os.close(fd)
        self.pending += data

    def bind(self, interface):
        obj = self.allocate()
        self.send(2, 0, u32(self.globals[interface]) + string(interface) + u32(1, obj))
        return obj

    def modifiers(self, mask):
        self.send(self.keyboard, 2, u32(mask, 0, 0, 0))

    def key(self, code, pressed=True):
        self.send(self.keyboard, 1, u32(int(time.monotonic() * 1000) & 0xffffffff, code, int(pressed)))

    def tap(self, code):
        self.key(code)
        self.key(code, False)

    def text(self, text):
        codes = dict(zip("qwertyuiop", range(16, 26)))
        codes.update(zip("asdfghjkl", range(30, 39)))
        codes.update(zip("zxcvbnm", range(44, 51)))
        codes.update(zip("1234567890", range(2, 12)))
        codes.update({" ": 57, "/": 53, ".": 52, "-": 12, "'": 40, "=": 13, ";": 39, "\\": 43, "[": 26, "]": 27})
        shifted = {">": (52, 1), "_": (12, 1), "$": (5, 1)}
        for character in text:
            code, mask = shifted.get(character, (codes.get(character.lower()), int(character.isupper())))
            assert code is not None, character
            self.modifiers(mask)
            self.tap(code)
        self.modifiers(0)
        self.sync()

    def close(self):
        self.sock.close()


def nodes(node):
    yield node
    for child in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from nodes(child)


def main():
    binary = str(Path(sys.argv[1]).resolve())
    screenshot = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else None
    with tempfile.TemporaryDirectory(prefix="ptyra-smoke-") as folder:
        root = Path(folder)
        assets = Path(__file__).resolve().parent.parent / "assets"
        subprocess.run(["tic", "-o", str(root / "terminfo"), str(assets / "ptyra.terminfo")], check=True)
        assert (root / "terminfo/p/ptyra").read_bytes() == (assets / "ptyra.terminfo.bin").read_bytes()
        config = root / "sway.conf"
        config.write_text("output * mode 800x600\nseat seat0 fallback true\n")
        keymap = root / "keymap.xkb"
        source = 'xkb_keymap { xkb_keycodes { include "evdev+aliases(qwerty)" }; xkb_types { include "complete" }; xkb_compatibility { include "complete" }; xkb_symbols { include "pc+us+inet(evdev)" }; };'
        subprocess.run(["xkbcomp", "-xkb", "-", str(keymap)], input=source.encode(), capture_output=True, check=True)
        env = dict(os.environ, XDG_RUNTIME_DIR=folder, WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1")
        for key in ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "SWAYSOCK", "WAYLAND_DEBUG"]:
            env.pop(key, None)
        with open(root / "sway.log", "w") as log:
            sway = subprocess.Popen(["sway", "-c", str(config)], env=env, stdout=log, stderr=log)
        app = keyboard = None
        try:
            ipc_path = wait_until(lambda: next(root.glob("sway-ipc*"), None))
            display = wait_until(lambda: next((p for p in root.glob("wayland-*") if not p.name.endswith(".lock")), None))
            env.update(WAYLAND_DISPLAY=display.name, SHELL="/bin/bash", PS1="ptyra$ ", HISTFILE="/dev/null")
            # User startup files are used only with the explicit configured-fish option.
            wrapper = root / "test-shell"
            fish_mode = os.environ.get("PTYRA_TEST_FISH")
            if fish_mode == "configured":
                wrapper.write_text("#!/bin/sh\nexec /usr/bin/fish --private --interactive\n")
            elif fish_mode:
                wrapper.write_text("#!/bin/sh\nexec /usr/bin/fish --no-config --interactive\n")
            else:
                wrapper.write_text("#!/bin/sh\nexec /bin/bash --noprofile --norc\n")
            if os.environ.get("PTYRA_TEST_MULTI_PTY"):
                os.mkfifo(root / "flood-start")
                wrapper.write_text(f"#!/bin/sh\nif mkdir '{root}/first' 2>/dev/null; then :; else if mkdir '{root}/flood-once' 2>/dev/null; then read ready < '{root}/flood-start'; fi; head -c 262144 /dev/zero | tr '\\000' x; echo drained > '{root}/drained'; fi\nexec /bin/bash --noprofile --norc\n")
            wrapper.chmod(0o700)
            env["SHELL"] = str(wrapper)
            if os.environ.get("PTYRA_TEST_TRACE"):
                env["PTYRA_TRACE_INPUT"] = "1"
            if os.environ.get("PTYRA_TEST_POINTER") or os.environ.get("PTYRA_TEST_CLIPBOARD_RECEIVE"):
                env["PTYRA_TRACE_POINTER"] = "1"
            if os.environ.get("PTYRA_TEST_CLIPBOARD_DEVICE"):
                env["PTYRA_TRACE_CLIPBOARD"] = "1"
            (root / "test.toml").write_text(("clipboard_bytes=131072\n" if os.environ.get("PTYRA_TEST_CLIPBOARD_RECEIVE") else "") + "[bindings]\ncopy='Ctrl+Shift+a'\n")
            keyboard = Keyboard(display, keymap.read_bytes())
            with open(root / "app.log", "w") as log:
                app = subprocess.Popen([binary, "--config", str(root / "test.toml")], env=env, stdout=log, stderr=log)
            def ipc(*args):
                return subprocess.check_output(["swaymsg", "-s", str(ipc_path), *args], text=True)
            wait_until(lambda: any(n.get("app_id") == "ptyra" for n in nodes(json.loads(ipc("-t", "get_tree")))))
            time.sleep(0.2)
            assert app.poll() is None, (root / "app.log").read_text()
            if os.environ.get("PTYRA_TEST_CLIPBOARD_DEVICE"):
                wait_until(lambda: "Ptyra clipboard: device bound" in (root / "app.log").read_text())
                assert (root / "app.log").read_text().count("Ptyra clipboard: device bound") == 1
                print("PASS: native seat binds exactly one clipboard data device")
            if os.environ.get("PTYRA_TEST_CLIPBOARD_OFFERS"):
                for mime, message in [("text/plain;charset=utf-8", "text offer selected"), ("application/octet-stream", "non-text offer selected")]:
                    before = (root / "app.log").read_text().count("Ptyra clipboard: " + message)
                    subprocess.run(["wl-copy", "--type", mime, "fixture"], env=env, check=True, timeout=5)
                    wait_until(lambda: (root / "app.log").read_text().count("Ptyra clipboard: " + message) > before)
                subprocess.run(["wl-copy", "--clear"], env=env, check=True, timeout=5)
                wait_until(lambda: "Ptyra clipboard: offer cleared" in (root / "app.log").read_text())
                before = (root / "app.log").read_text().count("Ptyra clipboard: text offer selected")
                subprocess.run(["wl-copy", "--type", "text/plain;charset=utf-8", "focus-fixture"], env=env, check=True, timeout=5)
                wait_until(lambda: (root / "app.log").read_text().count("Ptyra clipboard: text offer selected") > before)
                before = (root / "app.log").read_text().count("Ptyra clipboard: external offers cleared")
                ipc("workspace number 2")
                wait_until(lambda: (root / "app.log").read_text().count("Ptyra clipboard: external offers cleared") > before)
                before = (root / "app.log").read_text().count("Ptyra clipboard: text offer selected")
                ipc("workspace number 1")
                wait_until(lambda: (root / "app.log").read_text().count("Ptyra clipboard: text offer selected") > before)
                print("PASS: native offer replacement, selection None, focus-loss cleanup and reacquisition")
            if os.environ.get("PTYRA_TEST_TRACE"):
                keyboard.key(30)
                keyboard.sync()
                time.sleep(0.05)
                keyboard.key(30, False)
                keyboard.sync()
                wait_until(lambda: "press=1 release=1 repeat=0 queued=1 written=1" in (root / "app.log").read_text())
                keyboard.tap(14)
                keyboard.sync()
                print("PASS: one tap = 1 press + 1 release + 0 repeats + 1 queued byte + 1 written byte")
            if os.environ.get("PTYRA_TEST_CLIPBOARD_RECEIVE"):
                def paste_fixture(payload, expected, mime="text/plain;charset=utf-8", capture=False, bracketed=True, tail=b"", decision=None):
                    received = root / "paste-received"
                    observed = root / "paste-observed"
                    if observed.exists():
                        observed.unlink()
                    if capture:
                        if received.exists():
                            received.unlink()
                        framed = b"\x1b[200~" + payload + b"\x1b[201~" if bracketed else payload
                        wanted = b"z" if decision in ("cancel", "focus") else framed + tail
                        mode = b"\x1b[?2004h" if bracketed else b"\x1b[?2004l"
                        script = root / "capture.py"
                        script.write_text("import os, select, termios, tty, time\nfrom pathlib import Path\nold=termios.tcgetattr(0)\ntry:\n tty.setraw(0)\n os.write(1, " + repr(mode + b"\x1b]2;Paste fixture\x07") + ")\n time.sleep(0.3)\n data=bytearray()\n end=time.monotonic()+8\n while len(data)<" + str(len(wanted)) + ":\n  assert time.monotonic()<end\n  if select.select([0],[],[],0.1)[0]: data.extend(os.read(0," + str(len(wanted)) + "-len(data))); Path(" + repr(str(observed)) + ").write_bytes(data)\nfinally:\n termios.tcsetattr(0,termios.TCSANOW,old)\n os.write(1,b'\\x1b[?2004l\\x1b]2;Terminal\\x07')\nPath(" + repr(str(received)) + ").write_bytes(data)\n")
                        keyboard.text(f"python3 {script}")
                        keyboard.tap(28)
                        keyboard.sync()
                        wait_until(lambda: any(n.get("app_id") == "ptyra" and n.get("name") == "Paste fixture" for n in nodes(json.loads(ipc("-t", "get_tree")))))
                    drag_pointer = None
                    if decision == "cancel":
                        manager = keyboard.bind("zwlr_virtual_pointer_manager_v1")
                        drag_pointer = keyboard.allocate()
                        keyboard.send(manager, 0, u32(0, drag_pointer))
                        keyboard.send(drag_pointer, 1, u32(0, 150, 80, 800, 600))
                        keyboard.send(drag_pointer, 4)
                        keyboard.sync()
                        time.sleep(0.1)
                        keyboard.send(drag_pointer, 2, u32(0, 272, 1))
                        keyboard.send(drag_pointer, 1, u32(0, 180, 80, 800, 600))
                        keyboard.send(drag_pointer, 4)
                        keyboard.sync()
                        time.sleep(0.1)
                    before = (root / "app.log").read_text().count(expected)
                    queued = (root / "app.log").read_text().count("Ptyra paste: queued")
                    prompts = (root / "app.log").read_text().count("Ptyra paste: confirmation shown")
                    subprocess.run(["wl-copy", "--type", mime], input=payload, env=env, check=True, timeout=5)
                    time.sleep(0.1)
                    keyboard.modifiers(5)
                    keyboard.tap(47)
                    keyboard.modifiers(0)
                    if capture and tail and not decision:
                        keyboard.text(tail.decode())
                    else:
                        keyboard.sync()
                    wait_until(lambda: (root / "app.log").read_text().count(expected) > before)
                    if decision:
                        wait_until(lambda: (root / "app.log").read_text().count("Ptyra paste: confirmation shown") > prompts)
                        time.sleep(0.4)
                        assert not observed.exists(), "paste bytes reached PTY before confirmation"
                        assert (root / "app.log").read_text().count("Ptyra paste: queued") == queued
                        ppm = subprocess.check_output(["grim", "-t", "ppm", "-"], env=env)
                        assert b"\xcc\0\0" * 12 in ppm[-800*60*3:], "native prompt background missing"
                        if decision == "accept" and os.environ.get("PTYRA_TEST_PASTE_PROMPT_SCREENSHOT"):
                            subprocess.run(["grim", os.environ["PTYRA_TEST_PASTE_PROMPT_SCREENSHOT"]], env=env, check=True)
                        if drag_pointer:
                            keyboard.send(drag_pointer, 2, u32(0, 272, 0))
                            keyboard.send(drag_pointer, 4)
                            keyboard.sync()
                        keyboard.text("q")  # Modal text is consumed, not passed to the raw reader.
                        if decision == "focus":
                            ipc("workspace number 2")
                            time.sleep(0.1)
                            ipc("workspace number 1")
                            time.sleep(0.1)
                        else:
                            keyboard.tap(28 if decision == "accept" else 1)
                            keyboard.sync()
                        if drag_pointer:
                            selected = (root / "app.log").read_text().count("Ptyra selection: updated")
                            keyboard.send(drag_pointer, 1, u32(0, 250, 80, 800, 600))
                            keyboard.send(drag_pointer, 4)
                            keyboard.sync()
                            time.sleep(0.15)
                            assert (root / "app.log").read_text().count("Ptyra selection: updated") == selected, "dismissed prompt left a phantom selection drag"
                        if decision in ("cancel", "focus"):
                            keyboard.text("z")
                    if capture:
                        if decision not in ("cancel", "focus"):
                            wait_until(lambda: (root / "app.log").read_text().count("Ptyra paste: queued") > queued)
                        wait_until(received.exists)
                        assert received.read_bytes() == wanted, "native paste bytes differ"
                        time.sleep(0.1)
                    assert app.poll() is None
                text = ("繁體e\u0301 \t\n" * 6000).encode()
                paste_fixture(text, f"Ptyra paste: received {len(text)} bytes", capture=True, tail=b"z")
                paste_fixture(b"x" * 131073, "Ptyra paste: clipboard receive exceeds limit")
                paste_fixture(b"\xff", "Ptyra paste: clipboard receive is not UTF-8")
                paste_fixture(b"nontext", "Ptyra paste: clipboard has no text offer", "application/octet-stream")
                paste_fixture(b"recovered", "Ptyra paste: received 9 bytes", "text/plain", capture=True, bracketed=False)
                for decision in ("accept", "cancel", "focus"):
                    paste_fixture(b"unsafe\ntext", "Ptyra paste: received 11 bytes", capture=True, bracketed=False, decision=decision)
                print("PASS: multiline native prompt, zero bytes before decision, accept/Escape/focus cancellation")
                assert "繁體" not in (root / "app.log").read_text()
                print("PASS: native receive cap/UTF-8 recovery and exact raw PTY paste, framing, backpressure and later-key order")
            if os.environ.get("PTYRA_TEST_PASTE_CANCEL"):
                assert not os.environ.get("PTYRA_TEST_MULTI_PTY"), "run paste cancellation fixture separately"
                for change in ("focus", "tab"):
                    received = root / "cancel-received"
                    gate = root / "cancel-read"
                    for path in (received, gate):
                        if path.exists():
                            path.unlink()
                    script = root / "cancel.py"
                    script.write_text("import os, select, termios, tty, time\nfrom pathlib import Path\nold=termios.tcgetattr(0)\ntry:\n tty.setraw(0)\n os.write(1,b'\\x1b[?2004h\\x1b]2;Cancel fixture\\x07')\n end=time.monotonic()+10\n while not Path(" + repr(str(gate)) + ").exists():\n  assert time.monotonic()<end\n  time.sleep(0.01)\n data=bytearray()\n while not data.endswith(b'z'):\n  assert time.monotonic()<end\n  if select.select([0],[],[],0.1)[0]: data.extend(os.read(0,16384))\nfinally:\n termios.tcsetattr(0,termios.TCSANOW,old)\n os.write(1,b'\\x1b[?2004l\\x1b]2;Terminal\\x07')\nPath(" + repr(str(received)) + ").write_bytes(data)\n")
                    keyboard.text(f"python3 {script}")
                    keyboard.tap(28)
                    keyboard.sync()
                    wait_until(lambda: any(n.get("app_id") == "ptyra" and n.get("name") == "Cancel fixture" for n in nodes(json.loads(ipc("-t", "get_tree")))))
                    before = (root / "app.log").read_text().count("Ptyra paste: queued")
                    subprocess.run(["wl-copy", "--type", "text/plain;charset=utf-8"], input=b'x'*120000, env=env, check=True, timeout=5)
                    time.sleep(0.1)
                    keyboard.modifiers(5)
                    keyboard.tap(47)
                    keyboard.modifiers(0)
                    keyboard.text("z")
                    wait_until(lambda: (root / "app.log").read_text().count("Ptyra paste: queued") > before)
                    time.sleep(0.1)  # The raw reader is gated, forcing real PTY backpressure.
                    if change == "focus":
                        ipc("workspace number 2")
                        time.sleep(0.1)
                        ipc("workspace number 1")
                    else:
                        keyboard.modifiers(5)
                        keyboard.tap(20)  # New Tab cancels the old paste, not the old shell.
                        keyboard.modifiers(0)
                        keyboard.sync()
                        wait_until(lambda: len(Path(f"/proc/{app.pid}/task/{app.pid}/children").read_text().split()) == 2)
                        time.sleep(0.2)
                        probe = root / "cancel-isolated"
                        keyboard.text(f"echo isolated > {probe}")
                        keyboard.tap(28)
                        keyboard.sync()
                        wait_until(probe.exists)
                        assert probe.read_text() == "isolated\n", "new Shell input was contaminated"
                        keyboard.modifiers(5)
                        keyboard.tap(17)  # Close only the new Tab; the original raw reader survives.
                        keyboard.modifiers(0)
                        keyboard.sync()
                        wait_until(lambda: len(Path(f"/proc/{app.pid}/task/{app.pid}/children").read_text().split()) == 1)
                    time.sleep(0.1)
                    gate.touch()
                    wait_until(received.exists)
                    data = received.read_bytes()
                    assert data.startswith(b'\x1b[200~') and data.endswith(b'\x1b[201~z'), "cancelled paste lost framing or FIFO tail"
                    assert 12 < len(data) < 120013, "fixture did not interrupt a partial PTY paste"
                    assert data[6:-7] == b'x'*(len(data)-13), "cancelled bytes or other Tab input leaked"
                    assert app.poll() is None
                    time.sleep(0.2)
                print("PASS: native focus/Tab cancellation drops unsent payload, closes original framing, preserves tail and other Shell")
            if os.environ.get("PTYRA_TEST_COPY_OWNER"):
                subprocess.run(["wl-copy", "--type", "application/octet-stream", "sentinel"], env=env, check=True, timeout=5)
            keyboard.modifiers(5)
            keyboard.key(30)
            keyboard.sync()
            wait_until(lambda: "Ptyra copy: no valid selection" in (root / "app.log").read_text())
            time.sleep(0.7)  # Holding a shortcut must not repeat or leak into the Shell.
            keyboard.modifiers(0)
            keyboard.key(30, False)
            keyboard.sync()
            assert (root / "app.log").read_text().count("Ptyra copy: no valid selection") == 1
            if os.environ.get("PTYRA_TEST_COPY_OWNER"):
                assert subprocess.check_output(["wl-paste", "--type", "application/octet-stream", "--no-newline"], env=env, timeout=5) == b"sentinel"

            if os.environ.get("PTYRA_TEST_NEW_TAB"):
                children = Path(f"/proc/{app.pid}/task/{app.pid}/children")
                first = children.read_text().split()
                assert len(first) == 1
                keyboard.modifiers(5)
                keyboard.key(20)  # Ctrl+Shift+T
                keyboard.sync()
                wait_until(lambda: len(children.read_text().split()) == 2)
                time.sleep(0.7)
                keyboard.key(20, False)
                keyboard.modifiers(0)
                keyboard.sync()
                assert len(children.read_text().split()) == 2
                assert first[0] in children.read_text().split()
                if os.environ.get("PTYRA_TEST_MULTI_PTY"):
                    keyboard.modifiers(4)
                    keyboard.tap(104)
                    keyboard.modifiers(0)
                    keyboard.sync()
                    with open(root / "flood-start", "w") as start:
                        start.write("go\n")
                    wait_until(lambda: (root / "drained").exists())
                    keyboard.modifiers(4)
                    keyboard.tap(109)
                    keyboard.modifiers(0)
                    keyboard.sync()
                    print("PASS: hidden Shell drains 256 KiB without starving active input")
                print("PASS: one held NewTab shortcut creates exactly one additional independent Shell")
            if os.environ.get("PTYRA_TEST_TAB_SWITCH"):
                proof1, proof2 = root / "tab1", root / "tab2"
                keyboard.text("echo $$ > " + str(proof2))
                keyboard.tap(28)
                keyboard.sync()
                wait_until(proof2.exists)
                keyboard.modifiers(4)
                keyboard.tap(104)  # Ctrl+PageUp
                keyboard.modifiers(0)
                keyboard.sync()
                keyboard.text("echo $$ > " + str(proof1))
                keyboard.tap(28)
                keyboard.sync()
                wait_until(proof1.exists)
                assert proof1.read_text().strip() == first[0]
                assert proof2.read_text().strip() != first[0]
                assert proof2.read_text().strip() in children.read_text().split()
                assert len(children.read_text().split()) == 2
                print("PASS: new Tab activation and previous Tab shortcut route input without spawning")
            if os.environ.get("PTYRA_TEST_POINTER"):
                manager = keyboard.bind("zwlr_virtual_pointer_manager_v1")
                pointer = keyboard.allocate()
                keyboard.send(manager, 0, u32(0, pointer))
                keyboard.send(pointer, 1, u32(0, 400, 300, 800, 600))
                keyboard.send(pointer, 4)
                keyboard.sync()
                time.sleep(0.1)
                keyboard.send(pointer, 2, u32(0, 272, 1))
                keyboard.send(pointer, 2, u32(0, 272, 0))
                keyboard.send(pointer, 3, u32(0, 0, 2560))
                keyboard.send(pointer, 4)
                keyboard.sync()
                wait_until(lambda: "Ptyra pointer: axis" in (root / "app.log").read_text())
                assert "Ptyra pointer: press" in (root / "app.log").read_text()
                assert "Ptyra pointer: release" in (root / "app.log").read_text()
                print("PASS: real Wayland pointer enter/button/axis events reach the targeted adapter")
                if os.environ.get("PTYRA_TEST_SELECTION"):
                    time.sleep(0.45)  # Independent drag, outside the multi-click interval.
                    before = (root / "app.log").read_text().count("Ptyra selection: updated")
                    keyboard.send(pointer, 2, u32(0, 272, 1))
                    keyboard.send(pointer, 1, u32(0, 500, 300, 800, 600))
                    keyboard.send(pointer, 2, u32(0, 272, 0))
                    keyboard.send(pointer, 4)
                    keyboard.sync()
                    wait_until(lambda: (root / "app.log").read_text().count("Ptyra selection: updated") >= before + 3)
                    print("PASS: real Wayland press/motion/release update the selected grid range")

            if os.environ.get("PTYRA_TEST_MULTI_CLICK"):
                pattern = r"Ptyra selection geometry: (-?\d+):(\d+)-(-?\d+):(\d+) cell=(\d+):(\d+) origin=(\d+)"
                def selected():
                    return tuple(map(int, re.findall(pattern, (root / "app.log").read_text())[-1]))
                _, _, _, _, cw, ch, origin = selected()
                node = next(n for n in nodes(json.loads(ipc("-t", "get_tree"))) if n.get("app_id") == "ptyra")
                rect, window = node["rect"], node["window_rect"]
                fixture = root / "selection-fixture"
                fixture.write_bytes(("\x1b[2J\x1b[Habc 中e\u0301" + "z" * 200 + "\r\n").encode())
                keyboard.text("cat " + str(fixture))
                keyboard.tap(28)
                keyboard.sync()
                time.sleep(0.2)
                def click_cell(column):
                    x = rect["x"] + window["x"] + column * cw + cw // 2
                    y = rect["y"] + window["y"] + origin + ch // 2
                    before = len(re.findall(pattern, (root / "app.log").read_text()))
                    keyboard.send(pointer, 1, u32(0, x, y, 800, 600))
                    keyboard.send(pointer, 2, u32(0, 272, 1))
                    keyboard.send(pointer, 2, u32(0, 272, 0))
                    keyboard.send(pointer, 4)
                    keyboard.sync()
                    wait_until(lambda: len(re.findall(pattern, (root / "app.log").read_text())) > before)
                click_cell(1)
                click_cell(1)
                assert selected()[:4] == (0, 0, 0, 2), selected()
                click_cell(1)
                assert selected()[0:2] == (0, 0) and selected()[2] > 0, selected()
                click_cell(5)
                click_cell(5)
                assert selected()[:4] == (0, 4, 0, 5), selected()
                print("PASS: native double/triple click selects ASCII, whole CJK and soft-wrapped logical lines")
                if os.environ.get("PTYRA_TEST_COPY_OWNER"):
                    keyboard.modifiers(5)
                    keyboard.tap(30)
                    keyboard.modifiers(0)
                    keyboard.sync()
                    wait_until(lambda: "Ptyra copy: published" in (root / "app.log").read_text())
                    formats = subprocess.check_output(["wl-paste", "--list-types"], env=env, timeout=5).decode().splitlines()
                    assert set(formats) == {"text/plain;charset=utf-8", "text/plain"}, formats
                    if os.environ.get("PTYRA_TEST_COPY_TRANSFER"):
                        for mime in formats:
                            assert subprocess.check_output(["wl-paste", "--no-newline", "--type", mime], env=env, timeout=5) == "中".encode()
                        ipc("workspace number 2")
                        time.sleep(0.15)
                        assert subprocess.check_output(["wl-paste", "--no-newline", "--type", "text/plain"], env=env, timeout=5) == "中".encode()
                        ipc("workspace number 1")
                        time.sleep(0.15)
                        print("PASS: native Copy sends exact UTF-8 for both MIME types and retains its source after focus loss")

                    subprocess.run(["wl-copy", "--type", "application/octet-stream", "replacement"], env=env, check=True, timeout=5)
                    wait_until(lambda: "Ptyra copy: cancelled" in (root / "app.log").read_text())
                    print("PASS: Copy preserves an external owner without selection, publishes exact text MIME, and handles cancellation")

                if os.environ.get("PTYRA_TEST_SELECTION_PIXELS"):
                    def highlight_visible():
                        ppm = subprocess.check_output(["grim", "-t", "ppm", "-"], env=env)
                        _, dimensions, _, pixels = ppm.split(b"\n", 3)
                        width, height = map(int, dimensions.split())
                        x = rect["x"] + window["x"] + 4 * cw
                        y = rect["y"] + window["y"] + origin
                        assert x + 2 * cw <= width and y + ch <= height
                        for column in range(x, x + 2 * cw):
                            if not any(pixels[(row * width + column) * 3:(row * width + column) * 3 + 3] == b"\xe7\xe7\xe7" for row in range(y, y + ch)):
                                return False
                        return True
                    wait_until(highlight_visible)
                    if screenshot:
                        subprocess.run(["grim", str(screenshot.with_name(screenshot.stem + "-selection.png"))], env=env, check=True)
                    print("PASS: compositor pixels highlight both complete CJK cells without overwriting glyphs")
                    if os.environ.get("PTYRA_TEST_SELECTION_LIFECYCLE"):
                        keyboard.text("echo changed > " + str(root / "selection-output"))
                        keyboard.tap(28)
                        keyboard.sync()
                        wait_until(lambda: (root / "selection-output").exists())
                        wait_until(lambda: not highlight_visible())
                        print("PASS: native PTY output invalidates the old selection highlight")


            if os.environ.get("PTYRA_TEST_TAB_CLICK"):
                node = next(n for n in nodes(json.loads(ipc("-t", "get_tree"))) if n.get("app_id") == "ptyra")
                rect, window = node["rect"], node["window_rect"]
                for fraction, name, expected in [(0.75, "click2", proof2.read_text().strip()), (0.25, "click1", first[0])]:
                    x = int(rect["x"] + window["x"] + window["width"] * fraction)
                    y = int(rect["y"] + window["y"] + 6)
                    keyboard.send(pointer, 1, u32(0, x, y, 800, 600))
                    keyboard.send(pointer, 4)
                    keyboard.sync()
                    time.sleep(0.1)
                    keyboard.send(pointer, 2, u32(0, 272, 1))
                    keyboard.send(pointer, 2, u32(0, 272, 0))
                    keyboard.send(pointer, 4)
                    keyboard.sync()
                    time.sleep(0.1)
                    keyboard.text("echo $$ > " + str(root / name))
                    keyboard.tap(28)
                    keyboard.sync()
                    wait_until(lambda: (root / name).exists())
                    assert (root / name).read_text().strip() == expected
                print("PASS: actual tab-strip clicks select the exact first/second Shell PID")
            if os.environ.get("PTYRA_TEST_TAB_BAR"):
                wait_until(lambda: b"\x34\x65\xa4" * 24 in subprocess.check_output(["grim", "-t", "ppm", "-"], env=env))
                print("PASS: native compositor pixels contain the active GUI tab strip")
            if os.environ.get("PTYRA_TEST_TITLES"):
                def title():
                    return next(n["name"] for n in nodes(json.loads(ipc("-t", "get_tree"))) if n.get("app_id") == "ptyra")
                keyboard.text(r"printf '\033]2;first-title\007'")
                keyboard.tap(28)
                keyboard.sync()
                wait_until(lambda: title() == "first-title")
                keyboard.modifiers(4)
                keyboard.tap(109)
                keyboard.modifiers(0)
                keyboard.text(r"printf '\033[22;0t\033]2;second-title\007'")
                keyboard.tap(28)
                keyboard.sync()
                wait_until(lambda: title() == "second-title")
                keyboard.text(r"sleep 1; printf '\033]2;hidden-title\007'")
                keyboard.tap(28)
                keyboard.modifiers(4)
                keyboard.tap(104)
                keyboard.modifiers(0)
                keyboard.sync()
                wait_until(lambda: title() == "first-title")
                time.sleep(1.2)
                assert title() == "first-title"
                keyboard.modifiers(4)
                keyboard.tap(109)
                keyboard.modifiers(0)
                keyboard.sync()
                wait_until(lambda: title() == "hidden-title")
                keyboard.text(r"printf '\033[23;0t'")
                keyboard.tap(28)
                keyboard.sync()
                wait_until(lambda: title() == "Terminal")
                keyboard.modifiers(4)
                keyboard.tap(104)
                keyboard.modifiers(0)
                keyboard.sync()
                print("PASS: active/hidden OSC titles and reset update only the correct tab/window")
            if os.environ.get("PTYRA_TEST_RESIZE_TABS"):
                ipc('[app_id="ptyra"] floating enable')
                ipc('[app_id="ptyra"] resize set width 640 height 400')
                time.sleep(0.2)
                keyboard.text("stty size > " + str(root / "size1"))
                keyboard.tap(28)
                keyboard.modifiers(4)
                keyboard.tap(109)
                keyboard.modifiers(0)
                keyboard.text("stty size > " + str(root / "size2"))
                keyboard.tap(28)
                keyboard.sync()
                wait_until(lambda: (root / "size2").exists())
                assert (root / "size1").read_text() == (root / "size2").read_text()
                assert all(int(value) > 0 for value in (root / "size1").read_text().split())
                keyboard.modifiers(4)
                keyboard.tap(104)
                keyboard.modifiers(0)
                keyboard.sync()
                print("PASS: resize updates both active and hidden real PTY dimensions")
            if os.environ.get("PTYRA_TEST_EXIT_TAB"):
                keyboard.modifiers(4)
                keyboard.tap(109)
                keyboard.modifiers(0)
                keyboard.text("echo final > " + str(root / "final"))
                keyboard.tap(28)
                keyboard.text("exit 7")
                keyboard.tap(28)
                keyboard.sync()
                wait_until(lambda: children.read_text().split() == first)
                assert (root / "final").read_text() == "final\n"
                assert app.poll() is None
                print("PASS: second child exit drains and closes only that Tab; original Shell survives")
            if os.environ.get("PTYRA_TEST_CLOSE_TAB"):
                keyboard.modifiers(4)
                keyboard.tap(109)  # Ctrl+PageDown selects second.
                keyboard.modifiers(5)
                keyboard.tap(17)  # Ctrl+Shift+W closes second.
                keyboard.modifiers(0)
                keyboard.sync()
                wait_until(lambda: children.read_text().split() == first)
                assert app.poll() is None
                print("PASS: closing second Tab reaps only its child and selects original Shell")
            proof = root / "proof"
            keyboard.text("echo wrongx")
            keyboard.tap(14)  # Backspace
            keyboard.text(" > " + str(proof))
            keyboard.tap(28)  # Enter
            keyboard.sync()
            wait_until(proof.exists)
            assert proof.read_text() == "wrong\n"
            # Up recalls the previous command; Left/Right preserve editing position.
            proof.unlink()
            keyboard.tap(103)
            keyboard.tap(105)
            keyboard.tap(106)
            keyboard.tap(28)
            keyboard.sync()
            wait_until(proof.exists)
            assert proof.read_text() == "wrong\n"
            keyboard.text("sleep 30")
            keyboard.tap(28)
            keyboard.sync()
            time.sleep(0.2)
            keyboard.modifiers(4)
            keyboard.tap(46)  # Ctrl+C interrupts the foreground child.
            keyboard.modifiers(0)
            keyboard.text("echo interrupted > " + str(proof))
            keyboard.tap(28)
            keyboard.sync()
            wait_until(lambda: proof.read_text() == "interrupted\n")
            keyboard.text("echo ")
            keyboard.key(30)
            keyboard.sync()
            time.sleep(0.9)
            ipc("workspace number 2")
            time.sleep(0.15)
            keyboard.key(30, False)
            keyboard.sync()
            time.sleep(0.8)
            ipc("workspace number 1")
            time.sleep(0.15)
            keyboard.text(" > " + str(proof))
            keyboard.tap(28)
            keyboard.sync()
            wait_until(lambda: proof.read_text().strip().startswith("a"))
            repeated = proof.read_text().strip()
            assert 2 <= len(repeated) <= 25 and set(repeated) == {"a"}, repeated
            keyboard.text("echo native-keyboard-pass")
            keyboard.tap(28)
            keyboard.sync()
            keyboard.text(r"printf '\033[41m      \033[0m\n'")
            keyboard.tap(28)
            keyboard.sync()
            # Verify PTY output reached actual compositor pixels, even after hiding the window.
            wait_until(lambda: b"\xcc\0\0" * 12 in subprocess.check_output(["grim", "-t", "ppm", "-"], env=env))
            if screenshot:
                subprocess.run(["grim", str(screenshot)], env=env, check=True)
            remaining = Path(f"/proc/{app.pid}/task/{app.pid}/children")
            while len(remaining.read_text().split()) > 1:
                count = len(remaining.read_text().split())
                keyboard.modifiers(5)
                keyboard.tap(17)
                keyboard.modifiers(0)
                keyboard.sync()
                wait_until(lambda: len(remaining.read_text().split()) < count)
            keyboard.text("exit")
            keyboard.tap(28)
            keyboard.sync()
            assert app.wait(timeout=5) == 0, (root / "app.log").read_text()
            # Reconnect against the same compositor/keymap, catching shared FD offset regressions.
            for reconnect_index in range(2):
                with open(root / "app.log", "w") as log:
                    app = subprocess.Popen([binary, "--config", str(root / "test.toml")], env=env, stdout=log, stderr=log)
                wait_until(lambda: any(n.get("pid") == app.pid for n in nodes(json.loads(ipc("-t", "get_tree")))))
                time.sleep(0.2)
                assert app.poll() is None, (root / "app.log").read_text()
                if reconnect_index == 0:
                    keyboard.modifiers(5)
                    keyboard.tap(17)  # Last-tab Close shortcut also exits cleanly.
                    keyboard.modifiers(0)
                else:
                    keyboard.text("exit")
                    keyboard.tap(28)
                keyboard.sync()
                assert app.wait(timeout=5) == 0, (root / "app.log").read_text()
            print("PASS: reconnect twice; real Wayland keyboard, Enter, Backspace, arrows, Ctrl+C, repeat/focus, rendered PTY output and exit")
        except Exception:
            if (root / "app.log").exists():
                print((root / "app.log").read_text(), file=sys.stderr)
            raise
        finally:
            if keyboard:
                keyboard.close()
            if app and app.poll() is None:
                app.terminate()
                app.wait(timeout=5)
            sway.terminate()
            sway.wait(timeout=5)


if __name__ == "__main__":
    main()
