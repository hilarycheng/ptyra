"""Exact native core replies through real raw PTYs, including an unfocused hidden Tab."""
import time


def check_replies(root, keyboard, app, wait_until, ipc):
    log_start = (root / 'app.log').stat().st_size
    def key(code, mask=0):
        keyboard.modifiers(mask)
        keyboard.tap(code)
        keyboard.modifiers(0)
        keyboard.sync()

    for number, row, column in [(1, 3, 7), (2, 2, 9)]:
        prefix = root / f"replies-{number}"
        script = prefix.with_suffix(".py")
        query = (b"\x1b[c\x1b[5n\x1b[=31u\x1b[?u\x1b]4;1;?\x07\x1b[?u"
                 + f"\x1b[{row};{column}H\x1b[6n".encode()
                 + b"\x1b[?2026$p\x1b[?u")
        expected = (b"\x1b[?6c\x1b[0n\x1b[?0u\x1b]4;1;rgb:cccc/0000/0000\x07\x1b[?0u"
                    + f"\x1b[{row};{column}R".encode()
                    + b"\x1b[?2026;2$y\x1b[?0u")
        script.write_text(
            "import os,select,termios,time,tty\nfrom pathlib import Path\n"
            f"prefix=Path({str(prefix)!r})\n"
            "old=termios.tcgetattr(0)\ntty.setraw(0)\n"
            "try:\n"
            " prefix.with_suffix('.ready').write_text('ready')\n"
            " end=time.monotonic()+10\n"
            " while not prefix.with_suffix('.go').exists():\n"
            "  assert time.monotonic()<end, 'gate timeout'\n"
            "  time.sleep(0.01)\n"
            # Split sequences across writes, then a burst larger than kernel input capacity.
            f" query={query!r}\n expected={expected!r}+b'\\x1b[?0u'*4096\n"
            " for byte in query:\n  os.write(1,bytes([byte])); time.sleep(0.002)\n"
            " os.write(1,b'\\x1b[?u'*4096)\n time.sleep(0.15)\n"
            " received=bytearray()\n end=time.monotonic()+6\n"
            " while len(received)<len(expected) and time.monotonic()<end:\n"
            "  if select.select([0],[],[],0.1)[0]: received.extend(os.read(0,65536))\n"
            " assert bytes(received)==expected, (len(received),len(expected),bytes(received[:64]))\n"
            " assert not select.select([0],[],[],0.2)[0], 'duplicate or cross-Tab reply'\n"
            " prefix.with_suffix('.done').write_text('done')\n"
            "finally:\n termios.tcsetattr(0,termios.TCSANOW,old)\n"
        )
        keyboard.text('python3 ' + str(script))
        key(28)
        wait_until(prefix.with_suffix('.ready').exists)
        if number == 1:
            key(20, 5)  # Ctrl+Shift+T: original raw reader becomes hidden.
            time.sleep(0.2)
    # Neither source has keyboard focus when its queries are emitted.
    ipc('workspace number 2')
    time.sleep(0.1)
    for number in [1, 2]:
        (root / f'replies-{number}.go').write_text('go')
    for number in [1, 2]:
        wait_until((root / f'replies-{number}.done').exists, seconds=8)
    assert app.poll() is None, (root / 'app.log').read_text()
    assert b'reply rejected' not in (root / 'app.log').read_bytes()[log_start:]
    ipc('workspace number 1')
    time.sleep(0.1)
    key(17, 5)  # Close Tab 2; leave Tab 1's Shell for the remaining native suite.
    time.sleep(0.1)
    print('PASS: native split mixed DA/DSR/OSC/sync/Kitty queries and 4096-query bursts return exact ordered zero flags once to both original unfocused/hidden PTYs')
