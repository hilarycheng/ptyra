"""Raw-PTY legacy keypad bytes from real XKB/Wayland modifiers and per-Tab modes."""
import time


def check_keypad(root, keyboard, app, wait_until, u32):
    def modifiers(mask=0,lock=0):
        keyboard.send(keyboard.keyboard,2,u32(mask,0,lock,0))

    def key(code,mask=0,lock=0):
        modifiers(mask,lock)
        keyboard.tap(code)
        modifiers()
        keyboard.sync()

    script=root/'keypad-probe.py'
    script.write_text('''import os,re,select,sys,termios,time,tty
from pathlib import Path
root=Path('''+repr(str(root))+''')
number=int(sys.argv[1]);count=6 if number==1 else 1
old=termios.tcgetattr(0);tty.setraw(0)
def wait(path):
 end=time.monotonic()+30
 while not path.exists():
  assert time.monotonic()<end,str(path)
  time.sleep(0.01)
try:
 (root/f'keypad-{number}.running').touch()
 for phase in range(count):
  path=root/f'keypad-{number}-{phase}'
  wait(path.with_suffix('.go'))
  mode=bytes.fromhex(path.with_suffix('.go').read_text())
  os.write(1,mode+b'\\x1b[6n')
  reply=bytearray();end=time.monotonic()+5
  while not re.search(rb'R$',reply):
   assert time.monotonic()<end
   if select.select([0],[],[],0.1)[0]: reply.extend(os.read(0,65536))
  assert re.fullmatch(rb'\\x1b\\[\\d+;\\d+R',reply),bytes(reply)
  path.with_suffix('.ready').touch()
  received=bytearray();end=time.monotonic()+30
  while not path.with_suffix('.stop').exists():
   assert time.monotonic()<end
   if select.select([0],[],[],0.02)[0]: received.extend(os.read(0,65536))
  while select.select([0],[],[],0.15)[0]: received.extend(os.read(0,65536))
  path.with_suffix('.result').write_text(received.hex())
finally:
 os.write(1,b'\\x1b>\\x1b[?1l')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/f'keypad-{number}.done').touch()
''')
    def start(number):
        keyboard.text('python3 '+str(script)+' '+str(number))
        key(28)
        wait_until((root/f'keypad-{number}.running').exists)

    def begin(number,phase,mode):
        path=root/f'keypad-{number}-{phase}'
        path.with_suffix('.go').write_text(mode.hex())
        wait_until(path.with_suffix('.ready').exists)

    def finish(number,phase,expected):
        path=root/f'keypad-{number}-{phase}'
        path.with_suffix('.stop').touch()
        wait_until(path.with_suffix('.result').exists)
        actual=bytes.fromhex(path.with_suffix('.result').read_text())
        assert actual==expected,(number,phase,actual,expected)

    keys=[(82,b'\x1b[2~',b'p',b'0'),(79,b'\x1b[F',b'q',b'1'),(80,b'\x1b[B',b'r',b'2'),
          (81,b'\x1b[6~',b's',b'3'),(75,b'\x1b[D',b't',b'4'),(76,b'\x1b[E',b'u',b'5'),
          (77,b'\x1b[C',b'v',b'6'),(71,b'\x1b[H',b'w',b'7'),(72,b'\x1b[A',b'x',b'8'),
          (73,b'\x1b[5~',b'y',b'9'),(83,b'\x1b[3~',b'n',b'.'),(96,b'\r',b'M',b'\r'),
          (98,b'/',b'o',b'/'),(55,b'*',b'j',b'*'),(74,b'-',b'm',b'-'),(78,b'+',b'k',b'+')]
    start(1)
    for phase,(cursor,keypad) in enumerate([(False,False),(False,True),(True,False),(True,True)]):
        begin(1,phase,(b'\x1b=' if keypad else b'\x1b>')+(b'\x1b[?1h' if cursor else b'\x1b[?1l'))
        expected=b''
        for lock in [0,16]: # The native evdev/US map binds NumLock to Mod2.
            for code,normal,suffix,numeric in keys:
                key(code,lock=lock)
                value=numeric if lock else b'\x1bO'+suffix if keypad else normal
                if cursor and not keypad and not lock and code in [79,80,75,77,71,72]:
                    value=value.replace(b'\x1b[',b'\x1bO')
                expected+=value
        for mask in [1,4,5,9,12,13]:
            key(72,mask)
            modifier=1+bool(mask&1)+2*bool(mask&8)+4*bool(mask&4)
            expected+=f'\x1b[1;{modifier}A'.encode()
        key(72,8)
        expected+=b'\x1b\x1bOx' if keypad else b'\x1b[1;3A'
        for code,mask,value in [(28,0,b'\r'),(28,1,b'\r'),(46,4,b'\x03'),(48,5,b'\x02'),
                                (59,0,b'\x1bOP'),(60,0,b'\x1bOQ'),(61,0,b'\x1bOR'),(62,0,b'\x1bOS'),
                                (63,0,b'\x1b[15~'),(103,4,b'\x1b[1;5A'),(48,64,b'')]:
            key(code,mask);expected+=value
        finish(1,phase,expected)
    # Different live Tabs retain different APP_KEYPAD modes.
    begin(1,4,b'\x1b=\x1b[?1l')
    key(20,5)
    start(2)
    begin(2,0,b'\x1b>\x1b[?1l')
    key(96)
    key(104,4)
    key(96)
    key(109,4)
    finish(2,0,b'\r')
    wait_until((root/'keypad-2.done').exists)
    key(17,5)
    finish(1,4,b'\x1bOM')
    # RIS resets both modes and the input path samples the reset core state.
    begin(1,5,b'\x1bc')
    key(96);key(72);key(103)
    finish(1,5,b'\r\x1b[A\x1b[A')
    wait_until((root/'keypad-1.done').exists)
    time.sleep(0.1)
    assert app.poll() is None,(root/'app.log').read_text()
    print('PASS: native XKB keypad digits/operators/Enter/navigation, NumLock, APP_KEYPAD/APP_CURSOR, modified navigation, unchanged core legacy keys, Tab isolation and RIS reset; no unsolicited Kitty sequences')
