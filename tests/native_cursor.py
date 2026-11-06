"""Silent PTY cursor modes, real compositor pixels, and inactive-view idle checks."""
import json
import subprocess
import time


def check_cursor(root, env, keyboard, app, wait_until, ipc, nodes):
    script = root / 'cursor-probe.py'
    script.write_text('''import os,re,select,termios,time,tty
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
old=termios.tcgetattr(0);tty.setraw(0)
try:
 phase=0
 while not (root/'cursor-stop').exists():
  request=root/f'cursor-{phase}.go';end=time.monotonic()+30
  while not request.exists():
   if (root/'cursor-stop').exists():break
   assert time.monotonic()<end
   time.sleep(0.005)
  if not request.exists():break
  os.write(1,request.read_bytes()+b'\\x1b[6n')
  data=b'';end=time.monotonic()+5
  while not data.endswith(b'R'):
   assert time.monotonic()<end
   if select.select([0],[],[],0.05)[0]:data+=os.read(0,4096)
  assert re.fullmatch(rb'\\x1b\\[\\d+;\\d+R',data),data
  pending=root/'cursor.pending';pending.write_text(str(time.monotonic()));pending.replace(root/f'cursor-{phase}.ready')
  phase+=1
finally:
 os.write(1,b'\\x1b[0m\\x1b[2 q\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'cursor-done').touch()
''')
    keyboard.text('python3 ' + str(script)); keyboard.tap(28); keyboard.sync()
    phase = 0
    def command(data):
        nonlocal phase
        pending = root / 'cursor-request.pending'
        pending.write_bytes(data); pending.replace(root / f'cursor-{phase}.go')
        wait_until((root / f'cursor-{phase}.ready').exists)
        phase += 1
    output = json.loads(ipc('-t', 'get_outputs'))[0]['name']
    def pixels():
        node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
        rect, window = node['rect'], node['window_rect']
        x, y = rect['x'] + window['x'], rect['y'] + window['y'] + 28
        ppm = subprocess.check_output(['grim', '-o', output, '-t', 'ppm', '-'], env=env)
        _, dimensions, _, data = ppm.split(b'\n', 3)
        width, height = map(int, dimensions.split())
        assert (width, height) == (800, 600)
        return b''.join(data[((y + row) * width + x) * 3:((y + row) * width + x + 12) * 3] for row in range(20))
    bg = bytes.fromhex('181818')
    empty = bg * 240
    block = bytes.fromhex('e7e7e7') * 240
    ink = bytes.fromhex('eeeeec')
    underline = bg * (12 * 19) + ink * 12
    beam = (ink + bg * 11) * 20
    def observe(expected):
        wait_until(lambda: pixels() == expected, seconds=2)
        assert app.poll() is None, (root / 'app.log').read_text()
    def stable(expected, seconds=1.1):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            assert pixels() == expected, 'steady cursor pixels changed'
            time.sleep(0.05)
    def idle():
        def ticks():
            return sum(map(int, open('/proc/' + str(app.pid) + '/stat').read().split()[13:15]))
        # Exclude the one full-buffer focus/Tab transition paint before sampling idle.
        time.sleep(1.1)
        before = ticks(); time.sleep(1.1)
        delta = ticks() - before
        assert delta <= 3, ('inactive/steady view used CPU during idle sampling', phase, delta)
    command(b'\x1bc\x1b[2J\x1b[H')
    observe(block); stable(block); idle()
    for code, expected in [(1, block), (3, underline), (5, beam)]:
        command(f'\x1b[{code} q'.encode())
        observe(expected); observe(empty); observe(expected)
        command(f'\x1b[{code + 1} q'.encode())
        observe(expected); stable(expected)
    command(b'\x1b[?12h')
    observe(empty); observe(beam)
    command(b'\x1b[?12l')
    observe(beam); stable(beam); idle()
    command(b'\x1b[?12h\x1b[?25l')
    observe(empty); stable(empty); idle()
    command(b'\x1b[?25h\x1b[1 q')
    observe(block); observe(empty)
    ipc('workspace number 2')
    idle()
    ipc('workspace number 1')
    observe(block); observe(empty); observe(block)
    # Background terminal may still request blinking; the new steady active tab stays idle.
    keyboard.modifiers(5); keyboard.tap(20); keyboard.modifiers(0); keyboard.sync()
    time.sleep(0.2)
    idle()
    before = pixels()
    command(b'\x1b[3 q')
    stable(before); idle()
    keyboard.modifiers(5); keyboard.tap(17); keyboard.modifiers(0); keyboard.sync()
    observe(underline); observe(empty); observe(underline)
    command(b'\x1bc')
    observe(block); stable(block); idle()
    command(b'\x1b[1 q\x1b[0 q')
    observe(block); stable(block)
    (root / 'cursor-stop').touch(); wait_until((root / 'cursor-done').exists)
    assert app.poll() is None, (root / 'app.log').read_text()
    print('PASS: native silent block/beam/underline blink and steady modes, DEC12/25, RIS/default reset, focus restoration, inactive/background-tab idle and source-tab shape restoration')
