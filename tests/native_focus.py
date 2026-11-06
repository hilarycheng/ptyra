"""Exact focus-in/out reports on enabled raw PTYs; disabled and stale targets stay quiet."""
import time


def check_focus(root, keyboard, app, wait_until, ipc):
    def key(code,mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    script=root/'focus-probe.py'
    script.write_text('''import os,re,select,sys,termios,time,tty
from pathlib import Path
root=Path('''+repr(str(root))+''')
number=sys.argv[1]
old=termios.tcgetattr(0);tty.setraw(0)
try:
 os.write(1,(b'\\x1b[?1004h' if number!='3' else b'\\x1b[?1004l')+b'\\x1b[6n')
 reply=bytearray();end=time.monotonic()+5
 while not re.search(rb'R$',reply):
  assert time.monotonic()<end
  if select.select([0],[],[],0.1)[0]: reply.extend(os.read(0,4096))
 assert re.fullmatch(rb'\\x1b\\[\\d+;\\d+R',reply),bytes(reply)
 path=root/f'focus-{number}.bytes';path.write_bytes(b'')
 (root/f'focus-{number}.ready').touch()
 received=bytearray();end=time.monotonic()+30
 while not (root/'focus-exit').exists():
  assert time.monotonic()<end
  if select.select([0],[],[],0.02)[0]:
   received.extend(os.read(0,4096))
   pending=path.with_suffix('.pending');pending.write_bytes(received);pending.replace(path)
finally:
 os.write(1,b'\\x1b[?1004l')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/f'focus-{number}.done').touch()
''')
    expected={1:b'',2:b'',3:b''}
    def expect(number,extra=b''):
        expected[number]+=extra
        path=root/f'focus-{number}.bytes'
        wait_until(lambda:path.exists() and path.read_bytes()==expected[number])

    def start(number):
        keyboard.text('python3 '+str(script)+' '+str(number))
        key(28)
        wait_until((root/f'focus-{number}.ready').exists)
        expect(number)

    start(1)
    key(20,5)
    expect(1,b'\x1b[O')
    start(2)
    key(104,4)
    expect(2,b'\x1b[O');expect(1,b'\x1b[I')
    key(109,4)
    expect(1,b'\x1b[O');expect(2,b'\x1b[I')
    ipc('workspace number 2')
    expect(2,b'\x1b[O')
    ipc('workspace number 2') # Duplicate no-op leave cannot repeat the report.
    time.sleep(0.1)
    expect(2)
    ipc('workspace number 1')
    expect(2,b'\x1b[I')
    ipc('[app_id="ptyra"] focus')
    time.sleep(0.1)
    expect(2)
    key(20,5)
    expect(2,b'\x1b[O')
    start(3) # Mode disabled across focus, switch, and close.
    ipc('workspace number 2');time.sleep(0.1)
    ipc('workspace number 1');time.sleep(0.1)
    expect(3)
    key(104,4)
    expect(2,b'\x1b[I');expect(3)
    key(109,4)
    expect(2,b'\x1b[O');expect(3)
    key(17,5)
    expect(2,b'\x1b[I');expect(3)
    key(17,5)
    expect(1,b'\x1b[I')
    time.sleep(0.2)
    for number in [1,2,3]:
        assert (root/f'focus-{number}.bytes').read_bytes()==expected[number]
    (root/'focus-exit').touch()
    wait_until((root/'focus-1.done').exists)
    time.sleep(0.1)
    assert app.poll() is None,(root/'app.log').read_text()
    print('PASS: native exact enabled-Tab focus-in/out for switching, window leave/enter and close successor; duplicate/no-op focus and disabled third Tab stay quiet')
