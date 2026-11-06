"""Configured palette, OSC precedence, real compositor pixels and source-Tab replies."""
import json
import re
import subprocess


def configure_palette(root):
    path = root / 'test.toml'
    path.write_text(path.read_text() + "\n[colors]\nforeground='#12ab34'\nbackground='#123456'\ncursor='#abcdef'\n[colors.ansi]\nred='#654321'\nblue='#563412'\n")


def check_palette(root, env, keyboard, app, wait_until, ipc, nodes):
    script = root / 'palette-probe.py'
    script.write_text('''import os,select,sys,termios,time,tty
from pathlib import Path
root=Path(sys.argv[1]);old=termios.tcgetattr(0);tty.setraw(0)
try:
 phase=0
 while not (root/'stop').exists():
  request=root/f'{phase}.go';end=time.monotonic()+30
  while not request.exists():
   if (root/'stop').exists():break
   assert time.monotonic()<end
   time.sleep(0.005)
  if not request.exists():break
  os.write(1,request.read_bytes()+b'\\x1b[6n')
  data=b'';end=time.monotonic()+5
  while not data.endswith(b'R'):
   assert time.monotonic()<end
   if select.select([0],[],[],0.05)[0]:data+=os.read(0,4096)
  pending=root/'pending';pending.write_bytes(data);pending.replace(root/f'{phase}.ready')
  phase+=1
finally:
 os.write(1,b'\\x1b[0m\\x1b[2 q\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'done').touch()
''')
    def probe(name):
        folder = root / name
        folder.mkdir()
        keyboard.text('python3 ' + str(script) + ' ' + str(folder))
        keyboard.tap(28); keyboard.sync()
        phase = 0
        def command(data, expected=b''):
            nonlocal phase
            pending = folder / 'request'
            pending.write_bytes(data); pending.replace(folder / f'{phase}.go')
            wait_until((folder / f'{phase}.ready').exists)
            reply = (folder / f'{phase}.ready').read_bytes()
            assert re.fullmatch(re.escape(expected) + rb'\x1b\[\d+;\d+R', reply), reply
            phase += 1
            assert app.poll() is None, (root / 'app.log').read_text()
        def stop():
            (folder / 'stop').touch()
            wait_until((folder / 'done').exists)
        return command, stop
    output = json.loads(ipc('-t', 'get_outputs'))[0]['name']
    def pixels(columns=4, rows=1):
        node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
        rect, window = node['rect'], node['window_rect']
        x, y = rect['x'] + window['x'], rect['y'] + window['y'] + 28
        ppm = subprocess.check_output(['grim', '-o', output, '-t', 'ppm', '-'], env=env)
        _, dimensions, _, data = ppm.split(b'\n', 3)
        width, height = map(int, dimensions.split())
        assert (width, height) == (800, 600)
        return b''.join(data[((y + row) * width + x) * 3:((y + row) * width + x + columns * 12) * 3] for row in range(rows * 20))
    def query(prefix, rgb):
        channels = [int(rgb[i:i + 2], 16) * 257 for i in (0, 2, 4)]
        return ('\x1b]' + prefix + ';rgb:' + '/'.join(f'{v:04x}' for v in channels) + '\x07').encode()
    def row(*colors):
        return b''.join(bytes.fromhex(rgb) * 12 for rgb in colors) * 20
    first, stop_first = probe('palette-one')
    first(b'\x1bc\x1b[?25l\x1b[2J\x1b[H\x1b[41m \x1b[0m\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07', query('4;1', '654321') + query('10', '12ab34') + query('11', '123456') + query('12', 'abcdef'))
    wait_until(lambda: pixels() == row('654321', '123456', '123456', '123456'))
    first(b'\x1b]4;1;?;1;#112233;1;?\x07\x1b]11;#778899\x07\x1b]12;#aabbcc\x07\x1b[?25h\x1b[2 q', query('4;1', '654321') + query('4;1', '112233'))
    wait_until(lambda: pixels() == row('112233', 'aabbcc', '778899', '778899'))
    keyboard.modifiers(5); keyboard.tap(20); keyboard.modifiers(0); keyboard.sync()
    second, stop_second = probe('palette-two')
    second(b'\x1b[?25l\x1b[2J\x1b[H\x1b[41m \x1b[0m\x1b]4;1;?\x07\x1b]11;?\x07', query('4;1', '654321') + query('11', '123456'))
    wait_until(lambda: pixels() == row('654321', '123456', '123456', '123456'))
    stop_second()
    keyboard.modifiers(5); keyboard.tap(17); keyboard.modifiers(0); keyboard.sync()
    wait_until(lambda: pixels() == row('112233', 'aabbcc', '778899', '778899'))
    first(b'\x1b]104;1\x07\x1b]111\x07\x1b]112\x07\x1b]4;1;?\x07\x1b]11;?\x07\x1b]12;?\x07', query('4;1', '654321') + query('11', '123456') + query('12', 'abcdef'))
    wait_until(lambda: pixels() == row('654321', 'abcdef', '123456', '123456'))
    for code in [2, 4, 6]:
        first(f'\x1b[2J\x1b[H\x1b[{code} q'.encode())
        bg, ink = bytes.fromhex('123456'), bytes.fromhex('abcdef')
        expected = b''.join(ink if code == 2 or (code == 4 and y == 19) or (code == 6 and x == 0) else bg for y in range(20) for x in range(12))
        wait_until(lambda: pixels(1) == expected)
    first(b'\x1b[?25l\x1b[2J\x1b[HA')
    wait_until(lambda: bytes.fromhex('12ab34') in pixels(1))
    stop_first()
    print('PASS: native configured foreground/background/ANSI/cursor pixels, block/beam/underline, query replies, same-OSC precedence, explicit reset and new-Tab isolation')
