#!/usr/bin/env python3
"""Real scope workspace PTY regression, Python stdlib only; no mocked input loop."""
import argparse
import codecs
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
import unicodedata
import unittest


class Screen:
    """Small VT screen model for the cursor/erase sequences emitted by Ratatui.

    Assertions inspect reconstructed frames, not ANSI-stripped diff fragments.
    Styling, terminal modes and cursor visibility intentionally have no cells.
    """
    def __init__(self, rows=44, cols=180):
        self.resize(rows, cols)
        self.pending = ''
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')

    def resize(self, rows, cols):
        self.rows, self.cols = rows, cols
        self.cells = [[' '] * cols for _ in range(rows)]
        self.row = self.col = 0

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        while self.pending:
            if self.pending.startswith('\x1b['):
                match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', self.pending)
                if not match:
                    return
                params, _, action = match.groups()
                self.pending = self.pending[match.end():]
                if params.startswith('?'):
                    continue
                nums = [int(n or '0') for n in params.split(';')]
                n = nums[0] or 1
                if action in 'Hf':
                    self.row = max(0, n - 1)
                    self.col = max(0, (nums[1] or 1) - 1) if len(nums) > 1 else 0
                elif action == 'A': self.row = max(0, self.row - n)
                elif action == 'B': self.row += n
                elif action == 'C': self.col += n
                elif action == 'D': self.col = max(0, self.col - n)
                elif action == 'G': self.col = n - 1
                elif action == 'd': self.row = n - 1
                elif action == 'J' and nums[0] in (2, 3):
                    self.cells = [[' '] * self.cols for _ in range(self.rows)]
                elif action == 'K' and self.row < self.rows:
                    start = 0 if nums[0] in (1, 2) else self.col
                    end = self.cols if nums[0] in (0, 2) else self.col + 1
                    self.cells[self.row][start:end] = [' '] * max(0, end - start)
                continue
            if self.pending.startswith('\x1b'):
                if len(self.pending) < 2:
                    return
                if self.pending[1] in '()':
                    if len(self.pending) < 3:
                        return
                    self.pending = self.pending[3:]
                else:
                    self.pending = self.pending[2:]
                continue
            ch, self.pending = self.pending[0], self.pending[1:]
            if ch == '\r': self.col = 0
            elif ch == '\n': self.row += 1
            elif ch == '\b': self.col = max(0, self.col - 1)
            elif ch >= ' ':
                width = 0 if unicodedata.combining(ch) else (2 if unicodedata.east_asian_width(ch) in 'WF' else 1)
                if self.row < self.rows and self.col < self.cols:
                    self.cells[self.row][self.col] = ch
                    if width == 2 and self.col + 1 < self.cols:
                        self.cells[self.row][self.col + 1] = ''
                self.col += width

    def text(self):
        return '\n'.join(''.join(row) for row in self.cells)


