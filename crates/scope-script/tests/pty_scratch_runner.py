#!/usr/bin/env python3
"""Stdlib actual-binary PTY tests.

Run from any directory after:
    cargo build -p scope-script --example scratch_runner --offline --locked
    python3 crates/scope-script/tests/pty_scratch_runner.py -v

The PTY/query handling follows scope-ide/tests/pty_shell.py, but this test is
self-contained so it does not depend on that optional developer helper.
"""
import argparse
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import shutil
import signal
import struct
import tempfile
import termios
import time
import unittest

ROOT = Path(__file__).resolve().parents[3]
BINARY = ROOT / 'target/debug/examples/scratch_runner'
FIXTURE = ROOT / 'crates/scope-script/examples/semantic-workspace'
ANSI = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]')
ACTION = 'language.example.declarations.describe_selection'
RUN = f':scope run {ACTION} --selection 0:0..0:15'


class Shell:
    def __init__(self):
        self.temp = tempfile.TemporaryDirectory(prefix='scratch-runner-pty-')
        self.workspace = Path(self.temp.name) / 'workspace'
        shutil.copytree(FIXTURE, self.workspace)
        self.original = self.snapshot()
        self.pending = b''
        self.transcript = ''
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ['TERM'] = 'xterm-256color'
            os.environ['XDG_STATE_HOME'] = self.temp.name
            os.execv(str(BINARY), [str(BINARY), str(self.workspace),
                                   '--allow-read', '--terminal'])
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 160, 0, 0))

    def snapshot(self):
        return {str(p.relative_to(self.workspace)): p.read_bytes()
                for p in self.workspace.rglob('*') if p.is_file()}

    def read(self):
        data = b''
        deadline = time.monotonic() + 5
        quiet = time.monotonic() + .2
        while time.monotonic() < deadline:
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
            data += chunk
            quiet = time.monotonic() + .2
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

    def exit_status(self):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.pid = -1
                return os.waitstatus_to_exitcode(status)
            self.read()
        raise AssertionError('runner did not exit\n' + self.transcript)

    def close(self):
        if self.pid > 0:
            try:
                os.kill(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(self.pid, 0)
            self.pid = -1
        os.close(self.fd)
        self.temp.cleanup()


class ScratchRunnerTests(unittest.TestCase):
    def setUp(self):
        self.shell = Shell()
        self.addCleanup(self.shell.close)
        output = ''
        deadline = time.monotonic() + 10
        while 'scope> ' not in output and time.monotonic() < deadline:
            output += self.shell.read()
        self.assertIn('scope> ', output)

    def command(self, text, expected, cooked=False):
        output = self.shell.send(text.encode() + (b'\n' if cooked else b'\r'))
        self.assertIn(expected, output, repr(output))
        self.assertNotIn('error:', output.lower(), repr(output))
        return output

    def values(self, output, label):
        # Rendered values can span many lines; decode one JSON value per marker.
        return [json.JSONDecoder().raw_decode(output[match.end():])[0]
                for match in re.finditer(rf'^{label}: ', output, re.M)]

    def assert_one_envelope(self, output):
        self.assertEqual(re.findall(r'^host: (\d+) returned envelopes retained',
                                    output, re.M), ['1'], output)

    def finish(self):
        self.shell.send(b':scope quit\r')
        self.assertEqual(self.shell.exit_status(), 0)
        self.assertEqual(self.shell.snapshot(), self.shell.original)

    def test_author_complete_repeat_edit_and_revisions(self):
        self.command('open example.decl', 'example.decl')
        self.command(':scope scratch new scope-script', 'scratch')
        self.command(':scope edit --commands', 'Editing draft')
        # Completion at an empty source is metadata-only, not a script run.
        completion = self.command(':complete', 'completion(s)', cooked=True)
        self.assertIn('scope', completion)
        for forbidden in ('semantic:', 'print:', 'final:', 'status: interpreter'):
            self.assertNotIn(forbidden, completion)
        program = [f'let result = scope.invoke("semantic", "{ACTION}", {{}});',
                   'print(result);', 'result.value.name']
        for line in program:
            self.shell.send(line.encode() + b'\n')
        shown = self.command(':show', 'result.value.name', cooked=True)
        for line in program:
            self.assertIn(line.replace('"', chr(92) + '"'), shown)
        self.command(':done', 'Returned to REPL', cooked=True)
        first = self.command(RUN, 'final: "answer"')
        second = self.command(RUN, 'final: "answer"')
        self.command(':scope edit --commands', 'Editing draft')
        self.shell.send(b':at 3 1\n')
        self.shell.send(b':delete 17\n')
        self.shell.send(b':insert result.value.kind\n')
        self.command(':done', 'Returned to REPL', cooked=True)
        third = self.command(RUN, 'final: "integer_literal_binding"')
        snapshots = []
        for output in (first, second, third):
            self.assert_one_envelope(output)
            printed = self.values(output, 'print')
            self.assertEqual(len(printed), 1, output)
            self.assertIn('status: interpreter success', output)
            envelope = printed[0]
            self.assertEqual(envelope['operation_id'], 'describe_selection')
            self.assertEqual(envelope['status'], 'read_only')
            self.assertTrue(envelope['rationale'])
            self.assertTrue(envelope['value']['partial'])
            self.assertEqual(envelope['value']['references']['status'], 'unsupported')
            self.assertEqual(envelope['value']['inferred_type']['status'], 'unknown')
            script = re.search(r'^script: (.+) revision (.+)$', output, re.M)
            target = re.search(r'^target: (.+)$', output, re.M)
            self.assertIsNotNone(script, output)
            self.assertIsNotNone(target, output)
            snapshots.append((script.group(1), script.group(2), target.group(1)))
        self.assertEqual(snapshots[0], snapshots[1])
        self.assertEqual(snapshots[0][0], snapshots[2][0])
        self.assertNotEqual(snapshots[0][1], snapshots[2][1])
        self.assertEqual(snapshots[0][2], snapshots[2][2])
        self.finish()

    def test_multiline_conditional_example(self):
        self.command('open example.decl', 'example.decl')
        self.command(':scope scratch new scope-script', 'scratch')
        self.command(':scope edit --commands', 'Editing draft')
        program = (FIXTURE.parent / 'conditional-declaration.scope').read_text()
        for line in program.splitlines():
            self.shell.send(line.encode() + b'\n')
        self.command(':done', 'Returned to REPL', cooked=True)
        outputs = [self.command(RUN, 'status: interpreter success') for _ in range(2)]
        for output in outputs:
            self.assert_one_envelope(output)
            printed = self.values(output, 'print')
            self.assertEqual(len(printed), 3, output)
            self.assertEqual(printed[:2], ['answer', '42'])
            self.assertEqual(printed[2]['outcome'], 'integer_literal_binding')
            semantic = printed[2]['evaluation']
            self.assertEqual(semantic['status'], 'read_only')
            self.assertEqual(self.values(output, 'final'), [semantic])
        self.finish()

    def test_command_and_raw_four_line_edit_equivalence_with_resize(self):
        self.command('open example.decl', 'example.decl')
        initial = ['let x = 6;', 'let y = 7;', 'print({x:x,y:y});', 'x*y']
        expected = initial.copy()
        expected[1] = 'let y = 8;'

        def create():
            output = self.command(':scope scratch new scope-script', 'scratch')
            match = re.search(r'^scratch (\d+) created', output, re.M)
            self.assertIsNotNone(match, output)
            self.command(':scope edit --commands', 'Editing draft')
            # One explicit insertion, with escaped rather than submitted newlines.
            self.shell.send((':insert ' + r'\n'.join(initial) + '\n').encode())
            self.command(':done', 'Returned to REPL', cooked=True)
            return int(match.group(1))

        def numbered(output, pattern):
            return [(int(number), text) for number, text in
                    re.findall(pattern, output, re.M)]

        def show():
            output = self.command(':scope show', 'Preview limited')
            return numbered(output, r'^\s*(\d+) \| (.*)$')

        def evaluate(value, y):
            output = self.command(RUN, f'final: {value}')
            self.assertEqual(self.values(output, 'final'), [value])
            self.assertEqual(self.values(output, 'print'), [{'x': 6, 'y': y}])
            self.assertIn('status: interpreter success', output)
            match = re.search(r'^script: (SourceId\(\d+\)) revision RevisionId\((\d+)\)$',
                              output, re.M)
            self.assertIsNotNone(match, output)
            return match.group(1), int(match.group(2))

        command_buffer = create()
        command_before = evaluate(42, 7)
        self.command(':scope edit --commands', 'Editing draft')
        self.shell.send(b':at 2 9\n')
        self.shell.send(b':delete 1\n')
        self.shell.send(b':insert 8\n')
        draft = self.command(':show', '2: let y = 8;', cooked=True)
        command_lines = numbered(draft, r'^(\d+): (.*)$')
        self.assertEqual(command_lines, list(enumerate(expected, 1)))
        self.command(':done', 'Returned to REPL', cooked=True)
        self.assertEqual(show(), command_lines)
        command_after = evaluate(48, 8)
        self.assertEqual(command_after, (command_before[0], command_before[1] + 1))

        raw_buffer = create()
        self.assertNotEqual(raw_buffer, command_buffer)
        raw_before = evaluate(42, 7)
        self.assertEqual(raw_before[1], command_before[1])
        self.shell.send(b':scope edit\r')
        self.shell.send(b'\x1b[B\x1b[H' + b'\x1b[C' * 8 + b'\x1b[3~8')
        for rows, columns in ((20, 100), (40, 160)):
            fcntl.ioctl(self.shell.fd, termios.TIOCSWINSZ,
                        struct.pack('HHHH', rows, columns, 0, 0))
            redraw = self.shell.read()
            self.assertIn('Ctrl-S: accept', redraw)
            for line in expected:
                self.assertEqual(redraw.count(line), 1, repr(redraw))
            self.assertNotIn('let y = 7;', redraw)
            # Shell.read strips cursor/erase sequences: this checks fresh frame
            # content, not a full terminal-emulator assertion about screen cells.
        self.shell.send(b'\x13')
        self.assertEqual(show(), command_lines)
        raw_after = evaluate(48, 8)
        self.assertEqual(raw_after, (raw_before[0], raw_before[1] + 1))
        self.assertEqual(raw_after[1], command_after[1])
        self.finish()

    def test_raw_multiline_edit_reopen_cancel_and_paste(self):
        self.command('open example.decl', 'example.decl')
        self.command(':scope scratch new scope-script', 'scratch')

        def edit():
            # Do not depend on full-screen rendering or its status-line wording.
            self.shell.send(b':scope edit\r')

        def run(expected):
            output = self.command(RUN, f'final: {expected}')
            self.assertEqual(self.values(output, 'final'), [expected])
            self.assertIn('status: interpreter success', output)
            match = re.search(r'^script: (SourceId\(\d+\)) revision RevisionId\((\d+)\)$', output, re.M)
            self.assertIsNotNone(match, output)
            return match.group(1), int(match.group(2))

        edit()
        # Paste is source, not shell/editor commands. Include Unicode, a tab,
        # command-like string contents, and real newlines.
        source = 'let label = "λ:cancel";\n\tlet n = 6;\nn'
        self.shell.send(b'\x1b[200~' + source.encode() + b'\x1b[201~')
        self.shell.send(b'\x13')  # Ctrl-S accepts, without saving to disk.
        first = run(6)

        edit()
        # Start at the first line; Down/Home/Right/Delete replace only the 6.
        self.shell.send(b'\x1b[H\x1b[B\x1b[H' + b'\x1b[C' * 9 + b'\x1b[3~8')
        # End/Enter/Tab/Backspace exercise multiline editing without changing
        # the program's result. Up/Left return through the inserted whitespace.
        self.shell.send(b'\x1b[F\r\t\x7f\x7f\x1b[A\x1b[D')
        self.shell.send(b'\x13')
        changed = run(8)
        self.assertEqual(changed, (first[0], first[1] + 1))

        edit()
        self.shell.send(b'\x13')
        self.assertEqual(run(8), changed)
        for cancel in (b'\x03', b'\x1b'):
            edit()
            self.shell.send(b'not valid source')
            self.shell.send(cancel)
            self.assertEqual(run(8), changed)
        self.finish()

    def test_top_level_completion_cancel_and_keyboard(self):
        self.assertIn('[1/', self.shell.send(b's\t'))
        self.assertIn('^C', self.shell.send(b'\x03'))
        self.command('help', 'Commands:')
        self.command('hel\t', 'Commands:')
        self.shell.send(b's\t')
        self.shell.send(b'\x1b')
        self.command('\x15help', 'Commands:')
        # Home, Right, Delete, End and Backspace turn hXelpx into help.
        self.command('hXelpx\x1b[H\x1b[C\x1b[3~\x1b[F\x7f', 'Commands:')
        self.command('\x1b[A', 'Commands:')
        self.shell.send(b'\x04')
        self.assertEqual(self.shell.exit_status(), 0)
        self.assertEqual(self.shell.snapshot(), self.shell.original)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=BINARY)
    args, rest = parser.parse_known_args()
    BINARY = args.binary.resolve()
    if not BINARY.is_file():
        parser.error(f'binary not found: {BINARY}; build the scratch_runner example first')
    unittest.main(argv=[__file__] + rest)
