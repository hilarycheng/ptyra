"""Trusted synthetic fonts through real Fontconfig -> SHM -> compositor pixels."""
import json
from pathlib import Path
import subprocess
from xml.sax.saxutils import escape


def configure_fonts(root, env):
    fixtures = Path(__file__).resolve().parent / 'fixtures'
    config = root / 'fonts.conf'
    config.write_text('''<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig><dir>''' + escape(str(fixtures)) + '''</dir><cachedir>''' + escape(str(root / 'font-cache')) + '''</cachedir>
<alias><family>fallbackprimary</family><prefer><family>fallbackprimary</family><family>fallbackcjk</family><family>fallbackmark</family></prefer></alias></fontconfig>''')
    env['FONTCONFIG_FILE'] = str(config)
    result = subprocess.check_output(['fc-match', '-s', '-f', '%{file}\n', '--', 'fallbackprimary'], env=env)
    for name in [b'fallback-primary.ttf', b'fallback-cjk.ttf', b'fallback-mark.ttf']:
        assert name in result, result
    path = root / 'test.toml'
    path.write_text("font_family='fallbackprimary'\nfont_size=20\n" + path.read_text())


def check_fonts(root, env, keyboard, app, wait_until, ipc, nodes):
    script = root / 'fonts-probe.py'
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
  wait(root/f'fonts-{phase}.go')
  os.write(1,b'\\x1b[0m\\x1b[2J\\x1b[H\\x1b[?25l'+'A中e\\u0301🦀'.encode()+b'\\x1b[6n')
  data=b'';end=time.monotonic()+5
  while not data.endswith(b'R'):
   assert time.monotonic()<end
   if select.select([0],[],[],0.05)[0]: data+=os.read(0,4096)
  assert data==b'\\x1b[1;7R',data
  (root/f'fonts-{phase}.ready').touch()
  wait(root/f'fonts-{phase}.stop')
finally:
 os.write(1,b'\\x1b[0m\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'fonts-done').touch()
''')
    keyboard.text('python3 ' + str(script)); keyboard.tap(28); keyboard.sync()
    output = json.loads(ipc('-t', 'get_outputs'))[0]['name']
    for phase, factor in enumerate([1, 2, 1]):
        ipc(f'output {output} mode {800 * factor}x{600 * factor} scale {factor}')
        (root / f'fonts-{phase}.go').touch()
        wait_until((root / f'fonts-{phase}.ready').exists)
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
                return pixels[offset:offset + 3]
            # Same baseline, real wide CJK in its second cell, negative-bearing mark,
            # and an explicit box when no candidate covers the crab scalar.
            last = [at(x,y).hex() for x,y in [(3,10),(32,10),(40,1),(49,8),(36,10),(3,17),(32,17),(50,8),(73,8)]]
            return all(at(x, y) == bytes.fromhex('d3d7cf') for x, y in [(3, 10), (32, 10), (40, 1), (49, 8)]) and all(at(x, y) == bytes.fromhex('181818') for x, y in [(36, 10), (3, 17), (32, 17), (50, 8), (73, 8)])
        try:
            wait_until(matches)
        except AssertionError:
            raise AssertionError((phase, factor, left, top, last)) from None
        (root / f'fonts-{phase}.stop').touch()
    wait_until((root / 'fonts-done').exists)
    print('PASS: real Fontconfig coverage and compositor CJK/combining/missing-glyph pixels at scale 1/2/1')
