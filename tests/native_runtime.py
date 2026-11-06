#!/usr/bin/env python3
"""Opt-in native test: python3 tests/native_runtime.py target/debug/ptyra /tmp/ptyra.png.

Requires Sway, grim, xkbcomp and Fontconfig. Uses only an owned headless session.
The small Wayland client injects fixed test keys through virtual-keyboard-v1.
"""
import array
import json
import os
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
        codes.update({" ": 57, "/": 53, ".": 52, "-": 12, "'": 40, "=": 13, "\\": 43, "[": 26, "]": 27})
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
                wrapper.write_text(f"#!/bin/sh\nif mkdir '{root}/first' 2>/dev/null; then :; else head -c 262144 /dev/zero | tr '\\000' x; echo drained > '{root}/drained'; fi\nexec /bin/bash --noprofile --norc\n")
            wrapper.chmod(0o700)
            env["SHELL"] = str(wrapper)
            if os.environ.get("PTYRA_TEST_TRACE"):
                env["PTYRA_TRACE_INPUT"] = "1"
            (root / "test.toml").write_text("[bindings]\ncopy='Ctrl+Shift+a'\n")
            keyboard = Keyboard(display, keymap.read_bytes())
            with open(root / "app.log", "w") as log:
                app = subprocess.Popen([binary, "--config", str(root / "test.toml")], env=env, stdout=log, stderr=log)
            def ipc(*args):
                return subprocess.check_output(["swaymsg", "-s", str(ipc_path), *args], text=True)
            wait_until(lambda: any(n.get("app_id") == "ptyra" for n in nodes(json.loads(ipc("-t", "get_tree")))))
            time.sleep(0.2)
            assert app.poll() is None, (root / "app.log").read_text()
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
            keyboard.modifiers(5)
            keyboard.key(30)
            keyboard.sync()
            wait_until(lambda: "Ptyra action unavailable: Copy" in (root / "app.log").read_text())
            time.sleep(0.7)  # Holding a shortcut must not repeat or leak into the Shell.
            keyboard.modifiers(0)
            keyboard.key(30, False)
            keyboard.sync()
            assert (root / "app.log").read_text().count("Ptyra action unavailable: Copy") == 1
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
                    wait_until(lambda: (root / "drained").exists())
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
            keyboard.text("exit")
            keyboard.tap(28)
            keyboard.sync()
            assert app.wait(timeout=5) == 0, (root / "app.log").read_text()
            # Reconnect against the same compositor/keymap, catching shared FD offset regressions.
            for _ in range(2):
                with open(root / "app.log", "w") as log:
                    app = subprocess.Popen([binary, "--config", str(root / "test.toml")], env=env, stdout=log, stderr=log)
                wait_until(lambda: any(n.get("pid") == app.pid for n in nodes(json.loads(ipc("-t", "get_tree")))))
                time.sleep(0.2)
                assert app.poll() is None, (root / "app.log").read_text()
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
