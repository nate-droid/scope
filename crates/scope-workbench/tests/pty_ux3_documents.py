#!/usr/bin/env python3
"""Bounded real-launcher UX3 PTY evidence (stdlib; no live agent).

Run from any directory: python3 crates/scope-workbench/tests/pty_ux3_documents.py
Build first if needed: cargo build --locked --offline -p scope-workbench --bin scope
"""
import fcntl
import os
from pathlib import Path
import pty
import re
import signal
import struct
import tempfile
import termios
import time

from pty_workspace import Screen, Workspace

ROOT = Path(__file__).resolve().parents[3]


class CargoWorkspace(Workspace):
    """Keep the shared VT/input implementation; launch real cargo from repo root."""
    def __init__(self, writable=False, agent_flags=None):
        self.workspace = tempfile.TemporaryDirectory(prefix='scope-ux3-')
        self.screen = Screen()
        for name in ('existing.scope', 'copy.scope'):
            (Path(self.workspace.name) / name).write_text('41', encoding='utf-8')
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ['TERM'] = 'xterm-256color'
            os.environ['XDG_STATE_HOME'] = self.workspace.name
            os.environ.pop('SCOPE_HISTORY_FILE', None)
            os.chdir(ROOT)
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', 44, 180, 0, 0))
            args = ['cargo', 'run', '--locked']
            if agent_flags is not None:
                args += ['--features', 'agent', '--', *agent_flags]
                if writable:
                    args += ['--writable']
                args += ['workspace', self.workspace.name]
            elif writable:
                args += ['--', '--writable', 'workspace', self.workspace.name]
            os.execvp('cargo', args)
        self.query_pending = b''
        self.wire = bytearray()
        self.commands = self.actions = 0


def require(screen, text):
    assert text in screen, f'Missing {text!r}:\n{screen}'


def ready(ui):
    deadline = time.monotonic() + 30
    screen = ''
    while 'F4 command' not in screen and time.monotonic() < deadline:
        screen = ui.read()
    require(screen, 'F4 command')


def prefill(ui, action, command):
    screen = ui.action(action)
    require(screen, command)
    print(f'EVIDENCE F1 {action}: {command!r}', flush=True)


def transcript(ui):
    ui.action('Focus output')
    frames = [ui.send(b'\x1b[5~' * 30)]
    for _ in range(45):
        before = ui.screen.text()
        frames.append(ui.send(b'\x1b[B'))
        if frames[-1] == before:
            break
    return '\n'.join(frames)


def main():
    def timeout(*_):
        raise TimeoutError('UX3 150-second deadline')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(150)
    ui = CargoWorkspace()
    try:
        ready(ui)
        print('EVIDENCE repository-root cargo run: retained workbench ready', flush=True)
        ui.action('New Script')
        source_id = re.search(r'Selected scratch (\d+)', ui.screen.text()).group(1)
        ui.paste('print("ux3-run"); 42')
        ui.send(b'\x13')
        ui.send(b'\x1b[15~')
        output = transcript(ui)
        require(output, 'status: interpreter success')
        require(output, 'final: 42')
        print('EVIDENCE create/type/Ctrl-S/F5: interpreter success; final: 42', flush=True)
        ui.action(f'Switch Document: Untitled Script [{source_id}]')
        ui.paste(' + 7')
        ui.action('New Script')
        screen = ui.action(f'Switch Document: Untitled Script [{source_id}]')
        require(screen, ' + 7')
        print('EVIDENCE dirty draft survived New Script and per-source Switch Document return', flush=True)
        ui.send(b'\x13')
        for action, command in [('Open File', ':scope open '),
                                ('Save committed document', ':scope save'),
                                ('Save As', ':scope save-as ')]:
            prefill(ui, action, command)
            ui.send(b'\x1b')
        for action, command in [('Ask Agent', ':scope document-ask '),
                                ('Propose Edit', ':scope document-propose ')]:
            prefill(ui, action, command)
            ui.send(b'explain active source\r')
            output = transcript(ui)
            # The default launcher has no compiled-in agent support.
            require(output, 'agent support is disabled in this binary')
            print('EVIDENCE unavailable agent diagnostic: ' + '\n'.join(
                line.strip() for line in output.splitlines()
                if 'agent support is disabled' in line.lower())[-1800:], flush=True)
        ui.send(b'\x11')
    finally:
        ui.close()
    ui = CargoWorkspace(writable=True)
    try:
        ready(ui)
        prefill(ui, 'Open File', ':scope open ')
        ui.send(b'existing.scope\r')
        require(ui.screen.text(), '41')
        ui.action('Focus editor')
        ui.send(b'\x1b[H')
        ui.send(b'\x1b[3~' * 2)
        ui.paste('42')
        ui.send(b'\x13')
        path = Path(ui.workspace.name) / 'existing.scope'
        assert path.read_text() == '41', 'Ctrl-S unexpectedly wrote disk'
        prefill(ui, 'Save committed document', ':scope save')
        ui.send(b'\r')
        assert path.read_text() == '42', ui.screen.text()
        print('EVIDENCE --writable: Ctrl-S left disk at 41; explicit Save wrote 42', flush=True)
        prefill(ui, 'Save As', ':scope save-as ')
        ui.send(b'copy.scope\r')
        assert (path.parent / 'copy.scope').read_text() == '42', ui.screen.text()
        print('EVIDENCE Save As wrote existing authorized copy.scope: 42', flush=True)
        ui.send(b'\x11')
    finally:
        ui.close()
        signal.alarm(0)
    print('PASS: UX3 document PTY evidence; no live agent invoked', flush=True)


if __name__ == '__main__':
    main()
