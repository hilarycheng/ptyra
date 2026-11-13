"""Opt-in installed desktop launcher probe in an owned headless Sway session."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from gi.repository import Gio, GLib
from native_runtime import nodes, wait_until


def main():
    binary, terminfo, desktop, icon = map(Path, sys.argv[1:])
    assert icon.is_file() and icon.name == 'ptyra.svg'
    subprocess.run(['desktop-file-validate', str(desktop)], check=True, timeout=5)
    with tempfile.TemporaryDirectory(prefix='ptyra-launch-') as folder:
        root = Path(folder)
        (root / 'sway.conf').write_text('output * mode 800x600\nseat seat0 fallback true\n')
        env = dict(os.environ, XDG_RUNTIME_DIR=folder, WLR_BACKENDS='headless',
                   WLR_RENDERER='pixman', WLR_LIBINPUT_NO_DEVICES='1')
        for key in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'SWAYSOCK', 'WAYLAND_DEBUG']:
            env.pop(key, None)
        shell = root / 'shell'
        shell.write_text('''#!/usr/bin/python3
import curses,json,os,time
from pathlib import Path
root=Path(''' + repr(folder) + ''')
assert os.environ['TERM']=='ptyra'
entry=Path(os.environ['TERMINFO'])
assert entry.is_absolute() and entry.stat().st_mode & 0o777 == 0o700
assert (entry/'p/ptyra').read_bytes()==Path(''' + repr(str(terminfo)) + ''').read_bytes()
curses.setupterm()
assert curses.tigetnum('colors')==16 and curses.tigetnum('pairs')==256
assert curses.tigetstr('cup')==b'\\x1b[%i%p1%d;%p2%dH'
assert curses.tigetstr('setaf') and curses.tigetstr('op')==b'\\x1b[39;49m'
assert curses.tigetstr('kmous') is None and curses.tigetstr('Ms') is None
(root/'ready.json').write_text(json.dumps({'terminfo':str(entry), 'pid':os.getppid()}))
end=time.monotonic()+15
while not (root/'stop').exists():
 assert time.monotonic()<end
 time.sleep(.025)
''')
        shell.chmod(0o700)
        with (root / 'sway.log').open('w') as log:
            sway = subprocess.Popen(['sway', '-c', str(root / 'sway.conf')], env=env, stdout=log, stderr=log)
        pid = None
        def exited():
            child, status = os.waitpid(pid, os.WNOHANG)
            return (child, status) if child else None
        try:
            ipc = wait_until(lambda: next(root.glob('sway-ipc*'), None))
            display = wait_until(lambda: next((p for p in root.glob('wayland-*') if not p.name.endswith('.lock')), None))
            app = Gio.DesktopAppInfo.new_from_filename(str(desktop))
            assert app is not None
            assert app.get_icon().get_file().get_path() == str(icon)
            context = Gio.AppLaunchContext()
            for key in ['WAYLAND_SOCKET', 'WAYLAND_DEBUG']:
                context.unsetenv(key)
            for key, value in dict(XDG_RUNTIME_DIR=folder, WAYLAND_DISPLAY=display.name,
                                   XDG_CONFIG_HOME=folder, SHELL=str(shell), PATH='/usr/bin:/bin').items():
                context.setenv(key, value)
            def launched(info, child, data):
                nonlocal pid
                pid = child
            assert app.launch_uris_as_manager([], context, GLib.SpawnFlags.DO_NOT_REAP_CHILD,
                                              None, None, launched, None)
            assert pid is not None
            wait_until((root / 'ready.json').exists)
            ready = json.loads((root / 'ready.json').read_text())
            assert ready['pid'] == pid
            assert Path(f'/proc/{pid}/exe').resolve() == binary
            def mapped():
                tree = json.loads(subprocess.check_output(['swaymsg', '-s', str(ipc), '-t', 'get_tree']))
                return any(node.get('pid') == pid and node.get('app_id') == 'ptyra' for node in nodes(tree))
            wait_until(mapped)
            (root / 'stop').touch()
            status = wait_until(exited)
            pid = None
            assert status[1] == 0
            assert not Path(ready['terminfo']).exists()
            print('PASS: installed desktop, absolute binary, foreign cwd, private TERMINFO, ncurses and cleanup')
        finally:
            if pid is not None:
                (root / 'stop').touch()
                try:
                    wait_until(exited)
                except AssertionError:
                    os.kill(pid, 9)
                    os.waitpid(pid, 0)
            sway.terminate()
            try:
                sway.wait(timeout=5)
            except subprocess.TimeoutExpired:
                sway.kill()
                sway.wait(timeout=5)


if __name__ == '__main__':
    main()
