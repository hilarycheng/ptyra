#!/usr/bin/env python3
"""Opt-in: taskset -c 0,1 python3 tests/native_configure.py target/debug/ptyra.

Runs the actual binary against an owned, minimal Wayland compositor over
WAYLAND_SOCKET. Requires Linux, Python's stdlib, fc-match and tests/fixtures fonts.
No Sway, desktop connection, keyboard injection or user configuration is used.
This checks wire ordering and live PTYs, not desktop/IME acceptance.
"""
import array
from collections import deque
import json
import os
from pathlib import Path
import select
import socket
import struct
import subprocess
import sys
import tempfile
import time

from native_fonts import configure_fonts


MAX_PIXELS = 4 * 1024 * 1024


def u32(*values):
    return struct.pack("=" + "I" * len(values), *values)


def i32(*values):
    return struct.pack("=" + "i" * len(values), *values)


def string(value):
    value = value.encode() + b"\0"
    return u32(len(value)) + value + bytes(-len(value) % 4)


# Queries give an ordering barrier: receiving their replies proves preceding PTY
# output was parsed, rather than merely fitting inside the kernel's PTY buffer.
PROBE = r'''
import fcntl,json,os,select,struct,termios,time,tty
from pathlib import Path
root=Path(__file__).parent
tty.setraw(0)
def run(index,flood):
    if flood:
        data=b'\x1b[0m'*131072
        while data:
            data=data[os.write(1,data):]
    os.write(1,b'\x1b[?25l\x1b[H'+bytes([65+index%26])+b'\x1b[14t\x1b[18t')
    data=b'';end=time.monotonic()+10
    while data.count(b't')<2:
        assert time.monotonic()<end,repr(data)
        if select.select([0],[],[],0.05)[0]:
            data+=os.read(0,4096)
    import re
    match=re.fullmatch(rb'\x1b\[4;(\d+);(\d+)t\x1b\[8;(\d+);(\d+)t',data)
    assert match,repr(data)
    ph,pw,rows,cols=map(int,match.groups())
    winsize=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\0'*8))
    value=dict(pid=os.getpid(),grid=[rows,cols],cell=[pw//cols,ph//rows],winsize=list(winsize[:2]))
    path=root/f'result-{index}.json'
    path.with_suffix('.tmp').write_text(json.dumps(value))
    path.with_suffix('.tmp').rename(path)
run(-1,False)
index=0
while True:
    path=root/f'action-{index}.json'
    end=time.monotonic()+30
    while not path.exists():
        assert time.monotonic()<end,'parent did not continue probe'
        time.sleep(0.005)
    run(index,json.loads(path.read_text())['flood'])
    index+=1
'''


