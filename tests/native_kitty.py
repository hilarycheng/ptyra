"""Negotiated Kitty bytes through real Wayland keys and two independent raw PTYs."""
import json
import time


def check_kitty(root, keyboard, app, wait_until):
    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    script = root / 'kitty-probe.py'
    script.write_text(r'''
import json,os,select,sys,termios,time,tty
from pathlib import Path
root=Path(__file__).parent
number,count=map(int,sys.argv[1:])
old=termios.tcgetattr(0)
tty.setraw(0)
try:
 (root/f'kitty-{number}.running').touch()
 for phase in range(count):
  path=root/f'kitty-{number}-{phase}'
  end=time.monotonic()+30
  while not path.with_suffix('.go').exists():
   assert time.monotonic()<end, 'mode gate timeout'
   time.sleep(0.01)
  mode,flags=json.loads(path.with_suffix('.go').read_text())
  for byte in bytes.fromhex(mode)+b'\x1b[?u': os.write(1,bytes([byte]))
  reply=bytearray()
  while len(reply)<5:
   assert time.monotonic()<end, 'query timeout'
   if select.select([0],[],[],0.05)[0]: reply.extend(os.read(0,65536))
  assert reply==f'\x1b[?{flags}u'.encode(), (number,phase,bytes(reply),flags)
  path.with_suffix('.ready').touch()
  received=bytearray()
  while not path.with_suffix('.stop').exists():
   assert time.monotonic()<end, 'input gate timeout'
   if select.select([0],[],[],0.02)[0]: received.extend(os.read(0,65536))
   assert len(received)<=65536, 'unbounded input'
  while select.select([0],[],[],0.15)[0]:
   assert time.monotonic()<end and len(received)<=65536
   received.extend(os.read(0,65536))
  pending=path.with_suffix('.result.pending')
  pending.write_text(received.hex())
  pending.replace(path.with_suffix('.result'))
finally:
 os.write(1,b'\x1bc')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/f'kitty-{number}.done').touch()
''')

    def start(number, count):
        keyboard.text(f'python3 {script} {number} {count}')
        key(28)
        wait_until((root / f'kitty-{number}.running').exists)

    def begin(number, phase, mode, flags):
        path = root / f'kitty-{number}-{phase}'
        pending = path.with_suffix('.go.pending')
        pending.write_text(json.dumps([mode.hex(), flags]))
        pending.replace(path.with_suffix('.go'))
        wait_until(path.with_suffix('.ready').exists)

    def finish(number, phase):
        path = root / f'kitty-{number}-{phase}'
        path.with_suffix('.stop').touch()
        wait_until(path.with_suffix('.result').exists)
        return bytes.fromhex(path.with_suffix('.result').read_text())

    modes = [
        (b'', 0), (b'\x1b[=31u', 1), (b'\x1b[>0u', 0), (b'\x1b[<u', 1),
        (b'\x1b[=1;3u', 0), (b'\x1b[=31;2u', 1),
        (b'\x1b[>30u', 0), (b'\x1b[<u', 1),
        (b'\x1b[?1049h', 0), (b'\x1b[>1u', 1), (b'\x1b[?1049l', 1),
        (b'\x1b[=0u', 0), (b'\x1b[?1049h', 1), (b'\x1bc', 0),
        (b'\x1b[?1049h', 0), (b'\x1b[?1049l', 0),
    ]
    start(1, len(modes) + 2)
    for phase, (mode, flags) in enumerate(modes):
        begin(1, phase, mode + b'\x1b=\x1b[?1h', flags)
        expected = b''
        for code, mask, legacy, enhanced in [
            (30, 4, b'\x01', b'\x1b[97;5u'),
            (30, 8, b'\x1ba', b'\x1b[97;3u'),
            (48, 5, b'\x02', b'\x1b[98;6u'),
            (28, 1, b'\r', b'\x1b[13;2u'),
            (103, 0, b'\x1bOA', b'\x1b[A'),
            (59, 0, b'\x1bOP', b'\x1b[P'),
            (96, 0, b'\x1bOM', b'\x1b[57414u'),
            (1, 0, b'\x1b', b'\x1b[27u'),
            (28, 0, b'\r', b'\r'), (15, 0, b'\t', b'\t'),
            (14, 0, b'\x7f', b'\x7f'), (30, 1, b'A', b'A'),
            (30, 5, b'', b''),  # The fixture binds Copy; its press/release stays consumed.
        ]:
            key(code, mask)
            expected += enhanced if flags else legacy
        assert finish(1, phase) == expected, (phase, flags)

    phase = len(modes)
    begin(1, phase, b'\x1b[=1u', 1)
    keyboard.modifiers(4)
    keyboard.key(30)
    keyboard.sync()
    time.sleep(0.85)
    keyboard.key(30, False)
    keyboard.modifiers(0)
    keyboard.sync()
    repeated = finish(1, phase)
    token = b'\x1b[97;5u'
    assert len(repeated) >= 2 * len(token) and repeated == token * (len(repeated) // len(token))

    key(20, 5)
    start(2, 1)
    begin(2, 0, b'\x1b[=31u', 1)
    # Hidden Tab 1 resets its flags while active Tab 2 stays enhanced.
    begin(1, phase + 1, b'\x1b[=0u', 0)
    key(30, 4)
    key(28, 1)
    key(104, 4)
    key(30, 4)
    key(109, 4)
    assert finish(2, 0) == b'\x1b[97;5u\x1b[13;2u'
    wait_until((root / 'kitty-2.done').exists)
    key(17, 5)
    key(28, 1)
    assert finish(1, phase + 1) == b'\x01\r'
    wait_until((root / 'kitty-1.done').exists)
    time.sleep(0.1)
    assert app.poll() is None, (root / 'app.log').read_text()
    print('PASS: native Kitty bit-1 negotiation, exact key bytes, repeat, shortcuts, main/alt stacks, hidden Tab isolation and reset')