class Workspace:
    def __init__(self, binary):
        self.workspace = tempfile.TemporaryDirectory(prefix='scope-workspace-pty-')
        self.screen = Screen()
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.environ['TERM'] = 'xterm-256color'
            os.environ['XDG_STATE_HOME'] = self.workspace.name
            os.environ.pop('SCOPE_HISTORY_FILE', None)
            os.chdir(self.workspace.name)
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', 44, 180, 0, 0))
            os.execv(str(binary), [str(binary), 'workspace'])
        self.query_pending = b''
        self.wire = bytearray()
        self.commands = 0
        self.actions = 0

    def read(self):
        deadline, quiet = time.monotonic() + 5, time.monotonic() + .15
        while time.monotonic() < deadline:
            if not select.select([self.fd], [], [], .03)[0]:
                if time.monotonic() > quiet:
                    break
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not data:
                break
            self.wire.extend(data)
            self.screen.feed(data)
            self.query_pending += data
            while b'\x1b[6n' in self.query_pending:
                _, self.query_pending = self.query_pending.split(b'\x1b[6n', 1)
                os.write(self.fd, b'\x1b[1;1R')
            self.query_pending = self.query_pending[-3:]
            quiet = time.monotonic() + .15
        return self.screen.text()

    def send(self, keys):
        os.write(self.fd, keys)
        return self.read()

    def command(self, text):
        self.commands += 1
        self.send(b'\x1bOS')  # xterm F4
        return self.send(text.encode() + b'\r')

    def action(self, query):
        self.send(b'\x1bOP')  # xterm F1
        self.send(query.encode())
        self.actions += 1
        return self.send(b'\r')

    def paste(self, text):
        return self.send(b'\x1b[200~' + text.encode() + b'\x1b[201~')

    def resize(self, rows, cols):
        self.screen.resize(rows, cols)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack('HHHH', rows, cols, 0, 0))
        # TIOCSWINSZ delivers SIGWINCH to the PTY foreground process group.
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            screen = self.read()
            if 'F4 command' in screen or ('┌editor' in screen and 'F1 actions' in screen):
                return screen
        raise AssertionError('workspace did not redraw after resize:\n' + self.screen.text())

    def output_containing(self, expected):
        self.command('pane output')
        # Exercise both scroll bounds. Page increments can exceed the six-row
        # output viewport, so inspect frames from both ends of the transcript.
        frames = []
        for phase in range(4):
            frames.append(self.send(b'\x1b[5~' * 100))
            if expected in frames[-1]:
                return frames[-1]
            for direction in (b'\x1b[6~', b'\x1b[5~'):
                unchanged = 0
                for _ in range(80):
                    before = self.screen.text()
                    frames.append(self.send(direction))
                    if expected in frames[-1]:
                        return frames[-1]
                    # Shell banners repeat: a single identical frame need not
                    # mean we reached a scroll bound.
                    unchanged = unchanged + 1 if frames[-1] == before else 0
                    if unchanged == 3:
                        break
            # Current TUI pages by 10 with only 6 output rows. Three help lines
            # shift the bottom scroll bound, exposing otherwise skipped rows.
            # This is presentation-only; no source, runner or host is altered.
            if phase < 3:
                self.command('help')
        raise AssertionError(f'{expected!r} absent from output:\n' + '\n'.join(sorted({line.strip() for frame in frames for line in frame.splitlines()[-8:-2]})))

    def close(self):
        if self.pid > 0:
            try:
                os.kill(self.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            os.waitpid(self.pid, 0)
        os.close(self.fd)
        self.workspace.cleanup()


class WorkspaceTests(unittest.TestCase):
    def test_workspace_direct_action_workflow(self):
        ui = Workspace(BINARY)
        self.addCleanup(ui.close)
        screen = ''
        deadline = time.monotonic() + 10
        while 'F4 command' not in screen and time.monotonic() < deadline:
            screen = ui.read()
        self.assertIn('F4 command', screen)
        original = re.search(r'Selected scratch (\d+)', screen).group(1)
        initial_enters = ui.wire.count(b'\x1b[?1049h')
        initial_leaves = ui.wire.count(b'\x1b[?1049l')

        def editor_focused():
            self.assertIn('FOCUSED', ui.screen.text().splitlines()[1])

        def output_lines():
            # Read each output row via single-line scrolling; no transcript
            # commands or help output can manufacture/obscure a result.
            ui.send(b'\x1b[5~' * 20)
            frames = [ui.screen.text().split('┌output', 1)[1]]
            for _ in range(18):
                frame = ui.send(b'\x1b[B')
                frames.append(frame.split('┌output', 1)[1])
            ui.send(b'\x1b[5~' * 20)
            return '\n'.join(frames)

        def replace_source(old, new):
            editor_focused()
            ui.send(b'\x1b[H')
            ui.send(b'\x1b[3~' * len(old))
            ui.paste(new)
            ui.send(b'\x13')
            editor_focused()
            self.assertIn(new, ui.screen.text())

        # Canceling a searched action is isolated from both editor and shell.
        ui.send(b'\x1bOP')
        ui.send(b'New Script')
        self.assertIn('New Script scratch (select)', ui.screen.text())
        ui.send(b'\x1b')
        editor_focused()
        self.assertIn(f'Selected scratch {original}', ui.screen.text())
        editor_text = '\n'.join(line[:90] for line in ui.screen.text().splitlines()[1:35])
        self.assertNotIn('New Script', editor_text)
        ui.action('New Script')
        editor_focused()
        selected = re.search(r'Selected scratch (\d+)', ui.screen.text()).group(1)
        self.assertNotEqual(selected, original)
        source = 'print("first雪"); print("second"); 42'
        ui.paste(source)

        # Selected dirty source is blocked, with actionable picker feedback.
        ui.send(b'\x1bOP')
        screen = ui.send(b'Run selected Script')
        self.assertIn('commit or cancel all editor drafts', screen)
        ui.send(b'\r')
        self.assertIn('Actions:', ui.screen.text())
        ui.send(b'\x1b')
        ui.send(b'\x13')
        editor_focused()
        ui.send(b'\x1b[15~')  # xterm F5: direct run, not shell command
        editor_focused()
        ui.action('Focus output')
        self.assertNotIn('FOCUSED', ui.screen.text().splitlines()[1])
        success = output_lines()
        revisions = {re.search(r'revision RevisionId\((\d+)\)', success).group(1)}
        self.assertIn('Result 1/1', success)
        self.assertIn('current committed revision', success)
        self.assertIn('status: interpreter success', success)
        ordered = ui.screen.text()
        self.assertLess(ordered.index('print 1: "first雪"'), ordered.index('print 2: "second"'))
        self.assertLess(ordered.index('print 2: "second"'), ordered.index('final: 42'))
        before = ui.screen.text()
        self.assertNotEqual(ui.send(b'\x1b[6~'), before)
        ui.send(b'\x1b[A')
        ui.send(b'\x1b[5~')
        ui.send(b'\x1bOR')  # xterm F3: previous pane
        editor_focused()

        runtime = 'print("before failure"); missing'
        replace_source(source, runtime)
        ui.send(b'\x1b[15~')
        editor_focused()
        ui.action('Focus output')
        failure = output_lines()
        revisions.add(re.search(r'revision RevisionId\((\d+)\)', failure).group(1))
        self.assertIn('Result 2/2', failure)
        self.assertIn('status: interpreter failure', failure)
        self.assertIn('print 1: "before failure"', failure)
        self.assertIn('unknown binding', failure)
        self.assertNotIn('final:', failure)
        ui.send(b'\x1b[1;3D')
        historical = output_lines()
        self.assertIn('Result 1/2', historical)
        self.assertIn('older committed revision', historical)
        self.assertIn('final: 42', historical)
        self.assertNotIn('before failure', historical)
        ui.send(b'\x1b[1;3C')
        self.assertIn('Result 2/2', output_lines())
        ui.send(b'\x1bOR')

        malformed = 'print('
        replace_source(runtime, malformed)
        ui.send(b'\x1b[15~')
        editor_focused()
        ui.action('Focus output')
        parse_failure = output_lines()
        revisions.add(re.search(r'revision RevisionId\((\d+)\)', parse_failure).group(1))
        self.assertIn('Result 3/3', parse_failure)
        self.assertIn('status: interpreter failure', parse_failure)
        self.assertIn('error:', parse_failure)
        self.assertIn('location:', parse_failure)
        self.assertNotIn('print 1:', parse_failure)
        self.assertNotIn('final:', parse_failure)
        ui.send(b'\x1bOR')
        recovered = 'print("recovered"); 43'
        replace_source(malformed, recovered)
        ui.send(b'\x1b[15~')
        editor_focused()
        ui.action('Focus output')
        recovery = output_lines()
        revisions.add(re.search(r'revision RevisionId\((\d+)\)', recovery).group(1))
        self.assertEqual(len(revisions), 4)
        self.assertIn('Result 4/4', recovery)
        self.assertIn('final: 43', recovery)
        self.assertIn('status: interpreter success', recovery)
        ui.send(b'\x1bOR')
        ui.resize(8, 36)
        ui.resize(44, 180)
        editor_focused()
        self.assertIn(recovered, ui.screen.text())
        self.assertIn('Semantic target: unbound', ui.screen.text())
        self.assertEqual(ui.commands, 0)
        self.assertEqual(ui.wire.count(b'\x1b[?1049h'), initial_enters)
        self.assertEqual(ui.wire.count(b'\x1b[?1049l'), initial_leaves)
        self.assertEqual(list(Path(ui.workspace.name).rglob('*')), [])

        # Normal exit must restore the terminal, not just terminate the child.
        ui.send(b'\x11')
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            pid, status = os.waitpid(ui.pid, os.WNOHANG)
            if pid:
                ui.pid = -1
                self.assertEqual(os.waitstatus_to_exitcode(status), 0)
                break
            time.sleep(.05)
        else:
            self.fail('Ctrl-Q did not exit direct-action workspace')
        self.assertEqual(ui.wire.count(b'\x1b[?1049l'), initial_leaves + 1)
        self.assertIn(b'\x1b[?25h', ui.wire)
        restored = termios.tcgetattr(ui.fd)[3]
        self.assertTrue(restored & termios.ICANON)
        self.assertTrue(restored & termios.ECHO)
        print(f'\nDirect PTY evidence: commands={ui.commands}, picker actions={ui.actions}, '
              f'retained results=4, observed script revisions={sorted(revisions)}, '
              f'alt-screen enters={ui.wire.count(bytes.fromhex("1b5b3f3130343968"))}, '
              f'leaves={ui.wire.count(bytes.fromhex("1b5b3f313034396c"))}')

    def test_workspace_f2_shell_handoff(self):
        ui = Workspace(BINARY)
        self.addCleanup(ui.close)
        screen = ''
        deadline = time.monotonic() + 10
        while 'F4 command' not in screen and time.monotonic() < deadline:
            screen = ui.read()
        self.assertIn('F4 command', screen)
        ui.action('New Script')
        committed = 'print("handoff雪"); 17'
        ui.paste(committed)
        ui.send(b'\x13')
        draft_suffix = ' + 5'
        screen = ui.paste(draft_suffix)
        selected = re.search(r'Selected scratch (\d+)', screen).group(1)
        revision = re.search(r'Revision: RevisionId\((\d+)\)', screen).group(1)
        enters = ui.wire.count(b'\x1b[?1049h')
        leaves = ui.wire.count(b'\x1b[?1049l')
        start = len(ui.wire)
        ui.send(b'\x1bOQ')  # xterm F2: lend the terminal to the Scope shell
        deadline = time.monotonic() + 5
        while b'scope> ' not in ui.wire[start:] and time.monotonic() < deadline:
            ui.read()
        self.assertIn(b'scope> ', ui.wire[start:])
        self.assertEqual(ui.wire.count(b'\x1b[?1049h'), enters)
        self.assertEqual(ui.wire.count(b'\x1b[?1049l'), leaves + 1)


        screen = ui.send(b':scope quit\r')
        deadline = time.monotonic() + 5
        while 'F4 command' not in screen and time.monotonic() < deadline:
            screen = ui.read()
        self.assertEqual(ui.wire.count(b'\x1b[?1049h'), enters + 1)
        self.assertIn('FOCUSED', screen.splitlines()[1])
        self.assertIn(f'Selected scratch {selected}', screen)
        self.assertIn(f'Revision: RevisionId({revision})', screen)

        # Pane bodies share rows; inspect the left/right columns separately.
        editor_text = '\n'.join(line[:90] for line in screen.splitlines()[1:35])
        inspector_text = '\n'.join(line[90:] for line in screen.splitlines()[1:35])
        self.assertIn(committed + draft_suffix, editor_text)
        self.assertIn(committed, inspector_text)
        self.assertNotIn(committed + draft_suffix, inspector_text)
        ui.send(b'\x03')  # Cancel retained draft; committed source is unchanged.
        self.assertIn(committed, ui.screen.text())
        self.assertNotIn(committed + draft_suffix, ui.screen.text())
        self.assertIn(f'Revision: RevisionId({revision})', ui.screen.text())
        self.assertEqual(list(Path(ui.workspace.name).rglob('*')), [])

        exit_start = len(ui.wire)
        ui.send(b'\x11')
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            pid, status = os.waitpid(ui.pid, os.WNOHANG)
            if pid:
                ui.pid = -1
                self.assertEqual(os.waitstatus_to_exitcode(status), 0)
                break
            time.sleep(.05)
        else:
            self.fail('Ctrl-Q did not exit workspace after F2 shell handoff')
        self.assertEqual(ui.wire.count(b'\x1b[?1049l'), leaves + 2)
        self.assertIn(b'\x1b[?25h', ui.wire[exit_start:])
        restored = termios.tcgetattr(ui.fd)[3]
        self.assertTrue(restored & termios.ICANON)
        self.assertTrue(restored & termios.ECHO)
        print(f'\nF2 PTY evidence: shell handoffs=1, shell commands=1 (:scope quit), '
              f'selected scratch={selected}, retained revision={revision}, '
              f'draft retained then canceled, '
              f'alt-screen enters={enters + 1}, leaves={leaves + 2}, exit=0')

    def test_workspace_editor_command_output_and_recovery(self):
        ui = Workspace(BINARY)
        self.addCleanup(ui.close)
        screen = ''
        deadline = time.monotonic() + 10
        while 'F4 command' not in screen and time.monotonic() < deadline:
            screen = ui.read()
        self.assertIn('FOCUSED', screen)
        self.assertIn('Read-only inspector', screen)
        self.assertIn('Semantic target: unbound', screen)
        original = re.search(r'Selected scratch (\d+)', screen).group(1)

        # Bracketed paste remains literal draft text, including command-looking
        # lines. Do not commit: retain this original draft across scratch switches.
        marker = 'original雪😀'
        ui.paste(marker + '\n:scope script run')
        self.assertIn(marker, ui.screen.text())
        self.assertIn('LOCAL DRAFT', ui.screen.text())
        ui.command('pane inspector')
        ui.paste('READONLY_SENTINEL')
        self.assertNotIn('READONLY_SENTINEL', ui.screen.text())
        ui.command('pane last')
        self.assertIn('FOCUSED', ui.screen.text().splitlines()[1])

        # This exact grammar must reach the existing scratch parser, not a new
        # workspace shorthand. The reported live ID is used rather than assumed.
        ui.command(':scope scratch new scope-script')
        created = ui.output_containing('created (scope-script)')
        script = re.search(r'scratch (\d+) created \(scope-script\)', created).group(1)
        ui.command('pane last')
        ui.paste('print("script雪😀"); 42')
        ui.send(b'\x13')  # Ctrl-S commits the selected Script, not semantic target.
        self.assertIn('committed to', ui.screen.text())
        self.assertIn('Committed text:', ui.screen.text())
        ui.command(':scope script run')
        ui.output_containing('commit or cancel all editor drafts')
        ui.command('pane last')
        ui.command(f':scope scratch select {original}')
        self.assertIn(marker, ui.screen.text())
        ui.resize(18, 92)
        ui.resize(44, 180)
        self.assertIn(marker, ui.screen.text())
        ui.send(b'\x03')  # cancel original, allowing script execution
        self.assertNotIn(marker, ui.screen.text())
        ui.command(f':scope scratch select {script}')
        ui.command(':scope script run')
        ui.output_containing('final: 42')
        ui.output_containing('print: "script雪😀"')
        ui.output_containing('status: interpreter success')
        ui.command('pane editor')
        self.assertIn('FOCUSED', ui.screen.text().splitlines()[1])

        # Canceling F4 input must not execute it or insert it in the editor.
        ui.send(b'\x1bOS')
        ui.send(b':scope scratch discard')
        ui.send(b'\x1b')
        self.assertIn('print("script雪😀"); 42', ui.screen.text())
        ui.send(b'\x1b[H')  # Commit retains cursor; explicitly insert at the start.
        ui.paste('missing; ')
        ui.send(b'\x13')
        self.assertIn('missing; print("script雪😀"); 42', ui.screen.text())
        ui.command(':scope script run')
        ui.output_containing('unknown binding')
        ui.command('pane editor')
        ui.send(b'\x1b[H')
        ui.send(b'\x1b[3~' * len('missing; '))
        ui.send(b'\x1b[F')
        ui.paste(' + 1')
        ui.send(b'\x13')
        ui.command(':scope script run')
        # New result is distinguishable from earlier success in retained output.
        ui.output_containing('final: 43')
        ui.command('pane editor')
        self.assertIn('print("script雪😀"); 42 + 1', ui.screen.text())
        self.assertNotIn('missing;', ui.screen.text())
        self.assertIn('Semantic target: unbound', ui.screen.text())
        self.assertEqual(list(Path(ui.workspace.name).rglob('*')), [])
        ui.send(b'\x11')
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            pid, status = os.waitpid(ui.pid, os.WNOHANG)
            if pid:
                ui.pid = -1
                self.assertEqual(os.waitstatus_to_exitcode(status), 0)
                break
            time.sleep(.05)
        else:
            self.fail('Ctrl-Q did not exit workspace')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    args, rest = parser.parse_known_args()
    BINARY = args.binary.resolve()
    def timeout(signum, frame):
        raise TimeoutError('workspace PTY suite exceeded 90 seconds')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(90)
    unittest.main(argv=[__file__] + rest)
