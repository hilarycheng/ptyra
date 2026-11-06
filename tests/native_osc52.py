"""OSC 52 native regression inside the owned headless compositor fixture."""
import base64
import os
import subprocess
import time


def check_osc52(root, env, keyboard, app, wait_until):
    enabled = os.environ['PTYRA_TEST_OSC52'] == 'enabled'
    primary = not os.environ.get('PTYRA_TEST_NO_PRIMARY')
    log = lambda: (root / 'app.log').read_text()
    if enabled:
        wait_until(lambda: any(reason in log() for reason in ['Ptyra OSC 52: no valid input serial', 'Ptyra OSC 52: source is not focused']))
    kinds = [('c', False), ('p', True)] if primary else [('c', False)]
    for _, is_primary in kinds:
        subprocess.run(['wl-copy'] + (['--primary'] if is_primary else []) + ['--type', 'text/plain;charset=utf-8', 'external'], env=env, check=True, timeout=5)
    def read(is_primary):
        result = subprocess.run(['wl-paste', '--no-newline'] + (['--primary'] if is_primary else []), env=env, capture_output=True, timeout=5)
        return result.stdout if result.returncode == 0 else None
    for _, is_primary in kinds:
        wait_until(lambda: read(is_primary) == b'external')
    def sequence(kind, payload):
        return b'\x1b]52;' + kind.encode() + b';' + base64.b64encode(payload) + b'\x07'
    def run_script(data, after=''):
        script = root / 'osc-probe.py'
        script.write_text('import os,time\nfrom pathlib import Path\nos.write(1, '+repr(data)+')\n'+after)
        keyboard.text('python3 ' + str(script))
        keyboard.tap(28)
        keyboard.sync()
    payloads = {'c': 'OSC中 \n'.encode(), 'p': 'PRIMARY繁\n'.encode()}
    gate = root / 'osc-gate'
    done = root / 'osc-done'
    data = b''.join(sequence(kind, payloads[kind]) for kind in ['c', 'p'])
    run_script(data, f'while not Path({str(gate)!r}).exists(): time.sleep(0.01)\nos.write(1, {sequence("c", b"late-secret")!r})\nPath({str(done)!r}).write_text("done")\n')
    for kind, is_primary in kinds:
        wait_until(lambda: read(is_primary) == (payloads[kind] if enabled else b'external'))
    before = log().count('Ptyra OSC 52: no valid input serial')
    gate.write_text('go')
    wait_until(done.exists)
    if enabled:
        wait_until(lambda: log().count('Ptyra OSC 52: no valid input serial') > before)
    time.sleep(0.1)
    assert read(False) == (payloads['c'] if enabled else b'external')
    if enabled and not primary:
        assert 'Ptyra OSC 52: selection unavailable' in log()
    # Query never requests the clipboard or writes any reply into the PTY.
    query = root / 'osc-query.py'
    result = root / 'osc-query-result'
    query.write_text('import os,select,termios,tty\nfrom pathlib import Path\nold=termios.tcgetattr(0)\ntry:\n tty.setraw(0)\n os.write(1,b"\\x1b]52;c;?\\x07\\x1b]52;p;?\\x07")\n ready=select.select([0],[],[],0.25)[0]\n Path('+repr(str(result))+').write_text("reply" if ready else "denied")\nfinally: termios.tcsetattr(0,termios.TCSANOW,old)\n')
    keyboard.text('python3 '+str(query)); keyboard.tap(28); keyboard.sync()
    wait_until(result.exists)
    assert result.read_text() == 'denied'
    run_script(sequence('c', b'recovery'))
    wait_until(lambda: read(False) == (b'recovery' if enabled else b'external'))
    if enabled:
        before = log().count('Ptyra OSC 52: write exceeds clipboard limit')
        run_script(sequence('c', b'x' * 17))
        wait_until(lambda: log().count('Ptyra OSC 52: write exceeds clipboard limit') > before)
        assert read(False) == b'recovery'
        # Hidden output is rejected, and a newly created Tab inherits opt-in.
        ready = root / 'osc-hidden-ready'
        hidden_gate = root / 'osc-hidden-gate'
        hidden_done = root / 'osc-hidden-done'
        run_script(b'', f'Path({str(ready)!r}).write_text("ready")\nwhile not Path({str(hidden_gate)!r}).exists(): time.sleep(0.01)\nos.write(1, {sequence("c", b"hidden-secret")!r})\nPath({str(hidden_done)!r}).write_text("done")\n')
        wait_until(ready.exists)
        keyboard.modifiers(5); keyboard.tap(20); keyboard.modifiers(0); keyboard.sync()
        time.sleep(0.2)
        before = log().count('Ptyra OSC 52: source is not focused')
        hidden_gate.write_text('go')
        wait_until(hidden_done.exists)
        wait_until(lambda: log().count('Ptyra OSC 52: source is not focused') > before)
        assert read(False) == b'recovery'
        run_script(sequence('c', b'new-tab'))
        wait_until(lambda: read(False) == b'new-tab')
        keyboard.modifiers(5); keyboard.tap(17); keyboard.modifiers(0); keyboard.sync()
        time.sleep(0.1)
        run_script(sequence('c', b'recovery'))
        wait_until(lambda: read(False) == b'recovery')
    time.sleep(0.1)
    assert app.poll() is None, log()
    assert 'hidden-secret' not in log() and 'late-secret' not in log() and 'OSC中' not in log() and 'PRIMARY繁' not in log()
    print('PASS: native OSC52 '+('enabled' if enabled else 'default-denied')+'; serial consumption, independent selection, read denial and recovery'+(' without Primary protocol' if not primary else ''))
