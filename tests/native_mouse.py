"""Native raw-PTY application mouse bytes and exclusive Shift selection/Primary routing."""
import json
import subprocess
import time


def check_mouse(root, env, keyboard, app, wait_until, ipc, nodes, u32):
    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    script = root / 'mouse-probe.py'
    script.write_text('''import json,os,re,select,termios,time,tty
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
def wait(path):
 end=time.monotonic()+30
 while not path.exists():
  assert time.monotonic()<end, str(path)
  time.sleep(0.01)
def read_until(pattern):
 data=bytearray(); end=time.monotonic()+5
 while not re.search(pattern,data):
  assert time.monotonic()<end,bytes(data)
  if select.select([0],[],[],0.1)[0]: data.extend(os.read(0,65536))
 return bytes(data)
old=termios.tcgetattr(0);tty.setraw(0)
try:
 os.write(1,b'\\x1b[14t\\x1b[18t')
 size=read_until(rb'\\x1b\\[8;\\d+;\\d+t$')
 (root/'mouse-size').write_text(size.hex())
 for phase in range(10):
  wait(root/f'mouse-{phase}.go')
  mode=bytes.fromhex((root/f'mouse-{phase}.go').read_text())
  os.write(1,b'\\x1b[?1000l\\x1b[?1002l\\x1b[?1003l\\x1b[?1005l\\x1b[?1006l'+mode+b'\\x1b[0m\\x1b[2J\\x1b[Halpha beta\\x1b[?25l\\x1b[6n')
  assert read_until(rb'R$')==b'\\x1b[1;11R'
  (root/f'mouse-{phase}.ready').touch()
  data=bytearray(); end=time.monotonic()+30
  while not (root/f'mouse-{phase}.stop').exists():
   assert time.monotonic()<end
   if select.select([0],[],[],0.02)[0]: data.extend(os.read(0,65536))
  while select.select([0],[],[],0.15)[0]: data.extend(os.read(0,65536))
  (root/f'mouse-{phase}.result').write_text(data.hex())
finally:
 os.write(1,b'\\x1b[?1000l\\x1b[?1002l\\x1b[?1003l\\x1b[?1005l\\x1b[?1006l\\x1b[?25h\\r\\n')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
''')
    keyboard.text('python3 ' + str(script))
    key(28)
    wait_until((root / 'mouse-size').exists)
    import re
    h,w,rows,cols = map(int,re.fullmatch(rb'\x1b\[4;(\d+);(\d+)t\x1b\[8;(\d+);(\d+)t',bytes.fromhex((root/'mouse-size').read_text())).groups())
    cw,ch = w//cols,h//rows
    origin=ch+8
    node=next(n for n in nodes(json.loads(ipc('-t','get_tree'))) if n.get('app_id')=='ptyra')
    rect,window=node['rect'],node['window_rect']
    left,top=rect['x']+window['x'],rect['y']+window['y']
    manager=keyboard.bind('zwlr_virtual_pointer_manager_v1')
    pointer=keyboard.allocate()
    keyboard.send(manager,0,u32(0,pointer))

    def sync():
        keyboard.send(pointer,4)
        keyboard.sync()
        time.sleep(0.03)

    def move(col,row,bar=False,jitter=0):
        x=left+col*cw+cw//2+jitter
        y=top+(0 if bar else origin)+row*ch+ch//2
        keyboard.send(pointer,1,u32(0,x,y,800,600))
        sync()

    def button(code,pressed):
        keyboard.send(pointer,2,u32(0,code,int(pressed)))
        sync()

    def click(code=272):
        button(code,True)
        button(code,False)

    def wheel(value,axis=0):
        keyboard.send(pointer,3,u32(0,axis,int(value*256)&0xffffffff))
        sync()

    modes=[b'',b'\x1b[?1000h\x1b[?1006h',b'\x1b[?1002h\x1b[?1006h',b'\x1b[?1003h\x1b[?1006h',b'\x1b[?1000h',b'\x1b[?1000h\x1b[?1005h',b'\x1b[?1000h\x1b[?1006h',b'\x1b[?1000h\x1b[?1006h']
    modes.extend([b'\x1b[?1002h\x1b[?1006h',b'\x1b[?1003h\x1b[?1006h'])
    for phase,mode in enumerate(modes):
        move(1,0,bar=True)
        (root/f'mouse-{phase}.go').write_text(mode.hex())
        wait_until((root/f'mouse-{phase}.ready').exists)
        key(107,1) # Keep later fixtures on the live grid after the disabled-mode history wheel.
        move(1,1)
        expected=b''
        if phase==0:
            click()
            wheel(-10)
        elif phase==1:
            subprocess.run(['wl-copy','--primary','no-paste'],env=env,check=True,timeout=5)
            for code,index in [(272,0),(274,1),(273,2)]:
                click(code)
                expected+=f'\x1b[<{index};2;2M\x1b[<{index};2;2m'.encode()
            wheel(-5);wheel(-5);wheel(20);wheel(-10,1);wheel(10,1)
            expected+=b'\x1b[<64;2;2M\x1b[<65;2;2M\x1b[<65;2;2M\x1b[<66;2;2M\x1b[<67;2;2M'
            keyboard.modifiers(12);keyboard.sync();click();keyboard.modifiers(0);keyboard.sync()
            expected+=b'\x1b[<24;2;2M\x1b[<24;2;2m'
        elif phase==2:
            button(272,True);move(1,1,jitter=1);move(2,1);button(272,False)
            expected=b'\x1b[<0;2;2M\x1b[<32;3;2M\x1b[<0;3;2m'
        elif phase==3:
            move(1,1,jitter=1);move(2,1)
            expected=b'\x1b[<35;2;2M\x1b[<35;3;2M'
        elif phase in (4,5):
            click()
            expected=b'\x1b[M \x22\x22\x1b[M#\x22\x22'
        elif phase==6:
            # Releasing Shift mid-drag must not turn selection into app motion/release.
            move(0,0)
            keyboard.modifiers(1);keyboard.sync();button(272,True)
            keyboard.modifiers(0);keyboard.sync();move(4,0);button(272,False)
            def primary():
                result=subprocess.run(['wl-paste','--primary','--no-newline'],env=env,capture_output=True,timeout=5)
                return result.stdout
            wait_until(lambda:primary()==b'alpha')
            keyboard.modifiers(1);keyboard.sync();button(274,True)
            keyboard.modifiers(0);keyboard.sync();button(274,False)
            expected=b'alpha'
        elif phase==7:
            move(1,0,bar=True);click();move(1,1)
            # Original press is cancelled on focus loss, never released into a new focus.
            button(272,True)
            ipc('workspace number 2');time.sleep(0.1)
            button(272,False)
            ipc('workspace number 1');time.sleep(0.1)
            expected=b'\x1b[<0;2;2M'
        elif phase==8:
            for scroll in [lambda:wheel(-10),lambda:key(104,1)]:
                button(272,True)
                before=(root/'app.log').read_text().count('Ptyra viewport:')
                keyboard.modifiers(1);keyboard.sync();scroll()
                keyboard.modifiers(0);keyboard.sync()
                wait_until(lambda:(root/'app.log').read_text().count('Ptyra viewport:')>before)
                button(272,False)
                expected+=b'\x1b[<0;2;2M\x1b[<0;2;2m'
        elif phase==9:
            keyboard.modifiers(1);keyboard.sync();button(272,True)
            keyboard.modifiers(0);keyboard.sync()
            before=(root/'app.log').read_text().count('Ptyra viewport:')
            wheel(-10)
            wait_until(lambda:(root/'app.log').read_text().count('Ptyra viewport:')>before)
            move(2,1);button(272,False);move(3,1)
            expected=b'\x1b[<35;2;2M\x1b[<35;4;2M'
        (root/f'mouse-{phase}.stop').touch()
        wait_until((root/f'mouse-{phase}.result').exists)
        actual=bytes.fromhex((root/f'mouse-{phase}.result').read_text())
        assert actual==expected,(phase,actual,expected)
    keyboard.send(pointer,8) # Destroy the owned virtual pointer.
    keyboard.sync()
    time.sleep(0.2)
    assert app.poll() is None,(root/'app.log').read_text()
    print('PASS: native exact SGR/legacy/UTF8 click/release/drag/all-motion/wheel/modifier bytes, cell deduplication, exclusive Shift selection/Primary paste, strip exclusion and focus cancellation')
