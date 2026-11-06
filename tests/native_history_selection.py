"""Real Wayland history selection/Copy, independent of deferred desktop acceptance."""
import json
import re
import subprocess
import time


def check_history_selection(root, env, keyboard, wait_until, ipc, nodes, u32, capture, key, top_mask, baseline, start):
    node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
    rect, window = node['rect'], node['window_rect']
    x, y = rect['x'] + window['x'], rect['y'] + window['y']
    manager = keyboard.bind('zwlr_virtual_pointer_manager_v1')
    pointer = keyboard.allocate()
    keyboard.send(manager, 0, u32(0, pointer))
    pattern = r'Ptyra selection geometry: (-?\d+):(\d+)-(-?\d+):(\d+) cell=(\d+):(\d+) origin=(\d+)'

    def click(px, py):
        keyboard.send(pointer, 1, u32(0, x + px, y + py, 800, 600))
        keyboard.send(pointer, 4)
        keyboard.sync()
        time.sleep(0.03)
        keyboard.send(pointer, 2, u32(0, 272, 1))
        keyboard.send(pointer, 2, u32(0, 272, 0))
        keyboard.send(pointer, 4)
        keyboard.sync()
        time.sleep(0.05)

    def copied():
        result = subprocess.run(['wl-paste', '--type', 'text/plain;charset=utf-8', '--no-newline'], env=env, capture_output=True, timeout=5)
        return result.stdout if result.returncode == 0 else None

    def copy(expected):
        sentinel = b'history-copy-sentinel'
        subprocess.run(['wl-copy', '--type', 'text/plain;charset=utf-8'], input=sentinel, env=env, check=True, timeout=5)
        wait_until(lambda: copied() == sentinel)
        key(30, 5)  # Harness custom Ctrl+Shift+A.
        wait_until(lambda: copied() == expected.encode())

    click(1, start + 1)
    wait_until(lambda: re.findall(pattern, (root / 'app.log').read_text()))
    line, _, _, _, cw, ch, origin = map(int, re.findall(pattern, (root / 'app.log').read_text())[-1])
    assert line < 0 and origin == start, 'pointer must target actual negative history rows'
    time.sleep(0.45)
    click(2*cw + cw//2, origin + ch//2)
    click(2*cw + cw//2, origin + ch//2)
    copy('中')  # A click on the wide spacer must select the complete historical glyph.
    time.sleep(0.45)
    for _ in range(3):
        click(cw//2, origin + ch//2)
    columns = int((root / 'history-initial').read_text())
    expected = 'A中e\u0301 ' * 24 + ' ' * (-120 % columns)
    copy(expected)  # No soft-wrap newline, no combining loss, exact retained spaces.
    keyboard.send(pointer, 1, u32(0, 1, 1, 800, 600))
    keyboard.send(pointer, 4)
    keyboard.sync()
    rows = (120 + columns - 1) // columns
    def highlighted():
        actual = capture()
        return all(
            actual[r][:columns*cw*3] == bytes(byte ^ 255 for byte in baseline[r][:columns*cw*3])
            for r in range(origin, origin + rows*ch)
        )
    wait_until(highlighted)
    key(107, 1)
    copy(expected)  # A settled range remains attached to its offscreen historical cells.
    key(102, top_mask)
    wait_until(highlighted)
    print('PASS: native negative-row hit test, CJK spacer Copy, exact soft-wrap/combining/spaces, history highlight and offscreen settled-range Copy')
