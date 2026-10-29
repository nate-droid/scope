#!/usr/bin/env python3
"""Focused normal-launcher UX2 evidence; reuses the existing stdlib PTY helper.

Build: cargo build --locked --offline -p scope-workbench --bin scope
Run: python3 crates/scope-workbench/tests/pty_ux2_editor_evidence.py
Assertions use short ASCII source previews after committing. This is automated
xterm-sequence evidence, not human testing or general terminal compatibility.
"""
from pathlib import Path
import signal
import time

from pty_workspace import Workspace


def main():
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('90-second deadline')))
    signal.alarm(90)
    binary = Path(__file__).resolve().parents[3] / 'target/debug/scope'
    ui = Workspace(binary)
    try:
        screen = ui.read()
        deadline = time.monotonic() + 10
        while 'F4 command' not in screen and time.monotonic() < deadline:
            screen = ui.read()
        assert 'F4 command' in screen, screen

        def check(text, revision):
            ui.send(b'\x13')  # publish, then inspect through the normal shell
            ui.send(b'\x1bOQ')  # F2
            screen = ui.send(b':scope show\r')
            assert f'@{revision} ' in screen, screen
            assert f'1 | {text}' in screen, screen
            print(f'EVIDENCE committed preview: revision={revision}, text={text!r}', flush=True)
            ui.send(b':scope quit\r')

        ui.paste('abc')
        ui.send(b'\x1b[1;2H')  # Shift-Home
        ui.send(b'X')  # selected abc replaced
        screen = ui.send(b'\x1a')  # undo replacement
        assert 'abc' in screen, screen
        screen = ui.send(b'\x19')  # redo replacement
        assert 'abc' not in screen, screen
        check('X', 1)

        ui.send(b'\x1b[H')
        ui.send(b'\x1b[1;2F')  # Shift-End
        ui.paste('YZ')
        check('YZ', 2)

        ui.send(b'\x1b[1;2D')  # Shift-Left selects Z
        ui.send(b'\x7f')  # Backspace removes selection
        check('Y', 3)
        ui.send(b'\x1b[H')
        ui.send(b'\x1b[1;2C')  # Shift-Right selects Y
        ui.send(b'\x1b[3~')  # Delete selection
        ui.paste('A' + 'e\u0301' + 'B')
        ui.send(b'\x1b[D')  # before B
        ui.send(b'\x7f')  # delete entire combining grapheme
        check('AB', 4)

        ui.send(b'\x1b[H')
        ui.paste('👩\u200d💻')
        ui.send(b'\x1b[D')  # traverse entire joined emoji
        ui.send(b'\x1b[3~')  # delete entire joined emoji
        check('AB', 4)  # unchanged committed text: no new revision
        print('PASS: normal scope workspace PTY; selection replacement, undo/redo, '
              'selection deletion, combining/ZWJ grapheme movement/deletion.', flush=True)
        ui.send(b'\x11')
    finally:
        ui.close()
        signal.alarm(0)


if __name__ == '__main__':
    main()
