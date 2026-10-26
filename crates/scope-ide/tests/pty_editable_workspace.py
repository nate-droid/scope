#!/usr/bin/env python3
"""Bounded real-PTY integration for the caller-owned editable workspace.

Build: cargo build -p scope-ide --features tui --example editable_workspace
Run: python3 crates/scope-ide/tests/pty_editable_workspace.py
Standard library only. CSI stripping is NOT a screen emulator: TUI assertions
below concern emitted text/mode sequences, never cell positions or visibility.
Exact source assertions use the short, untruncated :scope show preview.
"""
import argparse
import errno
import fcntl
import os
from pathlib import Path
import pty
import pickle
import re
import select
import signal
import struct
import tempfile
import termios
import time
import unittest

BINARY = Path(__file__).resolve().parents[3] / 'target/debug/examples/editable_workspace'
ANSI = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]')


class Workspace:
    def __init__(self, binary):
        self.temp = tempfile.TemporaryDirectory(prefix='scope-editable-pty-')
        self.fd, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ,
                    struct.pack('HHHH', 50, 220, 0, 0))
        self.original = termios.tcgetattr(self.slave)
        self.raw = bytearray()
        self.pending = b''
        self.status = None
        self.deadline = time.monotonic() + 90
        self.restore_read, restore_write = os.pipe()
        self.pid = os.fork()
        if self.pid == 0:
            try:
                os.setsid()
                fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)
                # Keep the session leader alive until attributes are sampled:
                # macOS revokes the slave when the leader exits.
                worker = os.fork()
                if worker:
                    os.close(self.restore_read)
                    _, status = os.waitpid(worker, 0)
                    os.write(restore_write, pickle.dumps(termios.tcgetattr(self.slave)))
                    os._exit(os.waitstatus_to_exitcode(status) if os.WIFEXITED(status) else 1)
                os.close(restore_write)
                os.close(self.restore_read)
                for target in (0, 1, 2):
                    os.dup2(self.slave, target)
                os.close(self.fd)
                os.close(self.slave)
                os.environ['TERM'] = 'xterm-256color'
                os.environ['XDG_STATE_HOME'] = self.temp.name
                os.environ.pop('SCOPE_HISTORY_FILE', None)
                os.execv(str(binary), [str(binary)])
            except BaseException:
                os._exit(127)
        os.close(restore_write)

    def read(self, timeout=4):
        data = bytearray()
        end = min(time.monotonic() + timeout, self.deadline)
        quiet = time.monotonic() + .3
        while time.monotonic() < end:
            if not select.select([self.fd], [], [], .05)[0]:
                if time.monotonic() >= quiet:
                    break
                continue
            try:
                chunk = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            data.extend(chunk)
            self.raw.extend(chunk)
            quiet = time.monotonic() + .3
            # As in pty_shell.py, answer cursor queries even across read chunks.
            self.pending += chunk
            while b'\x1b[6n' in self.pending:
                _, self.pending = self.pending.split(b'\x1b[6n', 1)
                os.write(self.fd, b'\x1b[1;1R')
            self.pending = self.pending[-3:]
        return ANSI.sub(b'', bytes(data)).decode('utf-8', errors='replace').replace('\r', '')

    def expect(self, needle, initial=''):
        output = initial
        end = min(time.monotonic() + 6, self.deadline)
        while needle not in output and time.monotonic() < end:
            output += self.read(timeout=min(1, end - time.monotonic()))
        if needle not in output:
            raise AssertionError(f'missing {needle!r} in PTY output: {output!r}')
        return output

    def send(self, keys):
        if time.monotonic() >= self.deadline:
            raise AssertionError('90-second scenario deadline exceeded')
        os.write(self.fd, keys)
        return self.read()

    def command(self, command):
        return self.expect('scope> ', self.send(command.encode() + b'\r'))

    def wait_exit(self, timeout=5):
        end = min(time.monotonic() + timeout, self.deadline)
        while time.monotonic() < end:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.status = status
                return os.waitstatus_to_exitcode(status)
            time.sleep(.05)
        raise AssertionError('workspace did not exit within bounded timeout')

    def close(self):
        if self.status is None:
            try:
                os.killpg(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            end = time.monotonic() + 3
            while time.monotonic() < end:
                pid, status = os.waitpid(self.pid, os.WNOHANG)
                if pid:
                    self.status = status
                    break
                time.sleep(.05)
        os.close(self.fd)
        os.close(self.slave)
        os.close(self.restore_read)
        self.temp.cleanup()


class EditableWorkspaceTests(unittest.TestCase):
    def test_roundtrip_and_stale_draft(self):
        workspace = Workspace(BINARY)
        self.addCleanup(workspace.close)
        workspace.expect('Scratcheditor')
        self.assertIn(b'\x1b[?1049h', workspace.raw)

        def show(expected, revision):
            output = workspace.command(':scope show')
            identity = re.search(r'Shell buffer target: Scratch\((\d+)\) @(\d+) name=scratch-(\d+) destination=\[unbound\]', output)
            self.assertIsNotNone(identity, repr(output))
            self.assertEqual(identity.groups(), ('0', str(revision), '0'))
            rows = re.findall(r'^\s*(\d+) \| (.*)$', output, re.MULTILINE)
            self.assertEqual(rows, [(str(i), line) for i, line in enumerate(expected.split('\n'), 1)], repr(output))
            print(f'\nEVIDENCE revision {revision}: {identity.group(0)}; rows={rows!r}', flush=True)

        initial = 'Edit this unsaved text.\nUnicode: 猫 é\n'
        # Draft starts at offset zero; use real keyboard insertion and Ctrl-S.
        workspace.send(b'TUI-')
        # F6 moves to the read-only observer. F5 is host-authorized there,
        # not in the editor; returning must retain the uncommitted draft.
        workspace.send(b'\x1b[17~')
        report = workspace.send(b'\x1b[15~')
        # Ratatui emits cell diffs, so unchanged letters may be omitted.
        # Assert newly emitted result data, not a reconstructed screen label.
        self.assertIn('RevisionId(0):37chars,2lines', report.replace(' ', ''))
        self.assertIn('Reportsretained:1', report.replace(' ', ''))
        workspace.send(b'\x1b[17~')
        workspace.send(b'\x13')
        workspace.expect('scope> ', workspace.send(b'\x1b'))
        text = 'TUI-' + initial
        show(text, 1)

        # Normal keyboard editor, deliberately not --commands.
        editor = workspace.send(b':scope edit\r')
        self.assertNotIn('Editing draft; :help', editor)
        workspace.send(b'SHELL-')
        workspace.expect('Returned to REPL', workspace.send(b'\x13'))
        text = 'SHELL-' + text
        show(text, 2)
        returned = workspace.send(b':scope quit\r')
        workspace.expect('Scratcheditor', returned)
        # A fresh full TUI redraw emits the updated committed source. This is
        # stream evidence only, not a reconstructed observer-pane assertion.
        self.assertIn('SHELL-TUI-Editthisunsavedtext.', returned)

        # Repeat using F2, then preserve a local draft across a competing edit.
        workspace.send(b'STALE-')
        workspace.expect('scope> ', workspace.send(b'\x1bOQ'))
        show(text, 2)  # uncommitted TUI draft has not changed the source
        workspace.command(':scope line 1 "changed in the shell"')
        text = 'changed in the shell\nUnicode: 猫 é\n'
        show(text, 3)
        workspace.expect('Scratcheditor', workspace.send(b':scope quit\r'))
        workspace.send(b'\x13')  # stale commit must not overwrite shell edit
        workspace.expect('scope> ', workspace.send(b'\x1b'))
        show(text, 3)
        workspace.expect('Scratcheditor', workspace.send(b':scope quit\r'))
        workspace.send(b'\x03')  # cancel the retained stale draft
        workspace.send(b'FRESH-')
        workspace.send(b'\x13')
        workspace.expect('scope> ', workspace.send(b'\x1bOQ'))
        text = 'FRESH-' + text
        show(text, 4)  # cancellation permits a fresh baseline and commit
        workspace.expect('Scratcheditor', workspace.send(b':scope quit\r'))

        final = workspace.send(b'\x11')
        final += workspace.read()
        self.assertEqual(workspace.wait_exit(), 0)
        self.assertIn('Retained binding after workspace exit: Available', final)
        self.assertIn('FRESH-changed in the shell', final)
        self.assertNotIn('STALE-', final)
        self.assertTrue(select.select([workspace.restore_read], [], [], 1)[0],
                        'supervisor did not report restored terminal attributes')
        restored = pickle.loads(os.read(workspace.restore_read, 65536))
        self.assertEqual(restored, workspace.original)
        self.assertIn(b'\x1b[?1049l', workspace.raw)
        self.assertIn(b'\x1b[?25h', workspace.raw)
        print('EVIDENCE Ctrl-Q: exit=0; retained binding contains final text; '
              'termios exactly restored; alternate-screen leave and cursor-show emitted', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=BINARY)
    args, rest = parser.parse_known_args()
    BINARY = args.binary.resolve()
    if not BINARY.is_file():
        parser.error(f'build the example first: missing {BINARY}')
    unittest.main(argv=[__file__] + rest)
