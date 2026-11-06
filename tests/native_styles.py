"""SGR styles through fixed synthetic fonts and actual compositor pixels."""
import json
from pathlib import Path
import subprocess


def check_styles(root, env, keyboard, app, wait_until, ipc, nodes):
    styles = [('22;23', 'A'), ('1', 'A'), ('2', 'A'), ('3', 'A'), ('1;3', 'A'),
              ('7', 'A'), ('1;3;4;8;9', 'A'), ('9', ' '), ('4', ' '),
              ('4:2', ' '), ('4:3', ' '), ('4:4', ' '), ('4:5', ' '),
              ('4;58;2;18;52;86', ' ')]
    content = ''.join('\x1b[0;38;2;200;160;120;48;2;16;32;48;' + sgr + 'm' + char
                      for sgr, char in styles)
    content += '\r\n\x1b[0;38;2;200;160;120;48;2;16;32;48;1;3;4;9m中\u0301\x1b[0mA'
    script = root / 'styles-probe.py'
    script.write_text('''import os,select,termios,time,tty
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
def wait(path):
 end=time.monotonic()+30
 while not path.exists():
  assert time.monotonic()<end
  time.sleep(0.01)
old=termios.tcgetattr(0);tty.setraw(0)
try:
 for phase in range(3):
  wait(root/f'styles-{phase}.go')
  os.write(1,b'\\x1b[0m\\x1b[2J\\x1b[H\\x1b[?25l'+''' + repr(content.encode()) + '''+b'\\x1b[6n')
  data=b'';end=time.monotonic()+5
  while not data.endswith(b'R'):
   assert time.monotonic()<end
   if select.select([0],[],[],0.05)[0]: data+=os.read(0,4096)
  assert data==b'\\x1b[2;4R',data
  (root/f'styles-{phase}.ready').touch()
  wait(root/f'styles-{phase}.stop')
finally:
 os.write(1,b'\\x1b[0m\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'styles-done').touch()
''')
    keyboard.text('python3 ' + str(script)); keyboard.tap(28); keyboard.sync()
    output = json.loads(ipc('-t', 'get_outputs'))[0]['name']
    fg, bg, dim = 'c8a078', '102030', '64503c'
    probes = [(2, 4, fg), (10, 8, bg), (22, 8, fg), (27, 8, dim),
              (38, 4, bg), (47, 4, fg), (48, 0, bg), (60, 0, fg), (63, 8, bg),
              (75, 8, bg), (72, 19, bg), (84, 10, fg), (84, 9, bg),
              (96, 19, fg), (96, 17, bg), (108, 17, fg), (108, 18, bg),
              (122, 17, fg), (122, 19, bg), (132, 19, fg), (133, 19, bg),
              (147, 19, fg), (148, 19, bg), (156, 19, '123456'),
              (23, 39, fg), (24, 39, '181818'), (26, 24, 'd3d7cf')]
    for phase, factor in enumerate([1, 2, 1]):
        ipc(f'output {output} mode {800 * factor}x{600 * factor} scale {factor}')
        (root / f'styles-{phase}.go').touch()
        wait_until((root / f'styles-{phase}.ready').exists)
        node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
        rect, window = node['rect'], node['window_rect']
        left, top = rect['x'] + window['x'], rect['y'] + window['y'] + 28
        last = None
        def matches():
            nonlocal last
            assert app.poll() is None, (root / 'app.log').read_text()
            ppm = subprocess.check_output(['grim', '-o', output, '-t', 'ppm', '-'], env=env)
            _, dimensions, _, pixels = ppm.split(b'\n', 3)
            width, height = map(int, dimensions.split())
            assert (width, height) == (800 * factor, 600 * factor)
            def at(x, y):
                offset = (((top + y) * factor) * width + (left + x) * factor) * 3
                return pixels[offset:offset + 3].hex()
            last = [(x, y, expected, at(x, y)) for x, y, expected in probes if at(x, y) != expected]
            return not last
        try:
            wait_until(matches)
        except AssertionError:
            raise AssertionError((phase, factor, last)) from None
        (root / f'styles-{phase}.stop').touch()
    wait_until((root / 'styles-done').exists)
    print('PASS: compositor SGR bold/dim/italic/inverse/hidden/strike/underline variants and colors at scale 1/2/1')
