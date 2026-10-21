#!/usr/bin/env python3
"""Actual-binary PTY checks; run after cargo build -p scope-ide.

Uses only the Python standard library. Pass --binary for another build.
"""
import argparse
import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import tempfile
import termios
import time
import unittest

BINARY = Path(__file__).resolve().parents[3] / 'target/debug/scope-ide'
ANSI = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]')


class Shell:
    def __init__(self):
        self.workspace = tempfile.TemporaryDirectory(prefix='scope-pty-')
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ['TERM'] = 'xterm-256color'
            os.environ['XDG_STATE_HOME'] = self.workspace.name
            os.environ.pop('SCOPE_HISTORY_FILE', None)
            os.chdir(self.workspace.name)
            os.execv(str(BINARY), [str(BINARY)])
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
        self.pending = b''
        startup = ''
        deadline = time.monotonic() + 10
        while 'scope> ' not in startup:
            startup += self.read()
            if time.monotonic() > deadline:
                self.close()
                raise RuntimeError(f'shell did not become ready: {startup!r}')

    def read(self):
        data = b''
        deadline = time.monotonic() + 5
        quiet = time.monotonic() + .25
        while time.monotonic() < deadline:
            if not select.select([self.fd], [], [], .05)[0]:
                if time.monotonic() > quiet:
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
            data += chunk
            quiet = time.monotonic() + .25
            # Queries can straddle reads. Answer every crossterm cursor query.
            self.pending += chunk
            while b'\x1b[6n' in self.pending:
                before, self.pending = self.pending.split(b'\x1b[6n', 1)
                os.write(self.fd, b'\x1b[1;1R')
            self.pending = self.pending[-3:]
        return ANSI.sub(b'', data).decode('utf-8', errors='replace').replace('\r', '')

    def send(self, keys):
        os.write(self.fd, keys)
        return self.read()

    def close(self):
        if self.pid > 0:
            try:
                os.kill(self.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            os.waitpid(self.pid, 0)
        os.close(self.fd)
        self.workspace.cleanup()


class ShellTests(unittest.TestCase):
    def setUp(self):
        self.shell = Shell()
        self.addCleanup(self.shell.close)

    def submitted(self, keys, expected):
        output = self.shell.send(keys + b'\r')
        self.assertIn(expected, output, repr(output))
        return output

    def test_cancel_completion_draft(self):
        self.assertIn('[1/', self.shell.send(b's\t'))
        self.assertIn('^C', self.shell.send(b'\x03'))
        output = self.submitted(b'help', 'Commands:')
        self.assertNotIn('unknown command', output.lower())

    def test_cursor_editing(self):
        # hXlp -> hXelp -> help; exercise right, left, insert and Delete.
        self.submitted(b'hXlp\x1b[H\x1b[C\x1b[Ce\x1b[D\x1b[D\x1b[3~\x1b[F', 'Commands:')
        self.submitted(b'helpx\x7f', 'Commands:')

    def test_history(self):
        self.submitted(b'help', 'Commands:')
        self.submitted(b'\x1b[A', 'Commands:')
        self.shell.send(b'hel\x1b[A\x1b[B')
        self.submitted(b'p', 'Commands:')

    def test_completions(self):
        self.submitted(b'hel\t', 'Commands:')
        self.shell.send(b's\t')
        # Escape dismisses choices but preserves the draft.
        self.shell.send(b'\x1b')
        self.submitted(b'\x15help', 'Commands:')
        self.shell.send(b'\t')
        self.shell.send(b'\x1b[B\x1b[A\r')  # accept, not submit
        self.shell.send(b'\x03')
        self.submitted(b'help', 'Commands:')

    def test_ctrl_d(self):
        self.submitted(b'hXelp\x1b[H\x1b[C\x04', 'Commands:')
        self.shell.send(b'\x04')
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.shell.pid, os.WNOHANG)
            if pid:
                self.assertEqual(os.waitstatus_to_exitcode(status), 0)
                # close() still owns cleanup; replace reaped process handling.
                self.shell.pid = -1
                return
            time.sleep(.05)
        self.fail('Ctrl+D did not exit')

    def test_multiline(self):
        self.submitted(b'\x1b[200~help\n\x1b[201~', 'Commands:')
        self.submitted(b'help\x0a', 'Commands:')
        # Real multiline navigation: edit the first row, then clear the second.
        # A flattened paste or history routing instead of vertical movement
        # would not produce the submitted help command.
        self.shell.send(b'\x1b[200~helX\njunk\x1b[201~')
        self.submitted(b'\x1b[A\x1b[F\x7fp\x1b[B\x1b[H\x0b', 'Commands:')

    def test_cooked_editor(self):
        self.submitted(b':scope scratch new scope', 'scratch')
        output = self.submitted(b':scope edit --commands', 'Editing draft')
        self.assertIn('1:1>', output)
        self.assertIn('input line rejected', self.shell.send(b'\x1b[D\n'))
        self.shell.send(b':insert abc\\ndef\n')
        self.shell.send(b':at 1 2\n')
        self.shell.send(b':delete 1\n')
        output = self.shell.send(b':show\n')
        self.assertIn('ac', output)
        self.assertIn('def', output)
        self.assertIn('Returned to REPL', self.shell.send(b':done\n'))
        self.submitted(b':scope show', 'ac')
        self.submitted(b':scope edit --commands', 'Editing draft')
        self.shell.send(b':insert discarded\n')
        self.assertIn('Returned to REPL', self.shell.send(b':cancel\n'))
        output = self.submitted(b':scope show', 'ac')
        self.assertNotIn('discarded', output)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=BINARY)
    args, rest = parser.parse_known_args()
    BINARY = args.binary.resolve()
    unittest.main(argv=[__file__] + rest)
