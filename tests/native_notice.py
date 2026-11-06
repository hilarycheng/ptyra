"""Visible recoverable outcomes inside the native fixture's private compositor."""
import array
import base64
import struct
import os
import socket
import subprocess
import time


def check_notices(root, env, keyboard, app, wait_until, ipc, Keyboard, u32, string):
    log = lambda: (root / 'app.log').read_text()
    def visible():
        ppm = subprocess.check_output(['grim', '-t', 'ppm', '-'], env=env, timeout=5)
        return b'\xc4\xa0\x00' * 12 in ppm[-800*80*3:]
    def paste():
        keyboard.modifiers(5); keyboard.tap(47); keyboard.modifiers(0); keyboard.sync()
    def notice(text, action):
        expected = 'Ptyra clipboard notice: ' + text
        before = log().count(expected)
        action()
        wait_until(lambda: log().count(expected) > before)
        wait_until(visible)
        assert app.poll() is None
    def copied(payload, mime='text/plain;charset=utf-8'):
        subprocess.run(['wl-copy', '--type', mime], input=payload, env=env, check=True, timeout=5)
        time.sleep(0.1)
        paste()
    notice('Clipboard is empty.', lambda: copied(b''))
    subprocess.run(['wl-copy', '--clear'], env=env, check=True, timeout=5)
    time.sleep(0.1)
    notice('Clipboard is empty.', paste)
    notice('Clipboard has no text.', lambda: copied(b'sentinel-secret', 'application/octet-stream'))
    notice('Clipboard is not UTF-8.', lambda: copied(b'\xff'))
    notice('Clipboard exceeds limit.', lambda: copied(b'sentinel-secret-not-for-log'))
    if os.environ.get('PTYRA_TEST_NOTICE_SCREENSHOT'):
        subprocess.run(['grim', os.environ['PTYRA_TEST_NOTICE_SCREENSHOT']], env=env, check=True)

    class Owner(Keyboard):
        def __init__(self):
            self.fds = []
            self.sock = socket.socket(socket.AF_UNIX)
            self.sock.settimeout(5)
            self.sock.connect(str(root / env['WAYLAND_DISPLAY']))
            self.next_id = 3
            self.pending = b''
            self.globals = {}
            self.send(1, 1, u32(2))
            self.sync()
            seat = self.bind('wl_seat')
            manager = self.bind('zwlr_data_control_manager_v1')
            source = self.allocate()
            self.send(manager, 0, u32(source))
            self.send(source, 0, string('text/plain;charset=utf-8'))
            device = self.allocate()
            self.send(manager, 1, u32(device, seat))
            self.send(device, 0, u32(source))
            self.sync()
        def receive(self):
            data, ancillary, _, _ = self.sock.recvmsg(65536, socket.CMSG_SPACE(16))
            assert data
            for level, kind, value in ancillary:
                if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                    fds = array.array('i'); fds.frombytes(value); self.fds.extend(fds)
            self.pending += data
        def requested(self):
            self.sync()
            return bool(self.fds)
        def release(self):
            for fd in self.fds:
                os.close(fd)
            self.fds.clear()
    # Hold the writer open: owner disappearance, not EOF, must cancel the pending read.
    owner = Owner()
    try:
        time.sleep(0.1)
        paste()
        wait_until(owner.requested)
        notice('Clipboard owner changed.', owner.close)
    finally:
        owner.close()
        owner.release()
    # Deliberate focus cancellation is quiet and never becomes an owner-loss warning.
    owner = Owner()
    try:
        time.sleep(0.1)
        paste()
        wait_until(owner.requested)
        before = log().count('Ptyra clipboard notice:')
        ipc('workspace number 2'); time.sleep(0.1)
        ipc('workspace number 1'); time.sleep(0.1)
        assert log().count('Ptyra clipboard notice:') == before
        wait_until(lambda: not visible())
    finally:
        owner.close()
        owner.release()
    # Ask the native source to send into a pipe whose reader is already closed.
    # EPIPE must affect only that send, not revoke ownership or kill the shell.
    publish = root / 'notice-source.py'
    publish.write_text('import os\nos.write(1, '+repr(b'\x1b]52;c;'+base64.b64encode(b'copy-recovered')+b'\x07')+')\n')
    keyboard.text('python3 '+str(publish)); keyboard.tap(28); keyboard.sync()
    wait_until(lambda: subprocess.run(['wl-paste', '--no-newline'], env=env, capture_output=True, timeout=5).stdout == b'copy-recovered')
    class Observer(Owner):
        def __init__(self):
            self.fds = []
            self.offer = None
            self.device = None
            self.sock = socket.socket(socket.AF_UNIX)
            self.sock.settimeout(5)
            self.sock.connect(str(root / env['WAYLAND_DISPLAY']))
            self.next_id = 3
            self.pending = b''
            self.globals = {}
            self.send(1, 1, u32(2)); self.sync()
            seat = self.bind('wl_seat')
            manager = self.bind('zwlr_data_control_manager_v1')
            self.device = self.allocate()
            self.send(manager, 1, u32(self.device, seat)); self.sync()
        def receive(self):
            super().receive()
            pending = self.pending
            while len(pending) >= 8:
                obj, header = struct.unpack_from('=II', pending)
                size, opcode = header >> 16, header & 65535
                if len(pending) < size: break
                if obj == self.device and opcode == 1:
                    self.offer = struct.unpack_from('=I', pending, 8)[0]
                pending = pending[size:]
        def break_reader(self):
            assert self.offer
            read, write = os.pipe2(os.O_CLOEXEC)
            os.close(read)
            try: self.send(self.offer, 0, string('text/plain;charset=utf-8'), write)
            finally: os.close(write)
            self.sync()
    observer = Observer()
    try:
        notice('Clipboard transfer failed.', observer.break_reader)
        assert subprocess.check_output(['wl-paste', '--no-newline'], env=env, timeout=5) == b'copy-recovered'
    finally:
        observer.close()
        observer.release()
    # Normal keyboard input remains live and dismisses the non-modal metadata strip.
    proof = root / 'notice-shell-proof'
    keyboard.text('echo ok > ' + str(proof)); keyboard.tap(28); keyboard.sync()
    wait_until(proof.exists)
    assert proof.read_text().strip() == 'ok'
    # A later valid paste reaches the original shell unchanged.
    keyboard.text('echo ')
    subprocess.run(['wl-copy', '--type', 'text/plain', 'recovered'], env=env, check=True, timeout=5)
    time.sleep(0.1); paste(); time.sleep(0.1)
    proof2 = root / 'notice-paste-proof'
    keyboard.text(' > ' + str(proof2)); keyboard.tap(28); keyboard.sync()
    wait_until(proof2.exists)
    assert proof2.read_text().strip() == 'recovered'
    wait_until(lambda: not visible())
    assert 'sentinel-secret' not in log()
    print('PASS: visible empty/non-text/cap/UTF-8/owner-loss/EPIPE metadata, quiet cancellation, live Shell and later Paste recovery')
