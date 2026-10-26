#!/usr/bin/env python3
"""Stdlib PTY regression checks: cargo build -p scope-workbench --offline --locked;
python3 crates/scope-script/tests/pty_continuous_shell.py -v [--binary PATH].
Checks fresh terminal output, not emulated screen cells. --transcript prints the
ANSI-stripped real terminal reads (including redraws) after each test.
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

BINARY = Path(__file__).resolve().parents[3] / 'target/debug/scope'
TRANSCRIPT = False
ANSI = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]')


class ContinuousShell(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='continuous-script-pty-')
        self.addCleanup(self.temp.cleanup)
        self.transcript = ''
        self.pending = b''
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(self.temp.name)
            os.environ['TERM'] = 'xterm-256color'
            os.environ['XDG_STATE_HOME'] = self.temp.name
            os.environ.pop('SCOPE_HISTORY_FILE', None)
            os.execv(str(BINARY), [str(BINARY), 'shell'])
        self.addCleanup(self.close)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
        self.assertIn('scope> ', self.read(wait_for_start=True))
        self.command(':scope script enter', 'script> ')

    def close(self):
        if TRANSCRIPT:
            print('\n--- ' + self.id() + ' ---\n' + self.transcript)
        try:
            os.kill(self.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        os.waitpid(self.pid, 0)
        os.close(self.fd)

    def read(self, wait_for_start=False):
        data = b''
        deadline = time.monotonic() + 10
        quiet = time.monotonic() + .4
        while time.monotonic() < deadline:
            if not select.select([self.fd], [], [], .05)[0]:
                if time.monotonic() > quiet and (data or not wait_for_start):
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
            self.pending += chunk
            while b'\x1b[6n' in self.pending:
                _, self.pending = self.pending.split(b'\x1b[6n', 1)
                os.write(self.fd, b'\x1b[1;1R')
            self.pending = self.pending[-3:]
        text = ANSI.sub(b'', data).decode('utf-8', errors='replace').replace('\r', '')
        self.transcript += text
        return text

    def send(self, keys):
        os.write(self.fd, keys)
        return self.read()

    def command(self, text, expected, cooked=False):
        output = self.send(text.encode() + (b'\n' if cooked else b'\r'))
        self.assertIn(expected, output, repr(output))
        return output

    def result(self, keys, value):
        output = self.send(keys + b'\r')
        self.assertIn('\n' + str(value) + '\n', output, repr(output))
        self.assertIn('script> ', output)
        for verbose in ('final:', 'status: interpreter', 'script: SourceId'):
            self.assertNotIn(verbose, output)
        return output

    def test_navigation_history_completion_and_paste(self):
        self.command('let answer = 40', 'script> ')
        # Home/Right/Delete/End/Backspace: Xanswer + 2x -> answer + 2.
        self.result(b'Xanswer + 2x\x1b[H\x1b[C\x1b[D\x1b[3~\x1b[F\x7f', 42)
        self.result(b'\x1b[A', 42)
        # Down restores the draft after browsing history.
        self.send(b'answer + \x1b[A\x1b[B')
        self.result(b'3', 43)
        self.result(b'len\t([1, 2])', 2)
        self.result(b'\x1b[200~(answer +\n5)\x1b[201~', 45)
        self.command(':sho\t', 'let answer = 40')

    def test_pending_commands_invalid_and_ctrl_c(self):
        self.command('(', '...> ')
        self.command(':show', '...> ')
        self.command(':run', 'finish it or :cancel before :run')
        self.result(b'42)', 42)  # commands retained the pending opening paren
        self.command('(', '...> ')
        self.command('let = ;', 'error:')
        self.result(b'43', 43)  # invalid drops the entire pending draft
        self.command('(', '...> ')
        self.assertIn('script> ', self.send(b'\x03'))
        self.result(b'44', 44)
        self.command('(', '...> ')
        self.command(':cancel', 'Pending Script input canceled.')
        self.result(b'45', 45)

    def test_readable_growth_binding_edits_and_pending_inspection(self):
        self.result(b'let radius = 5;', 5)
        self.result(b'rad\t + 1', 6)
        for value in range(20):
            self.result(('radius + ' + str(value)).encode(), 5 + value)
        source = self.command(':show', 'radius + 19')
        self.assertNotIn('\n;\n', source)
        self.command('if true {', '...> ')
        pending = self.command(':show', 'Pending Script snapshot (not committed; not evaluated):')
        self.assertIn('Committed Script source:', pending)
        self.assertIn('if true {', pending)
        self.command(':cancel', 'Pending Script input canceled.')
        self.result(b'rad\t', 5)
        self.command(':edit --commands', 'Editing draft')
        self.command(':at 1 5', '>', cooked=True)
        self.command(':delete 6', '>', cooked=True)
        self.command(':insert diameter', '>', cooked=True)
        self.command(':done', 'script> ', cooked=True)
        # Completion reflects the edited declaration even though older uses now fail.
        output = self.send(b'diam\t')
        self.assertIn('diameter', output)
        self.send(b'\x03')

    def test_compact_proposal_lifecycle_without_eval_or_save(self):
        self.command(':edit --commands', 'Editing draft')
        self.command('# λ😀', '>', cooked=True)
        self.command('let mut n = 6; n = 12; n', '>', cooked=True)
        self.command(':done', 'script> ', cooked=True)
        preview = self.command(':compact', 'source proposal 1 pending')
        self.assertIn('Assurance: unevaluated', preview)
        self.assertIn('not saved or evaluated', preview)
        self.assertIn('+let mut n = 12;', preview)
        self.command(':source-reject 1', 'script> ')
        self.command(':scope scratch show', 'let mut n = 6; n = 12; n')
        self.command(':compact', 'source proposal 2 pending')
        self.command(':scope scratch new scope-script', 'created (scope-script)')
        self.command(':source-accept 2', 'source proposal 2 applied')
        other = self.command(':scope scratch show', 'Scratch(1) @0')
        self.assertNotIn('let mut n', other)
        self.command(':scope scratch select 0', 'script> ')
        reduced = self.command(':scope scratch show', 'let mut n = 12;  n')
        self.assertIn('# λ😀', reduced)
        self.command(':compact', 'unsupported source')
        self.command(':edit --commands', 'Editing draft')
        self.command(':at 2 1', '>', cooked=True)
        self.command(':delete 18', '>', cooked=True)
        self.command(':insert let mut n = 6; n = 12; n', '>', cooked=True)
        self.command(':done', 'script> ', cooked=True)
        self.command(':compact', 'source proposal 3 pending')
        self.command(':edit --commands', 'Editing draft')
        self.command(':at 2 13', '>', cooked=True)
        self.command(':insert 0', '>', cooked=True)
        self.command(':done', 'script> ', cooked=True)
        self.command(':source-accept 3', 'RevisionConflict')
        self.command(':script evidence', 'no retained Script evaluation')
        files = [p.relative_to(self.temp.name).as_posix()
                 for p in Path(self.temp.name).rglob('*') if p.is_file()]
        self.assertTrue(all('history' in p for p in files), files)

    def test_explicit_null(self):
        self.result(b'null', 'null')

    def test_editor_leave_reenter_effects_and_evidence(self):
        self.command('let n = 6;', 'script> ')
        self.result(b'n * 7', 42)
        self.command(':edit --commands', 'Editing draft')
        self.command(':at 1 9', '>', cooked=True)
        self.command(':delete 1', '>', cooked=True)
        self.command(':insert 8', '>', cooked=True)
        self.command(':done', 'script> ', cooked=True)
        self.result(b':run', 56)
        self.command(':script leave', 'scope> ')
        self.command(':scope script enter', 'script> ')
        self.result(b'n + 1', 9)
        notice = 'Source updated. Run required (:run, full snapshot).'
        self.command('print(n);', notice)
        self.result(b':run', 8)
        evidence = self.command(':script evidence', 'source snapshot:')
        self.assertIn('status: interpreter success', evidence)
        self.assertIn('print: 8', evidence)
        self.assertIn('script: SourceId', evidence)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=BINARY)
    parser.add_argument('--transcript', action='store_true')
    args, rest = parser.parse_known_args()
    BINARY = args.binary.resolve()
    TRANSCRIPT = args.transcript
    if not BINARY.is_file():
        parser.error(f'build the workbench first: {BINARY}')
    unittest.main(argv=[__file__] + rest)
