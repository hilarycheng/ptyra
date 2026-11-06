"""Owned-headless-Sway history pixels, including CJK, reflow and offscreen cursor."""
import json
import os
import subprocess
import time


def check_history(root, env, keyboard, app, wait_until, ipc, nodes):
    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    def capture():
        node = next(n for n in nodes(json.loads(ipc('-t', 'get_tree'))) if n.get('app_id') == 'ptyra')
        rect, window = node['rect'], node['window_rect']
        ppm = subprocess.check_output(['grim', '-t', 'ppm', '-'], env=env)
        _, dimensions, _, pixels = ppm.split(b'\n', 3)
        width, height = map(int, dimensions.split())
        x, y = rect['x'] + window['x'], rect['y'] + window['y']
        w, h = rect['width'] - 2 * window['x'], rect['height'] - window['y'] - window['x']
        assert 0 <= x < x+w <= width and 0 <= y < y+h <= height, (x, y, w, h, width, height, node)
        return [pixels[(r*width+x)*3:(r*width+x+w)*3] for r in range(y, y+h)]

    def band(rows):
        colored = [i for i, row in enumerate(rows) if b'\xcc\0\0' * 3 in row or b'\x4e\x9a\x06' * 3 in row]
        assert colored, 'fixture color rows missing'
        return colored[0], colored[-1] + 1

    ipc('[app_id="ptyra"] border none')
    ipc('[app_id="ptyra"] floating enable')
    time.sleep(0.3)  # Let Sway finish the floating configure before the size request.
    ipc('[app_id="ptyra"] resize set width 640 px height 400 px')
    time.sleep(0.3)
    ipc('[app_id="ptyra"] move position center')
    wait_until(lambda: len(capture()) == 400 and len(capture()[0]) == 640 * 3)
    script = root / 'history-probe.py'
    payload = ('\x1b[0m\x1b[2J\x1b[3J\x1b[H\x1b[?25l\x1b[41m' + 'A中e\u0301 ' * 24 + '\r\n\x1b[42mHistory 終點\x1b[0m').encode()
    script.write_text('''import os,time
from pathlib import Path
root=Path(''' + repr(str(root)) + ''')
def gate(name):
 while not (root/name).exists(): time.sleep(0.01)
def done(name): (root/name).write_text(str(os.get_terminal_size().columns))
os.write(1,''' + repr(payload) + ''')
done('history-initial')
gate('history-scroll')
os.write(1,b'\\r\\nnew live row'*80)
done('history-output')
gate('history-cursor')
os.write(1,b'\\x1b[?25h')
done('history-cursor-done')
gate('history-reference')
os.write(1,''' + repr(payload) + ''')
done('history-reference-done')
gate('history-exit')
os.write(1,b'\\x1b[0m\\x1b[?25h\\r\\n')
''')
    keyboard.text('python3 ' + str(script))
    key(28)
    wait_until((root / 'history-initial').exists)
    time.sleep(0.2)
    reference = capture()
    start, end = band(reference)
    assert end - start >= 3 * 10, 'soft-wrap fixture needs multiple visible rows'
    (root / 'history-scroll').touch()
    wait_until((root / 'history-output').exists)
    time.sleep(0.2)
    bottom = capture()
    key(102, 8 if os.environ.get('PTYRA_TEST_SCROLL') else 1)  # Honor the optional scroll suite's custom Alt+Home binding.
    wait_until(lambda: capture()[start:end] == reference[start:end])
    historical = capture()
    (root / 'history-cursor').touch()
    wait_until((root / 'history-cursor-done').exists)
    time.sleep(0.2)
    assert capture() == historical, 'offscreen live cursor leaked into history'
    key(107, 1)
    time.sleep(0.15)
    # Cursor was explicitly enabled, so compare all rows except its final live row.
    assert capture()[:-30] == bottom[:-30], 'scroll round trip left stale pixels'
    key(102, 8 if os.environ.get('PTYRA_TEST_SCROLL') else 1)
    ipc('[app_id="ptyra"] resize set width 480 px height 400 px')
    ipc('[app_id="ptyra"] move position center')
    wait_until(lambda: len(capture()) == 400 and len(capture()[0]) == 480 * 3)
    time.sleep(0.4)
    key(102, 8 if os.environ.get('PTYRA_TEST_SCROLL') else 1)
    wait_until(lambda: any(b'\xcc\0\0' * 3 in row for row in capture()))
    history_reflow = capture()
    if os.environ.get('PTYRA_TEST_HISTORY_SCREENSHOT'):
        subprocess.run(['grim', os.environ['PTYRA_TEST_HISTORY_SCREENSHOT']], env=env, check=True)
    start, end = band(history_reflow)
    key(107, 1)
    (root / 'history-reference').touch()
    wait_until((root / 'history-reference-done').exists)
    assert int((root / 'history-reference-done').read_text()) < int((root / 'history-initial').read_text()), 'native resize did not change columns'
    wait_until(lambda: capture()[start:end] == history_reflow[start:end])
    (root / 'history-exit').touch()
    time.sleep(0.2)
    ipc('[app_id="ptyra"] floating disable')
    time.sleep(0.2)
    assert app.poll() is None
    print('PASS: native history glyph/background rows, CJK/combining soft wrap, offscreen cursor, resize/reflow and clean scroll round trip pixels')
