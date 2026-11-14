"""Frozen drag pixels and saved Unicode Copy while the real PTY keeps refreshing."""
import json
import re
import subprocess
import time


def check_selection_snapshot(root, env, keyboard, wait_until, ipc, nodes, u32):
    script = root / 'selection-snapshot.py'
    script.write_text('''import os,re,select,termios,time,tty
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
def reply(pattern):
 data=bytearray();end=time.monotonic()+5
 while not re.search(pattern,data):
  assert time.monotonic()<end,bytes(data)
  if select.select([0],[],[],0.05)[0]:data.extend(os.read(0,4096))
 return bytes(data)
old=termios.tcgetattr(0);tty.setraw(0)
try:
 columns=os.get_terminal_size().columns
 os.write(1,b'\\x1b[?25l\\x1b[0m\\x1b[2J\\x1b[H'+"a中e\\u0301 z".encode()+b'\\x1b[3;1H\\x1b[41m'+b' '*columns+b'\\x1b[0m\\x1b[14t\\x1b[18t')
 (root/'snapshot-size').write_bytes(reply(rb'\\x1b\\[8;\\d+;\\d+t$'))
 end=time.monotonic()+20
 while not (root/'snapshot-refresh').exists():
  assert time.monotonic()<end
  time.sleep(0.01)
 count=0
 while not (root/'snapshot-stop').exists():
  assert time.monotonic()<end
  os.write(1,b'\\x1b[HREPLACED\\x1b[3;1H\\x1b[42m'+b' '*columns+b'\\x1b[0m\\x1b[6n')
  reply(rb'R$')
  count+=1
  pending=root/'snapshot-count.pending';pending.write_text(str(count));pending.replace(root/'snapshot-count')
  time.sleep(0.03)
finally:
 os.write(1,b'\\x1b[0m\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'snapshot-done').touch()
''')
    keyboard.text('python3 ' + str(script))
    keyboard.tap(28)
    keyboard.sync()
    wait_until((root / 'snapshot-size').exists)
    h, w, rows, columns = map(int, re.fullmatch(
        rb'\x1b\[4;(\d+);(\d+)t\x1b\[8;(\d+);(\d+)t',
        (root / 'snapshot-size').read_bytes()).groups())
    cw, ch = w // columns, h // rows
    origin = ch + 8
    node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
    rect, window = node['rect'], node['window_rect']
    x, y = rect['x'] + window['x'], rect['y'] + window['y']
    manager = keyboard.bind('zwlr_virtual_pointer_manager_v1')
    pointer = keyboard.allocate()
    keyboard.send(manager, 0, u32(0, pointer))

    def sync():
        keyboard.send(pointer, 4)
        keyboard.sync()
        time.sleep(0.05)

    def move(column):
        keyboard.send(pointer, 1, u32(0, x + column * cw + cw // 2, y + origin + ch // 2, 800, 600))
        sync()

    def button(pressed):
        keyboard.send(pointer, 2, u32(0, 272, int(pressed)))
        sync()

    def pixel(px, py):
        ppm = subprocess.check_output(['grim', '-t', 'ppm', '-'], env=env)
        _, dimensions, _, pixels = ppm.split(b'\n', 3)
        width, _ = map(int, dimensions.split())
        offset = ((y + py) * width + x + px) * 3
        return pixels[offset:offset + 3]

    def copied(primary=False):
        result = subprocess.run(['wl-paste', '--type', 'text/plain;charset=utf-8', '--no-newline']
                                + (['--primary'] if primary else []), env=env,
                                capture_output=True, timeout=5)
        return result.stdout if result.returncode == 0 else None

    red, green, yellow = bytes.fromhex('cc0000'), bytes.fromhex('4e9a06'), bytes.fromhex('c4a000')
    sample = (10 * cw, origin + 2 * ch + ch // 2)
    footer = (w - 2, origin + h - 2)
    wait_until(lambda: pixel(*sample) == red)
    move(0)
    button(True)
    wait_until(lambda: pixel(*footer) == yellow)
    (root / 'snapshot-refresh').touch()
    wait_until(lambda: (root / 'snapshot-count').exists() and int((root / 'snapshot-count').read_text()) >= 5)
    assert pixel(*sample) == red, 'drag view changed while PTY output continued'
    move(5)
    button(False)
    wait_until(lambda: pixel(*sample) == green)
    wait_until(lambda: 'Selection saved. Use your Copy shortcut.' in (root / 'app.log').read_text())
    expected = 'a中e\u0301 z'.encode()
    wait_until(lambda: copied(True) == expected)
    keyboard.modifiers(5)
    keyboard.tap(30)  # Harness custom Ctrl+Shift+A.
    keyboard.modifiers(0)
    keyboard.sync()
    wait_until(lambda: copied() == expected)
    wait_until(lambda: 'Ptyra clipboard notice: Copied.' in (root / 'app.log').read_text())
    assert pixel(*footer) == yellow
    count = int((root / 'snapshot-count').read_text())
    wait_until(lambda: int((root / 'snapshot-count').read_text()) >= count + 3)
    (root / 'snapshot-stop').touch()
    wait_until((root / 'snapshot-done').exists)
    print('PASS: native frozen drag pixels, live PTY replies, resumed view, English hints, original Unicode Clipboard and Primary')
