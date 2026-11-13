#!/usr/bin/env python3
"""Real Fcitx5/Cangjie test in an owned headless Sway session and private D-Bus.

Usage: python3 tests/native_ime.py target/release/ptyra /tmp/ptyra-ime.png
Add --tabs after the screenshot path for TASK-175 multi-Tab integration.
Requires Sway, Fcitx5 with cangjie3, dbus-daemon, grim and xkbcomp.
No user configuration or running input method is changed.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
from native_runtime import Keyboard, nodes, wait_until


def main():
    binary = str(Path(sys.argv[1]).resolve())
    screenshot = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else None
    processes = []
    keyboard = None
    with tempfile.TemporaryDirectory(prefix="ptyra-ime-") as folder:
        root = Path(folder)
        config = root / "config"
        (config / "fcitx5").mkdir(parents=True)
        (config / "fcitx5/profile").write_text(
            "[Groups/0]\nName=Test\nDefault Layout=us\nDefaultIM=cangjie3\n"
            "[Groups/0/Items/0]\nName=keyboard-us\nLayout=\n"
            "[Groups/0/Items/1]\nName=cangjie3\nLayout=\n"
            "[GroupOrder]\n0=Test\n"
        )
        (config / "fcitx5/config").write_text(
            "[Hotkey/TriggerKeys]\n0=Control+space\n[Behavior]\nActiveByDefault=False\n"
        )
        sway_config = root / "sway.conf"
        sway_config.write_text("output * mode 800x600\nseat seat0 fallback true\n")
        bus_config = root / "bus.conf"
        # No service directories: polling must never auto-start another Fcitx instance.
        bus_config.write_text('<busconfig><type>session</type><listen>unix:path=' + str(root / "bus") +
                              '</listen><policy context="default"><allow send_destination="*"/>'
                              '<allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        keymap = root / "keymap.xkb"
        source = 'xkb_keymap { xkb_keycodes { include "evdev+aliases(qwerty)" }; xkb_types { include "complete" }; xkb_compatibility { include "complete" }; xkb_symbols { include "pc+us+inet(evdev)" }; };'
        subprocess.run(["xkbcomp", "-xkb", "-", str(keymap)], input=source.encode(), capture_output=True, check=True)
        env = dict(os.environ, XDG_RUNTIME_DIR=folder, XDG_CONFIG_HOME=str(config),
                   XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
                   WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_LIBINPUT_NO_DEVICES="1")
        for key in ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "SWAYSOCK", "WAYLAND_DEBUG", "DISPLAY", "DBUS_SESSION_BUS_ADDRESS"]:
            env.pop(key, None)

        def start(args, name, process_env):
            with open(root / (name + ".log"), "w") as log:
                process = subprocess.Popen(args, env=process_env, stdout=log, stderr=log)
            processes.append(process)
            return process

        try:
            bus = subprocess.Popen(["dbus-daemon", "--config-file=" + str(bus_config), "--nofork", "--print-address=1"],
                                   env=env, stdout=subprocess.PIPE, text=True)
            processes.append(bus)
            env["DBUS_SESSION_BUS_ADDRESS"] = bus.stdout.readline().strip()
            assert env["DBUS_SESSION_BUS_ADDRESS"]
            start(["sway", "-c", str(sway_config)], "sway", env)
            ipc_path = wait_until(lambda: next(root.glob("sway-ipc*"), None))
            display = wait_until(lambda: next((p for p in root.glob("wayland-*") if not p.name.endswith(".lock")), None))
            env["WAYLAND_DISPLAY"] = display.name
            # Socket creation precedes compositor readiness on a busy host.
            subprocess.run(["swaymsg", "-s", str(ipc_path), "-t", "get_version"],
                           capture_output=True, check=True, timeout=20)
            keyboard = Keyboard(display, keymap.read_bytes())
            fcitx_args = ["fcitx5", "-D", "--disable", "all", "--enable",
                          "keyboard,table,punctuation,pinyinhelper,wayland,waylandim,classicui,dbus,dbusfrontend"]
            fcitx = start(fcitx_args, "fcitx", env)
            def remote():
                result = subprocess.run(["fcitx5-remote"], env=env, capture_output=True, text=True, timeout=2)
                return result.stdout.strip()
            wait_until(lambda: all("Loaded addon " + addon in (root / "fcitx.log").read_text()
                                   for addon in ("dbusfrontend", "classicui", "table")))
            shell = root / "shell"
            shell.write_text("#!/bin/sh\nexec /bin/bash --noprofile --norc\n")
            shell.chmod(0o700)
            app_env = dict(env, SHELL=str(shell), HISTFILE="/dev/null", PS1="ptyra$ ", WAYLAND_DEBUG="1")
            app = start([binary, "--config", str(root / "missing.toml")], "app", app_env)
            def ipc(*args):
                return subprocess.check_output(["swaymsg", "-s", str(ipc_path), *args], text=True)
            wait_until(lambda: any(n.get("pid") == app.pid for n in nodes(json.loads(ipc("-t", "get_tree")))))
            def app_log():
                return (root / "app.log").read_text()
            wait_until(lambda: "set_content_type(0, 13)" in app_log() and "set_cursor_rectangle(" in app_log())
            wait_until(lambda: remote() == "1")
            def cursor_rect():
                matches = re.findall(r"set_cursor_rectangle\((\d+), (\d+), (\d+), (\d+)\)", app_log())
                return tuple(map(int, matches[-1]))
            keyboard.text("echo '" + "x" * 80)
            wait_until(lambda: cursor_rect()[0] > 500)
            ipc("output * mode 500x400")
            wait_until(lambda: cursor_rect()[0] < 500 and cursor_rect()[1] > 0)
            keyboard.modifiers(4)
            keyboard.tap(46)
            keyboard.modifiers(0)
            keyboard.sync()
            ipc("output * mode 800x600")
            time.sleep(0.2)
            print("PASS: cursor rectangle follows typing and resize", flush=True)
            proof = root / "proof"
            def toggle():
                keyboard.modifiers(4)
                keyboard.tap(57)
                keyboard.modifiers(0)
                keyboard.sync()
            keyboard.text("echo '")
            toggle()
            wait_until(lambda: remote() == "2")
            before = app_log().count("preedit_string")
            keyboard.text("a")  # Cangjie: a -> 日.
            wait_until(lambda: app_log().count("preedit_string") > before)
            assert not proof.exists()
            time.sleep(0.2)
            if screenshot:
                subprocess.run(["grim", str(screenshot)], env=env, check=True)
            keyboard.tap(57)
            keyboard.sync()
            wait_until(lambda: 'commit_string, (Some("日"))' in app_log())
            toggle()
            wait_until(lambda: remote() == "1")
            keyboard.text("' > " + str(proof))
            keyboard.tap(28)
            keyboard.sync()
            wait_until(proof.exists)
            assert proof.read_text() == "日\n", proof.read_text()
            print("PASS: Ctrl+Space, real Cangjie preedit and exactly one Chinese PTY commit", flush=True)
            # Focus loss clears composition and re-enables the same terminal on return.
            keyboard.text("echo '")
            toggle()
            wait_until(lambda: remote() == "2")
            before = app_log().count("preedit_string")
            keyboard.text("a")
            wait_until(lambda: app_log().count("preedit_string") > before)
            ipc("workspace number 2")
            time.sleep(0.15)
            ipc("workspace number 1")
            time.sleep(0.2)
            if remote() == "2":
                toggle()
                wait_until(lambda: remote() == "1")
            keyboard.text("' > " + str(proof))
            keyboard.tap(28)
            keyboard.sync()
            wait_until(lambda: proof.read_text() == "\n")
            print("PASS: focus loss discards preedit", flush=True)
            # A restarted IME must not break ordinary keys or the next Chinese commit.
            fcitx.terminate()
            fcitx.wait(timeout=5)
            keyboard.text("echo english > " + str(proof))
            keyboard.tap(28)
            keyboard.sync()
            wait_until(lambda: proof.read_text() == "english\n")
            start(fcitx_args, "fcitx-restarted", env)
            wait_until(lambda: all("Loaded addon " + addon in (root / "fcitx-restarted.log").read_text()
                                   for addon in ("dbusfrontend", "classicui", "table")))
            ipc("workspace number 2")
            time.sleep(0.15)
            ipc("workspace number 1")
            wait_until(lambda: remote() in ("1", "2"))
            keyboard.text("echo '")
            if remote() != "2":
                toggle()
            wait_until(lambda: remote() == "2")
            before = app_log().count('commit_string, (Some("日"))')
            keyboard.text("a")
            time.sleep(0.15)
            keyboard.tap(57)
            keyboard.sync()
            wait_until(lambda: app_log().count('commit_string, (Some("日"))') > before)
            toggle()
            wait_until(lambda: remote() == "1")
            keyboard.text("' > " + str(proof))
            keyboard.tap(28)
            keyboard.sync()
            wait_until(lambda: proof.read_text() == "日\n")
            if "--tabs" in sys.argv[3:]:
                def english():
                    if remote() == "2":
                        toggle()
                    wait_until(lambda: remote() == "1")

                def compose():
                    if remote() != "2":
                        toggle()
                    wait_until(lambda: remote() == "2")
                    before = app_log().count("preedit_string")
                    keyboard.text("a")
                    wait_until(lambda: app_log().count("preedit_string") > before)

                def change_tab(code, modifiers):
                    before = app_log().count(".enable()")
                    keyboard.modifiers(modifiers)
                    keyboard.tap(code)
                    keyboard.modifiers(0)
                    keyboard.sync()
                    wait_until(lambda: app_log().count(".enable()") > before)
                    time.sleep(0.15)

                def finish_quote(path, expected):
                    english()
                    keyboard.text("' > " + str(path))
                    keyboard.tap(28)
                    keyboard.sync()
                    wait_until(lambda: path.exists() and path.read_text() == expected)

                def chinese_quote(path):
                    english()
                    keyboard.text("echo '")
                    compose()
                    before = app_log().count('commit_string, (Some("日"))')
                    keyboard.tap(57)
                    keyboard.sync()
                    wait_until(lambda: app_log().count('commit_string, (Some("日"))') > before)
                    finish_quote(path, "日\n")

                # Create a Tab while Chinese composition is active, without toggling English.
                keyboard.text("echo '")
                compose()
                change_tab(20, 5)  # Ctrl+Shift+T
                chinese_quote(root / "second")
                change_tab(104, 4)  # Ctrl+PageUp
                finish_quote(root / "first-cancelled", "\n")
                chinese_quote(root / "first-return")
                change_tab(109, 4)  # Ctrl+PageDown
                keyboard.text("echo '")
                compose()
                change_tab(17, 5)  # Ctrl+Shift+W discards the closing Tab's composition.
                chinese_quote(root / "after-close")
                change_tab(20, 5)
                chinese_quote(root / "replacement")
                change_tab(17, 5)
                assert (root / "second").read_text() == "日\n"
                assert (root / "first-cancelled").read_text() == "\n"
                print("PASS TASK-175: real Fcitx5 new/switch/close/reopen Tabs; exact isolated Chinese bytes; pending preedit cancelled", flush=True)

            keyboard.text("exit")
            keyboard.tap(28)
            keyboard.sync()
            assert app.wait(timeout=5) == 0
            print("PASS: real Fcitx5 Ctrl+Space, Cangjie preedit/commit, exact UTF-8 PTY bytes, focus reset and IME restart with refocus")
        except Exception:
            for name in ["sway", "fcitx", "fcitx-restarted", "app"]:
                path = root / (name + ".log")
                if path.exists():
                    print(path.read_text()[-12000:], file=sys.stderr)
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
