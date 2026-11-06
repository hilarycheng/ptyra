"""Match native color and geometry replies to actual compositor cells and PTY sizes."""
import json
import re
import subprocess
import time


def check_queries(root, env, keyboard, app, wait_until, ipc, nodes):
    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    def capture():
        node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
        rect, window = node['rect'], node['window_rect']
        ppm = subprocess.check_output(['grim', '-t', 'ppm', '-'], env=env)
        _, dimensions, _, pixels = ppm.split(b'\n', 3)
        width, height = map(int, dimensions.split())
        x, y = rect['x'] + window['x'], rect['y'] + window['y']
        w, h = rect['width'] - 2 * window['x'], rect['height'] - window['y'] - window['x']
        assert 0 <= x < x+w <= width and 0 <= y < y+h <= height
        return [pixels[(r*width+x)*3:(r*width+x+w)*3] for r in range(y, y+h)]

    ipc('[app_id="ptyra"] border none')
    ipc('[app_id="ptyra"] floating enable')
    time.sleep(0.3)
    ipc('[app_id="ptyra"] resize set width 640 px height 400 px')
    time.sleep(0.3)
    for number in [1, 2]:
        script = root / f'queries-{number}.py'
        script.write_text('''import json,os,re,select,termios,time,tty
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
number=''' + str(number) + '''
def gate(name):
 end=time.monotonic()+20
 while not (root/name).exists():
  assert time.monotonic()<end, 'gate timeout'
  time.sleep(0.01)
old=termios.tcgetattr(0)
tty.setraw(0)
try:
 (root/f'queries-{number}.ready').write_text('ready')
 for phase in range(3):
  gate(f'queries-{phase}.go')
  color='123456' if number==1 else '654321'
  change=(f'\\x1b]4;1;?;1;#{color};1;?\\x07'.encode() if phase==1 else b'\\x1b]104;1\\x07')
  os.write(1,change+b'\\x1b[0m\\x1b[2J\\x1b[H\\x1b[?25l\\x1b[41m \\x1b[42m \\x1b[0m')
  os.write(1,b'\\x1b]4;1;?\\x07\\x1b]10;?\\x07\\x1b]11;?\\x07\\x1b]12;?\\x07\\x1b[14t\\x1b[18t\\x1b]52;c;?\\x07')
  received=bytearray()
  end=time.monotonic()+5
  while not re.search(rb'\\x1b\\[8;\\d+;\\d+t$',received) and time.monotonic()<end:
   if select.select([0],[],[],0.1)[0]: received.extend(os.read(0,65536))
  assert re.search(rb'\\x1b\\[8;\\d+;\\d+t$',received), bytes(received)
  assert not select.select([0],[],[],0.2)[0], 'extra or clipboard reply'
  size=os.get_terminal_size()
  (root/f'queries-{number}-{phase}.json').write_text(json.dumps({'reply':received.hex(),'rows':size.lines,'columns':size.columns}))
 gate('queries-exit')
finally:
 os.write(1,b'\\x1b[0m\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSANOW,old)
''')
        keyboard.text('python3 ' + str(script))
        key(28)
        wait_until((root / f'queries-{number}.ready').exists)
        if number == 1:
            key(20, 5)
            time.sleep(0.2)

    active = 2
    dimensions = []
    for phase in range(3):
        if phase == 1:
            ipc('[app_id="ptyra"] resize set width 500 px height 300 px')
            time.sleep(0.3)
        (root / f'queries-{phase}.go').write_text('go')
        for number in [1, 2]:
            path = root / f'queries-{number}-{phase}.json'
            wait_until(path.exists)
            result = json.loads(path.read_text())
            response = bytes.fromhex(result['reply'])
            expected_color = bytes.fromhex(('123456' if number == 1 else '654321') if phase == 1 else 'cc0000')
            rgb = '/'.join(f'{channel:02x}'*2 for channel in expected_color)
            palette = f'\x1b]4;1;rgb:{rgb}\x07'.encode() + b'\x1b]10;rgb:d3d3/d7d7/cfcf\x07\x1b]11;rgb:1818/1818/1818\x07\x1b]12;rgb:eeee/eeee/ecec\x07'
            if phase == 1:
                palette = b'\x1b]4;1;rgb:cccc/0000/0000\x07' + f'\x1b]4;1;rgb:{rgb}\x07'.encode() + palette
            assert response.startswith(palette), response
            match = re.fullmatch(rb'\x1b\[4;(\d+);(\d+)t\x1b\[8;(\d+);(\d+)t', response[len(palette):])
            assert match, response
            pixel_h, pixel_w, rows, columns = map(int, match.groups())
            assert (rows, columns) == (result['rows'], result['columns'])
            dimensions.append((rows, columns))
            if active != number:
                key(104 if number == 1 else 109, 4)
                active = number
            def marker():
                image = capture()
                colored = [y for y, line in enumerate(image) if line.startswith(expected_color*3)]
                if not colored:
                    return None
                line = image[colored[0]]
                cell_w = next((x for x in range(0, len(line)//3) if line[x*3:x*3+3] != expected_color), 0)
                cell_h = len(colored)
                return image, colored, cell_w, cell_h
            image, colored, cell_w, cell_h = wait_until(marker)
            assert colored == list(range(colored[0], colored[0]+cell_h))
            assert colored[0] == cell_h + 8, 'text begins below the actual tab strip'
            assert image[colored[0]][cell_w*3:cell_w*6] == b'\x4e\x9a\x06'*cell_w
            assert (pixel_w, pixel_h) == (columns*cell_w, rows*cell_h)
            assert (len(image[0])//3)//cell_w == columns
            assert (len(image)-(cell_h+8))//cell_h == rows
    assert dimensions[0] != dimensions[2], 'resize must change actual PTY geometry'
    (root / 'queries-exit').write_text('exit')
    time.sleep(0.3)
    if active != 2:
        key(109, 4)
    key(17, 5)
    time.sleep(0.1)
    assert app.poll() is None, (root / 'app.log').read_text()
    print('PASS: native source-Tab color defaults/overrides/resets and query geometry match compositor cell pixels plus both PTY sizes before/after resize; OSC 52 reads remain denied')