class Compositor:
    """Only the protocol subset used by Ptyra; unknown requests fail the test."""
    globals = [("wl_compositor", 4), ("wl_shm", 1), ("xdg_wm_base", 1), ("wl_output", 3)]

    def __init__(self, root, binary):
        self.root = root
        root.mkdir()
        self.socket, client = socket.socketpair()
        self.socket.settimeout(5)
        self.pending = b""
        self.fds = deque()
        self.objects = {1: "wl_display"}
        self.pools = {}
        self.buffers = {}
        self.surface = self.xdg = self.top = self.output = None
        self.attached = None
        self.scale = 1
        self.frames = set()
        self.frame_requests = 0
        self.max_frames = 0
        self.commits = []
        self.requests = []
        self.configures = {}
        self.acks = []
        self.serial = 0
        self.staged = (0, 0)
        self.action = 0
        self.pid = None
        self.log = open(root / "app.log", "w")
        env = dict(os.environ, WAYLAND_SOCKET=str(client.fileno()), XDG_RUNTIME_DIR=str(root))
        for key in ("WAYLAND_DISPLAY", "WAYLAND_DEBUG", "SWAYSOCK", "PTYRA_TRACE_INPUT",
                    "PTYRA_TRACE_POINTER", "PTYRA_TRACE_CLIPBOARD"):
            env.pop(key, None)
        (root / "test.toml").write_text("")
        configure_fonts(root, env)
        probe = root / "probe.py"
        probe.write_text("#!" + sys.executable + "\n" + PROBE)
        probe.chmod(0o700)
        env["SHELL"] = str(probe)
        self.app = subprocess.Popen([binary, "--config", str(root / "test.toml")],
                                    env=env, cwd=root, pass_fds=(client.fileno(),),
                                    stdin=subprocess.DEVNULL, stdout=self.log, stderr=self.log)
        client.close()

    def __enter__(self):
        try:
            self.until(lambda: self.requests and self.requests[-1][1:3] == ("wl_surface", 6))
            assert self.surface and self.xdg and self.top
            assert not self.commits, "buffer attached before initial configure"
        except BaseException:
            self.__exit__(*sys.exc_info())
            raise
        return self

    def __exit__(self, kind, value, traceback):
        if kind:
            print(self.root.joinpath("app.log").read_text(), file=sys.stderr)
        try:
            if self.app.poll() is None:
                if self.top is not None:
                    self.send(self.top, 1)  # xdg_toplevel.close
                else:
                    self.app.terminate()
                self.app.wait(timeout=5)
        finally:
            if self.app.poll() is None:
                self.app.kill()
                self.app.wait(timeout=5)
            self.socket.close()
            for fd in list(self.fds) + list(self.pools.values()):
                os.close(fd)
            self.log.close()
        if kind is None:
            assert self.app.returncode == 0, self.root.joinpath("app.log").read_text()

    def send(self, obj, opcode, payload=b""):
        self.socket.sendall(u32(obj, ((len(payload) + 8) << 16) | opcode) + payload)

    def delete(self, obj):
        self.objects.pop(obj)
        self.send(1, 1, u32(obj))  # wl_display.delete_id

    def live(self):
        assert self.app.poll() is None, self.root.joinpath("app.log").read_text()

    def pump(self, seconds=0.05):
        self.live()
        if not select.select([self.socket], [], [], seconds)[0]:
            return
        data, ancillary, flags, _ = self.socket.recvmsg(65536, socket.CMSG_SPACE(64 * 4))
        assert data, "Ptyra disconnected: " + self.root.joinpath("app.log").read_text()
        assert not flags & socket.MSG_CTRUNC
        for level, kind, value in ancillary:
            assert (level, kind) == (socket.SOL_SOCKET, socket.SCM_RIGHTS)
            fds = array.array("i")
            fds.frombytes(value)
            self.fds.extend(fds)
        self.pending += data
        while len(self.pending) >= 8:
            obj, header = struct.unpack_from("=II", self.pending)
            size, opcode = header >> 16, header & 65535
            assert size >= 8 and size % 4 == 0
            if len(self.pending) < size:
                break
            payload, self.pending = self.pending[8:size], self.pending[size:]
            self.request(obj, opcode, payload)

    def until(self, check, seconds=10):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if check():
                return
            self.pump()
        raise AssertionError(("native configure timeout", self.requests[-12:],
                              self.root.joinpath("app.log").read_text()))

    def quiet(self, seconds=0.15):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.pump(min(0.02, end - time.monotonic()))

    def request(self, obj, opcode, payload):
        interface = self.objects[obj]
        words = struct.unpack("=" + "I" * (len(payload) // 4), payload)
        self.requests.append((obj, interface, opcode, words))
        if interface == "wl_display" and opcode in (0, 1):
            new, = words
            self.objects[new] = "wl_callback" if opcode == 0 else "wl_registry"
            if opcode == 0:
                self.send(new, 0, u32(0))
                self.delete(new)
            else:
                for name, (kind, version) in enumerate(self.globals, 1):
                    self.send(new, 0, u32(name) + string(kind) + u32(version))
        elif interface == "wl_registry" and opcode == 0:
            name, length = words[:2]
            kind = payload[8:8 + length - 1].decode()
            version, new = struct.unpack_from("=II", payload, 8 + (length + 3) // 4 * 4)
            assert self.globals[name - 1][0] == kind
            assert version <= self.globals[name - 1][1]
            self.objects[new] = kind
            if kind == "wl_shm":
                self.send(new, 0, u32(0))  # ARGB8888
                self.send(new, 0, u32(1))  # XRGB8888
            elif kind == "wl_output":
                self.output = new
                self.send(new, 0, i32(0, 0, 300, 200, 0) + string("Ptyra test") + string("owned") + i32(0))
                self.send(new, 1, u32(3) + i32(800, 600, 60000))
                self.send(new, 3, i32(1))
                self.send(new, 2)
        elif interface == "wl_compositor" and opcode == 0:
            self.surface, = words
            self.objects[self.surface] = "wl_surface"
            self.send(self.surface, 0, u32(self.output))
        elif interface == "xdg_wm_base" and opcode == 2:
            self.xdg, surface = words
            assert surface == self.surface
            self.objects[self.xdg] = "xdg_surface"
        elif interface == "xdg_surface" and opcode == 1:
            self.top, = words
            self.objects[self.top] = "xdg_toplevel"
        elif interface == "xdg_surface" and opcode == 4:
            serial, = words
            assert serial in self.configures and serial not in self.acks
            self.acks.append(serial)
        elif interface == "xdg_toplevel" and opcode in (2, 3, 8):
            pass  # title, app_id, min_size
        elif interface == "wl_shm" and opcode == 0:
            new, size = words
            assert self.fds and 0 < size <= MAX_PIXELS * 4
            fd = self.fds.popleft()
            assert os.fstat(fd).st_size >= size
            self.objects[new] = "wl_shm_pool"
            self.pools[new] = fd
        elif interface == "wl_shm_pool" and opcode == 0:
            new, offset, width, height, stride, fmt = words
            assert offset == 0 and fmt == 0 and stride == width * 4
            assert 0 < width * height <= MAX_PIXELS
            assert os.fstat(self.pools[obj]).st_size >= height * stride
            self.objects[new] = "wl_buffer"
            self.buffers[new] = (width, height)
        elif interface == "wl_shm_pool" and opcode == 1:
            os.close(self.pools.pop(obj))
            self.delete(obj)
        elif interface == "wl_buffer" and opcode == 0:
            self.buffers.pop(obj)
            self.delete(obj)
        elif interface == "wl_surface" and opcode == 1:
            self.attached, x, y = words
            assert self.attached in self.buffers and (x, y) == (0, 0)
        elif interface == "wl_surface" and opcode == 3:
            new, = words
            self.objects[new] = "wl_callback"
            self.frames.add(new)
            self.frame_requests += 1
            self.max_frames = max(self.max_frames, len(self.frames))
            assert len(self.frames) == 1, "frame callbacks accumulated"
        elif interface == "wl_surface" and opcode == 8:
            self.scale, = words
            assert 1 <= self.scale <= 4
        elif interface == "wl_surface" and opcode == 9:
            pass  # damage_buffer
        elif interface == "wl_surface" and opcode == 6:
            if self.attached is not None:
                assert self.acks, "commit preceded configure acknowledgment"
                serial = self.acks[-1]
                requested = self.configures[serial]
                assert all(value >= 0 for value in requested)
                assert requested[0] * requested[1] <= MAX_PIXELS, "commit after refused configure"
                dimensions = self.buffers[self.attached]
                for physical, logical in zip(dimensions, requested):
                    assert not logical or physical == logical * self.scale, (dimensions, requested, self.scale)
                self.commits.append((serial, dimensions, self.scale))
                self.send(self.attached, 0)  # Release storage, but deliberately withhold frame.done.
                self.attached = None
            else:
                assert not self.acks, "unexpected bufferless commit after configuration"
        else:
            raise AssertionError(("unsupported Wayland request", interface, opcode, words))

    def stage(self, size, maximized=False):
        self.staged = size
        states = u32(1) if maximized else b""
        self.send(self.top, 0, i32(*size) + u32(len(states)) + states)

    def configure(self, size=None, maximized=False, commit=True):
        if size is not None:
            self.stage(size, maximized)
        self.serial += 1
        serial = self.serial
        self.configures[serial] = self.staged
        self.send(self.xdg, 0, u32(serial))
        self.until(lambda: serial in self.acks)
        if commit:
            self.until(lambda: any(item[0] == serial for item in self.commits))
        return serial

    def finish_frames(self):
        for callback in list(self.frames):
            self.send(callback, 0, u32(int(time.monotonic() * 1000) & 0xffffffff))
            self.delete(callback)
            self.frames.remove(callback)

    def probe(self, flood=False, initial=False):
        index = -1 if initial else self.action
        if not initial:
            action = self.root / f"action-{index}.json"
            action.with_suffix(".tmp").write_text(json.dumps({"flood": flood}))
            action.with_suffix(".tmp").rename(action)
            self.action += 1
        path = self.root / f"result-{index}.json"
        self.until(path.exists)
        value = json.loads(path.read_text())
        assert value["grid"] == value["winsize"], value
        if self.pid is None:
            self.pid = value["pid"]
        assert self.pid == value["pid"], "live child was replaced during configure"
        os.kill(self.pid, 0)
        return value

    def no_commits(self, count):
        self.quiet()
        assert len(self.commits) == count, "committed while presentation was paused"


def grid_for(size, cell):
    width, height = size
    cw, ch = cell
    bar = min(ch + 8, height - 1)
    return [max((height - bar) // ch, 1), max(width // cw, 2)]


def check_refusal_and_recovery(root, binary):
    with Compositor(root, binary) as server:
        server.stage((800, 600))
        server.quiet()
        assert not server.commits and not (root / "result--1.json").exists()
        server.configure((2**31 - 1, 2**31 - 1), maximized=True, commit=False)
        server.no_commits(0)
        children = Path(f"/proc/{server.app.pid}/task/{server.app.pid}/children")
        assert not children.read_text().strip(), "first refused configure spawned a child"
        assert not (root / "result--1.json").exists()
        server.configure((800, 600))
        initial = server.probe(initial=True)
        assert initial["grid"] == grid_for((800, 600), initial["cell"])
        warning_count = root.joinpath("app.log").read_text().count("Ptyra configure refused:")
        assert warning_count == 1

        # A toplevel event alone cannot resize the live PTY or latch a new surface.
        server.stage((900, 650), maximized=True)
        assert server.probe()["grid"] == initial["grid"]
        server.configure()
        resized = server.probe()
        assert resized["grid"] == grid_for((900, 650), initial["cell"])
        assert len(server.frames) == server.frame_requests == 1

        count = len(server.commits)
        for size in [(2**31 - 1, 2**31 - 1), (4096, 1025), (4096, 1025)]:
            server.configure(size, maximized=True, commit=False)
            server.no_commits(count)
        # Free the old callback too: absence of commits must be caused by refusal,
        # not merely by a frame callback preventing otherwise-invalid drawing.
        server.finish_frames()
        assert server.probe(flood=True)["grid"] == resized["grid"]
        server.no_commits(count)
        assert root.joinpath("app.log").read_text().count("Ptyra configure refused:") == warning_count + 1
        for size in [(-1, 600), (800, -1), (-1, -1)]:
            server.configure(size, commit=False)
            assert server.probe()["grid"] == resized["grid"]
            server.no_commits(count)
        assert root.joinpath("app.log").read_text().count("Ptyra configure refused:") == warning_count + 2

        server.configure((800, 600))
        assert server.probe()["grid"] == initial["grid"]
        old_frames = set(server.frames)
        callbacks = server.frame_requests
        server.configure((2**31 - 1, 2**31 - 1), maximized=True, commit=False)
        count = len(server.commits)
        assert server.probe(flood=True)["grid"] == initial["grid"]
        server.no_commits(count)
        for size in [(810, 610), (820, 620), (820, 620)]:
            server.configure(size, maximized=True)
            assert server.frames == old_frames and server.frame_requests == callbacks
        # Nothing changed in the grid since the last commit. Even without pixel
        # damage, recovering at exactly the old size must submit a fresh commit.
        server.configure((4096, 1025), maximized=True, commit=False)
        server.no_commits(len(server.commits))
        server.configure((820, 620), maximized=True)
        assert server.frames == old_frames and server.frame_requests == callbacks
        for size, expected in [((0, 630), (820, 630)), ((830, 0), (830, 630)),
                               ((0, 0), (830, 630))]:
            server.configure(size)
            assert server.commits[-1][1:] == (expected, 1)
            assert server.frames == old_frames and server.frame_requests == callbacks
        assert server.probe()["grid"] == grid_for((830, 630), initial["cell"])
        server.finish_frames()
        count = len(server.commits)
        server.probe()
        server.until(lambda: len(server.commits) > count)
        assert server.frame_requests == callbacks + 1

        # Output scale is advisory: preserve exact logical geometry within 4M
        # physical pixels, including an unreasonable preferred integer scale.
        server.send(server.output, 3, i32(2**31 - 1))
        server.send(server.output, 2)
        for size, scale in [((160, 80), 4), ((800, 600), 2), ((1600, 1200), 1)]:
            server.configure(size, maximized=True)
            assert server.commits[-1][1:] == ((size[0] * scale, size[1] * scale), scale)
        assert server.max_frames == 1
        print("PASS: configure/ack/commit ordering, exact maximized sizes, refusal without stale commits, live PTY drain, callback-safe recovery and bounded scale")


def check_grid_limits(root, binary, size):
    with Compositor(root, binary) as server:
        server.configure(size, maximized=True)
        initial = server.probe(initial=True)
        requested = grid_for(size, initial["cell"])
        assert initial["grid"] == [min(requested[0], 24), min(requested[1], 80)]
        assert initial["grid"] != requested, "fixture did not exceed the terminal budget"
        assert server.commits[-1][1:] == (size, 1), "surface geometry was silently shrunk"
        assert "Ptyra resize rejected:" in root.joinpath("app.log").read_text()
        server.configure((800, 600))
        normal = server.probe()
        assert normal["grid"] == grid_for((800, 600), initial["cell"])
        server.configure(size, maximized=True)
        assert server.probe(flood=True)["grid"] == normal["grid"]
        assert server.commits[-1][1:] == (size, 1)
        server.configure((810, 610))
        assert server.probe()["grid"] == grid_for((810, 610), initial["cell"])
        assert server.max_frames == 1
        print(f"PASS: {size[0]}x{size[1]} bounded startup, retained grid/PTY and recovery")


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    binary = str(Path(sys.argv[1]).resolve())
    assert os.access(binary, os.X_OK), binary
    with tempfile.TemporaryDirectory(prefix="ptyra-configure-") as folder:
        root = Path(folder)
        check_refusal_and_recovery(root / "configure", binary)
        for name, size in [("thin", (1, MAX_PIXELS)), ("wide", (MAX_PIXELS, 1)),
                           ("history-budget", (20000, 80))]:
            check_grid_limits(root / name, binary, size)
    print("PASS: owned native configure integration (desktop acceptance not covered)")


if __name__ == "__main__":
    main()
