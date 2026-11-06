"""Native input routing/offset regression; history pixels are verified separately by T158."""
import re
import time


def check_scroll(root, env, keyboard, app, wait_until, u32):
    def offsets():
        return [(int(tab), int(offset)) for tab, offset in re.findall(r'Ptyra viewport: tab=(\d+) offset=(\d+)', (root / 'app.log').read_text())]
    def key(code, mask=0):
        keyboard.modifiers(mask); keyboard.tap(code); keyboard.modifiers(0); keyboard.sync()
    def changed(action, target=1, expected=None):
        before = len(offsets()); action()
        wait_until(lambda: len(offsets()) > before)
        tab, offset = offsets()[-1]
        assert tab == target, (tab, target)
        if expected is not None: assert offset == expected, (offset, expected)
        return offset
    ready, gate, done = (root / name for name in ['scroll-ready', 'scroll-gate', 'scroll-done'])
    script = root / 'scroll-probe.py'
    script.write_text('import os,time\nfrom pathlib import Path\nos.write(1,b"line\\r\\n"*100)\nPath('+repr(str(ready))+').write_text("ready")\nwhile not Path('+repr(str(gate))+').exists(): time.sleep(0.01)\nos.write(1,b"later\\r\\n")\nPath('+repr(str(done))+').write_text("done")\n')
    keyboard.text('python3 '+str(script)); key(28)
    wait_until(ready.exists); time.sleep(0.1)
    manager = keyboard.bind('zwlr_virtual_pointer_manager_v1')
    pointer = keyboard.allocate(); keyboard.send(manager, 0, u32(0, pointer))
    keyboard.send(pointer, 1, u32(0, 150, 80, 800, 600)); keyboard.send(pointer, 4); keyboard.sync(); time.sleep(0.1)
    def wheel(value, axis=0):
        keyboard.send(pointer, 3, u32(0, axis, int(value*256) & 0xffffffff)); keyboard.send(pointer, 4); keyboard.sync()
    changed(lambda: wheel(-30), expected=3)
    assert changed(lambda: key(104, 1)) > 3
    changed(lambda: key(109, 1), expected=3)
    assert changed(lambda: key(102, 8)) > 3  # Custom Alt+Home, not default Shift+Home.
    changed(lambda: key(107, 1), expected=0)
    before = len(offsets()); wheel(-5); time.sleep(0.1); assert len(offsets()) == before
    changed(lambda: wheel(-5), expected=1)
    before = len(offsets()); wheel(-100, 1); time.sleep(0.1); assert len(offsets()) == before
    # Output while browsing stays above bottom; accepted text input then follows bottom.
    gate.write_text('go'); wait_until(done.exists); time.sleep(0.1)
    changed(lambda: keyboard.text('a'), expected=0); key(14)
    # New/switch actions preserve the other Tab's viewport and do not emit PTY text.
    assert changed(lambda: key(104, 1)) > 0
    key(20, 5); time.sleep(0.2)
    before = len(offsets()); wheel(-10); time.sleep(0.1); assert len(offsets()) == before
    key(104, 4); time.sleep(0.1)
    changed(lambda: key(107, 1), expected=0)
    key(109, 4); time.sleep(0.1); key(17, 5); time.sleep(0.1)
    # An application-owned mouse mode reserves wheel events; explicit history keys remain usable.
    mode = root / 'scroll-mode.py'
    mode.write_text('import os\nos.write(1,b"\\x1b[?1000h")\n')
    keyboard.text('python3 '+str(mode)); key(28); time.sleep(0.2)
    before = len(offsets()); wheel(-100); time.sleep(0.1); assert len(offsets()) == before
    assert changed(lambda: key(104, 1)) > 0
    mode.write_text('import os\nos.write(1,b"\\x1b[?1000l")\n')
    changed(lambda: keyboard.text('python3 '+str(mode)), expected=0); key(28); time.sleep(0.2)
    changed(lambda: wheel(-10), expected=1)
    changed(lambda: key(107, 1), expected=0)
    assert app.poll() is None
    print('PASS: native wheel/fractional/custom+default keys, active-Tab isolation, output preservation, keyboard follow-bottom and mouse ownership; history pixels are T158')
