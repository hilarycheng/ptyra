"""Real raw-child Kitty reply rejection and recovery in the existing owned fixture."""
import subprocess
import time


def check_kitty_pressure(root, env, keyboard, app, wait_until):
    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    script = root / 'kitty-pressure.py'
    script.write_text(r'''
import os,select,termios,time,tty
from pathlib import Path
root=Path(__file__).parent
old=termios.tcgetattr(0)
tty.setraw(0)
def gate(name):
 end=time.monotonic()+20
 while not (root/name).exists():
  assert time.monotonic()<end, name
  time.sleep(0.01)
def write(data):
 while data:
  data=data[os.write(1,data):]
def read_until_quiet():
 data=bytearray()
 end=time.monotonic()+10
 while select.select([0],[],[],0.3)[0]:
  assert time.monotonic()<end, 'reply drainage did not stop'
  data.extend(os.read(0,65536))
 return bytes(data)
try:
 (root/'kitty.ready').touch()
 gate('kitty.go')
 # Deliberately stop reading while output fills both the kernel and bounded FIFO.
 for _ in range(32): write(b'\x1b[?u'*4096)
 (root/'kitty.sent').touch()
 gate('kitty.resume')
 replies=read_until_quiet()
 assert 65536<=len(replies)<32*4096*5, len(replies)
 assert replies==b'\x1b[?0u'*(len(replies)//5), 'truncated or corrupt reply'
 (root/'kitty.drained').touch()
 gate('kitty.again')
 write(b'\x1b[?u')
 assert read_until_quiet()==b'\x1b[?0u', 'lost, duplicate or replayed reply'
 (root/'kitty.recovered').touch()
 gate('kitty.exit')
finally:
 termios.tcsetattr(0,termios.TCSANOW,old)
''')
    keyboard.text('python3 ' + str(script))
    key(28)
    wait_until((root / 'kitty.ready').exists)
    key(20, 5)  # Source stays hidden while replies saturate its PTY.
    time.sleep(0.2)
    (root / 'kitty.go').touch()
    log = lambda: (root / 'app.log').read_text()
    wait_until(lambda: 'Terminal reply rejected: queue limit.' in log(), seconds=15)
    wait_until((root / 'kitty.sent').exists, seconds=15)
    assert app.poll() is None, log()
    # Verify the actual metadata strip renders, not only its stderr message.
    def visible():
        ppm = subprocess.check_output(['grim', '-t', 'ppm', '-'], env=env, timeout=5)
        return bytes.fromhex('c4a000') * 12 in ppm[-800 * 80 * 3:]
    wait_until(visible)
    proof = root / 'kitty-sibling-live'
    keyboard.text('printf healthy > ' + str(proof))
    key(28)
    wait_until(proof.exists)
    assert proof.read_bytes() == b'healthy'
    (root / 'kitty.resume').touch()
    wait_until((root / 'kitty.drained').exists, seconds=15)
    (root / 'kitty.again').touch()
    wait_until((root / 'kitty.recovered').exists)
    assert app.poll() is None, log()
    key(17, 5)  # Close only the sibling, exposing the original raw child.
    (root / 'kitty.exit').touch()
    time.sleep(0.2)
    proof = root / 'kitty-source-live'
    keyboard.text('printf recovered > ' + str(proof))
    key(28)
    wait_until(proof.exists)
    assert proof.read_bytes() == b'recovered'
    print('PASS: hidden Kitty flood visibly rejects bounded replies, keeps both shells live and recovers with one reply and no replay')
