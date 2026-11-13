"""Direct Kitty RGBA/PNG, source PTY replies and compositor pixels in owned Sway."""
import base64
import json
import re
import struct
import subprocess
import zlib


def check_graphics(root, env, keyboard, app, wait_until, ipc, nodes):
    script = root / 'graphics-probe.py'
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
  for byte in request.read_bytes(): os.write(1,bytes([byte]))
  os.write(1,b'\x1b[6n');data=b'';end=time.monotonic()+5
  while not data.endswith(b'R'):
   assert time.monotonic()<end
   if select.select([0],[],[],0.05)[0]:data+=os.read(0,65536)
  pending=root/'pending';pending.write_bytes(data);pending.replace(root/f'{phase}.ready');phase+=1
finally:
 os.write(1,b'\x1bc')
 termios.tcsetattr(0,termios.TCSAFLUSH,old)
 (root/'done').touch()
''')
    def probe(name):
        folder=root/name;folder.mkdir()
        keyboard.text('python3 '+str(script)+' '+str(folder));keyboard.tap(28);keyboard.sync()
        phase=0
        def command(data, expected=b''):
            nonlocal phase
            pending=folder/'request';pending.write_bytes(data);pending.replace(folder/f'{phase}.go')
            wait_until((folder/f'{phase}.ready').exists)
            result=(folder/f'{phase}.ready').read_bytes()
            assert re.fullmatch(re.escape(expected)+rb'\x1b\[\d+;\d+R',result),result
            phase+=1
            assert app.poll() is None
        def stop():
            (folder/'stop').touch();wait_until((folder/'done').exists)
        return command,stop
    def apc(header,payload=b''):
        return b'\x1b_G'+header+b';'+payload+b'\x1b\\'
    def reply(id,status=b'OK'):
        return apc(b'i='+str(id).encode(),status)
    def pixel(column):
        output=json.loads(ipc('-t','get_outputs'))[0]['name']
        node=next(n for n in nodes(json.loads(ipc('-t','get_tree'))) if n.get('app_id')=='ptyra')
        rect,window=node['rect'],node['window_rect']
        ppm=subprocess.check_output(['grim','-o',output,'-t','ppm','-'],env=env)
        _,dimensions,_,data=ppm.split(b'\n',3);width,_=map(int,dimensions.split())
        x=rect['x']+window['x']+12*column+6;y=rect['y']+window['y']+28+10
        offset=(y*width+x)*3
        return data[offset:offset+3]
    def key(code,mask=0):
        keyboard.modifiers(mask);keyboard.tap(code);keyboard.modifiers(0);keyboard.sync()
    first,stop_first=probe('graphics-one')
    first(b'\x1bc\x1b[?25l\x1b[H'+apc(b'a=T,i=1,f=32,s=1,v=1,c=1,r=1,C=1',b'/wAA/w=='),reply(1))
    wait_until(lambda:pixel(0)==b'\xff\0\0')
    def chunk(name,data):
        return struct.pack('!I',len(data))+name+data+struct.pack('!I',zlib.crc32(name+data))
    png=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',1,1,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b'\0\0\xff\0\xff'))+chunk(b'IEND',b'')
    first(b'\x1b[1;2H'+apc(b'a=T,i=2,f=100,c=1,r=1,C=1',base64.b64encode(png)),reply(2))
    wait_until(lambda:pixel(1)==b'\0\xff\0')
    first(apc(b'a=q,i=1,f=32,s=1,v=1',b'AAAA/w=='),reply(1))
    assert pixel(0)==b'\xff\0\0'
    first(apc(b'i=9,t=f,q=2',b'AAAA'))
    first(apc(b'i=9,t=s',b'AAAA'),reply(9,b'EACCES:transport disabled'))
    first(b'\x1b[1;3H\x1b[48;2;0;0;255m \x1b[0m\x1b[1;3H'+apc(b'a=T,i=3,f=32,s=2,v=1,x=1,w=1,c=1,r=1,C=1',b'AP8A//8AAIA='),reply(3))
    wait_until(lambda:pixel(2)==b'\x80\0\x7f')
    first(b'\x1b[?1049h\x1b[H'+apc(b'a=p,i=2,C=1,c=1,r=1'),reply(2))
    wait_until(lambda:pixel(0)==b'\0\xff\0')
    first(b'\x1b[?1049l')
    wait_until(lambda:pixel(0)==b'\xff\0\0')
    key(20,5)
    second,stop_second=probe('graphics-two')
    second(b'\x1bc\x1b[?25l\x1b[H'+apc(b'a=T,i=1,f=32,s=1,v=1,c=1,r=1,C=1',b'AAD//w=='),reply(1))
    wait_until(lambda:pixel(0)==b'\0\0\xff')
    first(apc(b'a=d,d=I,i=1'),reply(1))
    assert pixel(0)==b'\0\0\xff'
    key(104,4)
    wait_until(lambda:pixel(1)==b'\0\xff\0')
    assert pixel(0)!=b'\xff\0\0'
    first(b'\x1b[2J')
    wait_until(lambda:pixel(1)!=b'\0\xff\0')
    stop_first();key(109,4);stop_second();key(17,5)
    print('PASS native Graphics: split direct RGBA/PNG, alpha/crop compositor pixels, query/quiet/error replies, screen/hidden Tab isolation, delete/reset/close')
