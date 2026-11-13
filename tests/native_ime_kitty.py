"""Exact raw PTY bytes with real Fcitx5, in legacy and negotiated Kitty bit 1."""
import time


def check_ime_kitty(root, keyboard, wait_until, remote, toggle, app_log, ipc, restart_ime):
    script = root / 'ime-keyboard-probe.py'
    script.write_text(r'''
import os,select,sys,termios,time,tty
from pathlib import Path
root=Path(__file__).parent
flags=int(sys.argv[1])
old=termios.tcgetattr(0)
tty.setraw(0)
try:
 os.write(1,f'\x1b[={flags}u\x1b[?u'.encode())
 received=bytearray()
 end=time.monotonic()+10
 while not received.endswith(b'u'):
  assert time.monotonic()<end
  if select.select([0],[],[],0.05)[0]: received.extend(os.read(0,1024))
 assert received==f'\x1b[?{flags}u'.encode()
 for phase in range(3):
  path=root/f'ime-keys-{flags}-{phase}'
  path.with_suffix('.ready').touch()
  received=bytearray()
  end=time.monotonic()+30
  while not path.with_suffix('.stop').exists():
   assert time.monotonic()<end and len(received)<65536
   if select.select([0],[],[],0.02)[0]: received.extend(os.read(0,4096))
  while select.select([0],[],[],0.1)[0]:
   assert time.monotonic()<end and len(received)<65536
   received.extend(os.read(0,4096))
  pending=path.with_suffix('.pending')
  pending.write_text(received.hex())
  pending.replace(path.with_suffix('.result'))
finally:
 os.write(1,b'\x1b[=0u')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/f'ime-keys-{flags}.done').touch()
''')

    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    def english():
        if remote() == '2':
            toggle()
        wait_until(lambda: remote() == '1')

    def compose():
        if remote() != '2':
            toggle()
        wait_until(lambda: remote() == '2')
        before = app_log().count('preedit_string')
        keyboard.text('a')
        wait_until(lambda: app_log().count('preedit_string') > before)

    def commit():
        before = app_log().count('commit_string, (Some("日"))')
        key(57)
        wait_until(lambda: app_log().count('commit_string, (Some("日"))') > before)
        english()

    for flags in (0, 1):
        english()
        keyboard.text(f'python3 {script} {flags}')
        key(28)
        for phase in range(3):
            path = root / f'ime-keys-{flags}-{phase}'
            wait_until(path.with_suffix('.ready').exists)
            compose()
            if phase == 1:
                ipc('workspace number 2')
                time.sleep(0.15)
                ipc('workspace number 1')
                time.sleep(0.2)
                english()
                compose()
                before = app_log().count('.enable()')
                key(20, 5)  # New Tab while the source raw PTY owns Kitty mode.
                wait_until(lambda: app_log().count('.enable()') > before)
                before = app_log().count('.enable()')
                key(104, 4)
                wait_until(lambda: app_log().count('.enable()') > before)
                english()
                expected = b''
            else:
                if phase == 2:
                    restart_ime()
                    english()
                    compose()
                commit()
                expected = '日'.encode()
            key(30, 4)
            key(28, 1)
            key(46, 5)  # Copy has no selection and must never reach the raw PTY.
            expected += b'\x01\r' if flags == 0 else b'\x1b[97;5u\x1b[13;2u'
            path.with_suffix('.stop').touch()
            wait_until(path.with_suffix('.result').exists)
            assert bytes.fromhex(path.with_suffix('.result').read_text()) == expected, (flags, phase)
        wait_until((root / f'ime-keys-{flags}.done').exists)
        time.sleep(0.1)
        key(109, 4)
        key(17, 5)  # Close the temporary sibling after returning to the Shell.
        time.sleep(0.15)
        print(f'PASS TASK-183: flags={flags}, exact UTF-8 commit, shortcut/release ownership, focus/Tab cancellation and IME restart', flush=True)
