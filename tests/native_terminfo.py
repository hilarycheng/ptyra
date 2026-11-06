"""Actual ncurses default-color pairs and advertised ANSI colors through compositor pixels."""
import json
import subprocess


def check_terminfo(root, env, keyboard, app, wait_until, ipc, nodes):
    script = root / 'terminfo-probe.py'
    script.write_text('''import curses,os,select,termios,time,tty
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
def check(screen):
 curses.start_color()
 assert curses.COLORS==16 and curses.COLOR_PAIRS==256
 curses.use_default_colors()
 # htop initializes its normal color pairs with default background (-1).
 for index in range(16):
  curses.init_pair(index+1,index,-1)
  assert curses.pair_content(index+1)==(index,-1)
 # Check the full advertised pair range, not only htop's 64 pairs.
 curses.init_pair(255,15,15)
 assert curses.pair_content(255)==(15,15)
curses.wrapper(check)
foreground=[curses.tparm(curses.tigetstr('setaf'),i) for i in range(16)]
background=[curses.tparm(curses.tigetstr('setab'),i) for i in range(16)]
assert foreground==[f'\\x1b[{30+i if i<8 else 90+i-8}m'.encode() for i in range(16)]
assert background==[f'\\x1b[{40+i if i<8 else 100+i-8}m'.encode() for i in range(16)]
assert curses.tigetstr('op')==b'\\x1b[39;49m'
assert curses.tigetstr('bold')==b'\\x1b[1m'
old=termios.tcgetattr(0);tty.setraw(0)
try:
 data=b'\\x1bc\\x1b[?25l'+b''.join(code+b' ' for code in background)
 data+=b'\\x1b[0m\\r\\n'+b''.join(code+b'A' for code in foreground)
 data+=b'\\x1b[0m\\r\\n\\x1b[31;44m'+curses.tigetstr('bold')+curses.tigetstr('op')+b'A\\x1b[0m\\x1b[6n'
 os.write(1,data)
 reply=b'';end=time.monotonic()+5
 while not reply.endswith(b'R'):
  assert time.monotonic()<end
  if select.select([0],[],[],0.05)[0]:reply+=os.read(0,4096)
 assert reply==b'\\x1b[3;2R',reply
 (root/'terminfo-ready').touch()
 end=time.monotonic()+15
 while not (root/'terminfo-stop').exists():
  assert time.monotonic()<end
  time.sleep(.01)
finally:
 os.write(1,b'\\x1b[0m\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'terminfo-done').touch()
''')
    keyboard.text('python3 ' + str(script)); keyboard.tap(28); keyboard.sync()
    wait_until((root / 'terminfo-ready').exists)
    output = json.loads(ipc('-t', 'get_outputs'))[0]['name']
    node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
    rect, window = node['rect'], node['window_rect']
    left, top = rect['x'] + window['x'], rect['y'] + window['y'] + 28
    colors = ['181818', 'cc0000', '4e9a06', 'c4a000', '3465a4', '75507b', '06989a', 'd3d7cf',
              '555753', 'ef2929', '8ae234', 'fce94f', '729fcf', 'ad7fa8', '34e2e2', 'eeeeec']
    last = None
    def matches():
        nonlocal last
        assert app.poll() is None, (root / 'app.log').read_text()
        ppm = subprocess.check_output(['grim', '-o', output, '-t', 'ppm', '-'], env=env)
        _, dimensions, _, pixels = ppm.split(b'\n', 3)
        width, height = map(int, dimensions.split())
        assert (width, height) == (800, 600)
        def at(x, y):
            offset = ((top + y) * width + left + x) * 3
            return pixels[offset:offset + 3].hex()
        probes = [(index * 12 + 2, y, color) for index, color in enumerate(colors) for y in (4, 24)]
        probes += [(2, 44, 'd3d7cf'), (10, 48, 'd3d7cf'), (0, 40, '181818')]
        last = [(x, y, expected, at(x, y)) for x, y, expected in probes if at(x, y) != expected]
        return not last
    try:
        wait_until(matches)
    except AssertionError:
        raise AssertionError(last) from None
    (root / 'terminfo-stop').touch()
    wait_until((root / 'terminfo-done').exists)
    print('PASS: embedded ncurses default-background pairs, 16-color tparm expansion, 256 pairs, bright ANSI foreground/background pixels and op preserves bold')
