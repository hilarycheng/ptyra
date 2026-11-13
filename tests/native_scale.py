"""Owned headless outputs: integer scaling, physical pixels, input and output migration."""
import json
import re
import subprocess
import time


def check_scale(root, env, keyboard, app, wait_until, ipc, nodes, u32):
    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()
    script = root / 'scale-probe.py'
    script.write_text('''import os,re,select,termios,time,tty
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
def wait(path):
 end=time.monotonic()+30
 while not path.exists():
  assert time.monotonic()<end
  time.sleep(0.01)
def read(pattern):
 data=bytearray();end=time.monotonic()+5
 while not re.search(pattern,data):
  assert time.monotonic()<end,bytes(data)
  if select.select([0],[],[],0.05)[0]:data.extend(os.read(0,4096))
 return bytes(data)
old=termios.tcgetattr(0);tty.setraw(0)
try:
 for phase in range(5):
  wait(root/f'scale-{phase}.go')
  os.write(1,b'\\x1b[0m\\x1b[2J\\x1b[H\\x1b[?25l\\x1b[41mAB\\x1b[42mCD\\x1b[0m\\x1b[2;1H'+ 'A中e\\u0301 beta'.encode()+b'\\x1b[?1000h\\x1b[?1006h\\x1b[14t\\x1b[18t\\x1b[6n')
  data=read(rb'R$');(root/f'scale-{phase}.size').write_bytes(data)
  (root/f'scale-{phase}.ready').touch()
  data=read(rb'm$');(root/f'scale-{phase}.mouse').write_bytes(data)
  os.write(1,b'\\x1b[?1000l\\x1b[?1006l')
  (root/f'scale-{phase}.local').touch()
  wait(root/f'scale-{phase}.stop')
finally:
 os.write(1,b'\\x1b[?1000l\\x1b[?1006l\\x1b[0m\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'scale-done').touch()
''')
    keyboard.text('python3 ' + str(script)); key(28)
    outputs = json.loads(ipc('-t', 'get_outputs'))
    first = outputs[0]['name']
    second = None
    manager = keyboard.bind('zwlr_virtual_pointer_manager_v1')
    pointer = keyboard.allocate()
    keyboard.send(manager, 0, u32(0, pointer))

    def move(x, y, extent):
        keyboard.send(pointer, 1, u32(0, x, y, extent, 600))
        keyboard.send(pointer, 4); keyboard.sync(); time.sleep(0.04)
    def click():
        keyboard.send(pointer, 2, u32(0, 272, 1))
        keyboard.send(pointer, 2, u32(0, 272, 0))
        keyboard.send(pointer, 4); keyboard.sync(); time.sleep(0.06)
    def copied():
        result = subprocess.run(['wl-paste', '--type', 'text/plain', '--no-newline'], env=env, capture_output=True, timeout=5)
        return result.stdout if result.returncode == 0 else None
    base_cell = None
    base_grid = None
    for phase, factor in enumerate([1, 2, 1, 2, 1]):
        output, offset, extent = first, 0, 800
        if phase == 1:
            ipc(f'output {first} mode 1600x1200 scale 2')
        elif phase == 2:
            ipc(f'output {first} mode 800x600 scale 1')
        elif phase == 3:
            ipc(f'output {first} position 0 0')
            ipc('create_output')
            second = wait_until(lambda: next((o['name'] for o in json.loads(ipc('-t', 'get_outputs')) if o['name'] != first), None))
            ipc(f'output {second} mode 1600x1200 scale 2 position 800 0')
            ipc(f'[app_id="ptyra"] move container to output {second}')
            ipc('[app_id="ptyra"] focus')
            output, offset, extent = second, 800, 1600
        elif phase == 4:
            ipc(f'output {second} disable')
            ipc('[app_id="ptyra"] focus')
        time.sleep(0.2)
        assert app.poll() is None, (root / 'app.log').read_text()
        (root/f'scale-{phase}.go').touch()
        wait_until((root/f'scale-{phase}.ready').exists)
        data = (root/f'scale-{phase}.size').read_bytes()
        ph, pw, rows, cols, row, col = map(int, re.fullmatch(rb'\x1b\[4;(\d+);(\d+)t\x1b\[8;(\d+);(\d+)t\x1b\[(\d+);(\d+)R', data).groups())
        ch, cw = ph//rows, pw//cols
        if base_cell is None:
            base_cell, base_grid = (cw, ch), (rows, cols)
        assert (cw, ch) == (base_cell[0]*factor, base_cell[1]*factor), (phase, cw, ch, base_cell)
        assert (rows, cols) == base_grid, (phase, rows, cols, base_grid)
        assert (row, col) == (2, 10), (row, col)
        node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
        rect, window = node['rect'], node['window_rect']
        left, top = rect['x']+window['x'], rect['y']+window['y']
        origin = ch//factor + 8
        move(left + 300, top + 200, extent)  # Keep software cursors outside sampled cells.
        def pixels_match():
            ppm = subprocess.check_output(['grim', '-o', output, '-t', 'ppm', '-'], env=env)
            _, dimensions, _, pixels = ppm.split(b'\n', 3)
            width, height = map(int, dimensions.split())
            assert (width, height) == (800*factor, 600*factor)
            x, y = (left-offset)*factor, (top+origin)*factor
            # The bottom of each cell is pure background, independent of font antialiasing.
            line = pixels[((y+ch-1)*width+x)*3:((y+ch-1)*width+x+cw*4)*3]
            return line == bytes.fromhex('cc0000')*(2*cw) + bytes.fromhex('4e9a06')*(2*cw)
        wait_until(pixels_match)
        move(left + base_cell[0]*3//2, top+origin+base_cell[1]*3//2, extent)
        click()
        wait_until((root/f'scale-{phase}.mouse').exists)
        assert (root/f'scale-{phase}.mouse').read_bytes() == b'\x1b[<0;2;2M\x1b[<0;2;2m'
        wait_until((root/f'scale-{phase}.local').exists)
        subprocess.run(['wl-copy', '--type', 'text/plain'], input=b'scale-copy-sentinel', env=env, check=True, timeout=5)
        wait_until(lambda: copied() == b'scale-copy-sentinel')
        time.sleep(0.45)
        # Double-click the second half of the CJK cell, then copy exact Unicode.
        move(left + base_cell[0]*5//2, top+origin+base_cell[1]*3//2, extent)
        click()
        move(left + base_cell[0]*5//2 + 3, top+origin+base_cell[1]*3//2, extent)
        click(); key(30, 5)
        wait_until(lambda: copied() == '中'.encode())
        if phase == 1:
            key(20, 5)
            keyboard.text('echo ready > '+str(root/'scale-second-tab')); key(28)
            wait_until((root/'scale-second-tab').exists)
            move(left+base_cell[0], top+base_cell[1]//2, extent); click()
            wait_until(pixels_match)
            # Select and close only the new Tab; return to the raw-PTY probe.
            key(109, 4); key(17, 5)
            wait_until(pixels_match)
        (root/f'scale-{phase}.stop').touch()
    wait_until((root/'scale-done').exists)
    assert app.poll() is None, (root/'app.log').read_text()
    ipc('[app_id="ptyra"] move container to workspace number 1')
    ipc('workspace number 1')
    time.sleep(0.1)
    print('PASS: native scale 1/2/1, output move/removal, doubled pixel/cell metrics, stable logical grid, scaled Tab click, exact mouse and jittered CJK selection Copy')
