"""Owned fake xdg-open: literal argv, explicit click, history, bounded waits and failure recovery."""
import json
import os
import re
import time


def prepare_opener(root, env):
    folder=root/'opener-bin';folder.mkdir()
    helper=folder/'xdg-open'
    helper.write_text('''#!/usr/bin/python3
import json,os,sys,time
from pathlib import Path
root=Path('''+repr(str(root))+''')
with (root/'opened.jsonl').open('a') as out:
 out.write(json.dumps({'args':sys.argv[1:],'pid':os.getpid()})+'\\n')
sys.stdout.write('SECRET_OPENER_STDOUT');sys.stderr.write('SECRET_OPENER_STDERR')
if (root/'opener-hold').exists():
 end=time.monotonic()+15
 while not (root/'opener-release').exists():
  if time.monotonic()>end: sys.exit(3)
  time.sleep(0.01)
sys.exit(1 if (root/'opener-fail').exists() else 0)
''')
    helper.chmod(0o700)
    env['PATH']=str(folder)+os.pathsep+env.get('PATH','/usr/bin:/bin')


def check_hyperlink(root, keyboard, app, wait_until, ipc, nodes, u32):
    uri='https://example.test/$(touch${IFS}'+str(root/'injected')+")?x='quoted'&y=1"
    def key(code,mask=0):
        keyboard.modifiers(mask);keyboard.tap(code);keyboard.modifiers(0);keyboard.sync()
    script=root/'hyperlink-probe.py'
    script.write_text('''import os,re,select,termios,time,tty
from pathlib import Path
root=Path('''+repr(str(root))+''')
uri='''+repr(uri)+'''
def wait(path):
 end=time.monotonic()+30
 while not path.exists():
  assert time.monotonic()<end
  time.sleep(0.01)
def response(pattern):
 data=bytearray();end=time.monotonic()+5
 while not re.search(pattern,data):
  assert time.monotonic()<end,bytes(data)
  if select.select([0],[],[],0.1)[0]: data.extend(os.read(0,4096))
 return bytes(data)
old=termios.tcgetattr(0);tty.setraw(0)
try:
 os.write(1,b'\\x1b[14t\\x1b[18t')
 size=response(rb'\\x1b\\[8;\\d+;\\d+t$')
 (root/'hyperlink-size').write_text(size.hex())
 rows=int(re.search(rb'8;(\\d+);',size)[1])
 for phase in range(3):
  wait(root/f'hyperlink-{phase}.go')
  os.write(1,b'\\x1b[?1000l\\x1b[?1006l\\x1b[0m\\x1b[2J\\x1b[3J\\x1b[H'+(b'\\x1b[?1000h\\x1b[?1006h' if phase==2 else b''))
  os.write(1,('\\x1b]8;;'+uri+'\\x1b\\\\link\\x1b]8;;\\x1b\\\\').encode())
  os.write(1,b'\\r\\n\\x1b]8;;file:///tmp/denied\\x1b\\\\denied\\x1b]8;;\\x1b\\\\\\r\\nplain')
  if phase==1: os.write(1,b'\\r\\nline'*rows)
  os.write(1,b'\\x1b[6n');response(rb'R$')
  (root/f'hyperlink-{phase}.ready').touch()
  received=bytearray();end=time.monotonic()+30
  while not (root/f'hyperlink-{phase}.stop').exists():
   assert time.monotonic()<end
   if select.select([0],[],[],0.02)[0]:
    received.extend(os.read(0,4096))
    pending=root/'hyperlink-input.pending';pending.write_bytes(received);pending.replace(root/'hyperlink-input')
  (root/f'hyperlink-{phase}.bytes').write_bytes(received)
finally:
 os.write(1,b'\\x1b[?1000l\\x1b[?1006l\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'hyperlink-done').touch()
''')
    keyboard.text('python3 '+str(script));key(28)
    wait_until((root/'hyperlink-size').exists)
    h,w,rows,cols=map(int,re.fullmatch(rb'\x1b\[4;(\d+);(\d+)t\x1b\[8;(\d+);(\d+)t',bytes.fromhex((root/'hyperlink-size').read_text())).groups())
    cw,ch=w//cols,h//rows;origin=ch+8
    node=next(n for n in nodes(json.loads(ipc('-t','get_tree'))) if n.get('app_id')=='ptyra')
    rect,window=node['rect'],node['window_rect'];left,top=rect['x']+window['x'],rect['y']+window['y']
    manager=keyboard.bind('zwlr_virtual_pointer_manager_v1');pointer=keyboard.allocate();keyboard.send(manager,0,u32(0,pointer))
    def move(row=0,col=1,bar=False):
        keyboard.send(pointer,1,u32(0,left+col*cw+cw//2,top+(0 if bar else origin)+row*ch+ch//2,800,600))
        keyboard.send(pointer,4);keyboard.sync();time.sleep(0.04)
    def click(mask=4,button=272):
        keyboard.modifiers(mask);keyboard.sync()
        keyboard.send(pointer,2,u32(0,button,1));keyboard.send(pointer,2,u32(0,button,0));keyboard.send(pointer,4)
        keyboard.sync();keyboard.modifiers(0);keyboard.sync();time.sleep(0.04)
    def records():
        path=root/'opened.jsonl'
        if not path.exists(): return []
        return [json.loads(line) for line in path.read_text().splitlines()]
    def count(expected):
        wait_until(lambda:len(records())==expected)
        assert all(record['args']==[uri] for record in records())
    def phase(number):
        (root/f'hyperlink-{number}.go').touch();wait_until((root/f'hyperlink-{number}.ready').exists);time.sleep(0.1)
    def stop(number):
        (root/f'hyperlink-{number}.stop').touch();wait_until((root/f'hyperlink-{number}.bytes').exists)
    phase(0)
    move();keyboard.modifiers(4);keyboard.sync();move(col=2);keyboard.modifiers(0);keyboard.sync()
    click(0);click(4,273);click(12);move(row=1);click();move(bar=True);click();time.sleep(0.15)
    assert records()==[],'output, hover or nonqualifying click must not launch'
    move();click();count(1);time.sleep(0.1);assert len(records())==1
    (root/'opener-hold').touch()
    for expected in range(2,6): click();count(expected)
    click();time.sleep(0.1);assert len(records())==5
    wait_until(lambda:'Link could not be opened: unavailable or busy.' in (root/'app.log').read_text())
    key(48);wait_until(lambda:(root/'hyperlink-input').exists() and (root/'hyperlink-input').read_bytes()==b'b')
    (root/'opener-release').touch()
    wait_until(lambda:all(not os.path.exists('/proc/'+str(record['pid'])) for record in records()))
    time.sleep(0.1)
    (root/'opener-fail').touch();before=(root/'app.log').read_text().count('Link could not be opened')
    click();count(6)
    wait_until(lambda:(root/'app.log').read_text().count('Link could not be opened')>before)
    (root/'opener-fail').unlink();click();time.sleep(0.15);assert len(records())==6
    click();count(7)
    stop(0);assert (root/'hyperlink-0.bytes').read_bytes()==b'b'
    phase(1)
    move();click();time.sleep(0.1);assert len(records())==7
    key(102,8 if os.environ.get('PTYRA_TEST_SCROLL') else 1);time.sleep(0.15)
    move();click();count(8)
    stop(1);assert (root/'hyperlink-1.bytes').read_bytes()==b''
    phase(2);key(107,1);move();click();time.sleep(0.1);assert len(records())==8
    click(5);count(9)
    stop(2)
    assert (root/'hyperlink-2.bytes').read_bytes()==b'\x1b[<16;2;1M\x1b[<16;2;1m'
    wait_until((root/'hyperlink-done').exists)
    wait_until(lambda:all(not os.path.exists('/proc/'+str(record['pid'])) for record in records()))
    assert not (root/'injected').exists()
    log=(root/'app.log').read_text()
    assert uri not in log and 'SECRET_OPENER' not in log
    assert app.poll() is None,log
    keyboard.send(pointer,8);keyboard.sync()
    print('PASS: native explicit HTTP(S) Ctrl+click, literal argv, history hit, mouse ownership, bounded async opener completion/failure and no payload logs')
