"""Actual compositor pixels stay unchanged during sync; end/deadline releases without PTY stalls."""
import json
import subprocess
import time


def check_sync(root, env, keyboard, app, wait_until, ipc, nodes):
    def key(code):
        keyboard.modifiers(0);keyboard.tap(code);keyboard.sync()
    script=root/'sync-probe.py'
    script.write_text('''import os,re,select,termios,time,tty
from pathlib import Path
root=Path('''+repr(str(root))+''')
def wait(path):
 end=time.monotonic()+20
 while not path.exists():
  assert time.monotonic()<end
  time.sleep(0.001)
def response(pattern):
 data=bytearray();end=time.monotonic()+5
 while not re.search(pattern,data):
  assert time.monotonic()<end,bytes(data)
  if select.select([0],[],[],0.05)[0]:data.extend(os.read(0,4096))
 return bytes(data)
old=termios.tcgetattr(0);tty.setraw(0)
try:
 os.write(1,b'\\x1b[?2026l\\x1b[?25l\\x1b[44m\\x1b[2J\\x1b[H\\x1b[6n')
 assert response(rb'R$')==b'\\x1b[1;1R'
 (root/'sync-initial').touch()
 for phase,color in enumerate([41,42,44,42]):
  wait(root/f'sync-{phase}.go')
  started=time.monotonic()
  os.write(1,b'\\x1b[?2026h'+f'\\x1b[{color}m\\x1b[2J\\x1b[Hinside'.encode()+b'\\x1b[?2026$p\\x1b[6n')
  assert response(rb'R$')==b'\\x1b[?2026;1$y\\x1b[1;7R'
  (root/f'sync-{phase}.ready').write_text(str(started))
  data=bytearray();next_begin=time.monotonic()+0.02
  while not (root/f'sync-{phase}.stop').exists():
   assert time.monotonic()-started<20
   if phase>=2 and time.monotonic()>=next_begin and (phase==2 or time.monotonic()-started<0.14):
    os.write(1,b'\\x1b[?2026h' if phase==2 else b'\\x1bc\\x1b[?2026h\\x1b[?25l\\x1b[42m\\x1b[2J');next_begin=time.monotonic()+0.02
   if select.select([0],[],[],0.001)[0]:
    data.extend(os.read(0,4096))
    pending=root/f'sync-{phase}.pending';pending.write_bytes(data);pending.replace(root/f'sync-{phase}.input')
  if phase==0:os.write(1,b'\\x1b[?2026l')
  os.write(1,b'\\x1b[?2026$p')
  assert response(rb'y$')==b'\\x1b[?2026;2$y'
  os.write(1,b'\\x1b[?2026l')
  (root/f'sync-{phase}.done').write_text(str(time.monotonic()))
finally:
 os.write(1,b'\\x1b[?2026l\\x1b[0m\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'sync-exit').touch()
''')
    keyboard.text('python3 '+str(script));key(28)
    wait_until((root/'sync-initial').exists)
    node=next(n for n in nodes(json.loads(ipc('-t','get_tree'))) if n.get('app_id')=='ptyra')
    rect,window=node['rect'],node['window_rect']
    x=rect['x']+window['x']+100;y=rect['y']+window['y']+150
    def pixel():
        ppm=subprocess.check_output(['grim','-t','ppm','-'],env=env)
        _,dimensions,_,pixels=ppm.split(b'\n',3);width,height=map(int,dimensions.split())
        assert 0<=x<width and 0<=y<height
        return pixels[(y*width+x)*3:(y*width+x+1)*3]
    colors=[bytes.fromhex('3465a4'),bytes.fromhex('cc0000'),bytes.fromhex('4e9a06'),bytes.fromhex('3465a4'),bytes.fromhex('4e9a06')]
    wait_until(lambda:pixel()==colors[0])
    for phase in range(4):
        (root/f'sync-{phase}.go').touch();wait_until((root/f'sync-{phase}.ready').exists)
        started=float((root/f'sync-{phase}.ready').read_text())
        held=pixel();elapsed=time.monotonic()-started
        assert elapsed<0.15,('test missed hold interval',phase,elapsed)
        assert held==colors[phase],('intermediate frame leaked',phase,held)
        if phase==0:
            key(48)
            wait_until(lambda:(root/'sync-0.input').exists() and (root/'sync-0.input').read_bytes()==b'b')
            (root/'sync-0.stop').touch();wait_until((root/'sync-0.done').exists)
            assert float((root/'sync-0.done').read_text())-started<0.15,'end must precede timeout'
            wait_until(lambda:pixel()==colors[1])
        else:
            wait_until(lambda:pixel()==colors[phase+1],seconds=2)
            elapsed=time.monotonic()-started
            assert 0.13<=elapsed<1,('missing end did not release on deadline',elapsed)
            if phase==1:
                # No PTY output is pending: the deadline alone must have woken presentation.
                stat=lambda:sum(map(int,open('/proc/'+str(app.pid)+'/stat').read().split()[13:15]))
                before=stat();time.sleep(0.25);assert stat()-before<=3,'runtime busy-looped after timeout'
            (root/f'sync-{phase}.stop').touch();wait_until((root/f'sync-{phase}.done').exists)
        time.sleep(0.05)
    wait_until((root/'sync-exit').exists)
    assert app.poll() is None,(root/'app.log').read_text()
    print('PASS: native synchronized pixel hold, early end, silent/repeated-begin/reset 150ms timeout, immediate queries/keyboard and no post-timeout busy loop')
