#!/usr/bin/env python3
"""Real App source integrity and final visible cells; stdlib, 90s deadline."""
import argparse
from pathlib import Path
import signal
import shlex
import tempfile
import time
import unittest
import unicodedata

from pty_workspace import Workspace
from visible_screen import VisibleScreen


class ScreenTests(unittest.TestCase):
    def test_fragmented_utf8_tabs_combining_wide_and_overwrite(self):
        screen = VisibleScreen(3, 32)
        wire = '\x1b[2J\x1b[1;1HA\t猫e\u0301Z'.encode()
        for byte in wire:
            screen.feed(bytes([byte]))
        self.assertEqual(screen.cells[0][:13], ['A'] + [' '] * 7 + ['猫', '', 'e\u0301', 'Z', ' '])
        screen.feed(b'\x1b[1;9HX\x1b[K')
        self.assertEqual(screen.text().splitlines()[0], 'A       X' + ' ' * 23)

    def test_cursor_erase_resize_and_bound(self):
        screen = VisibleScreen(2, 10)
        screen.feed(b'old\x1b[1;1Hnew\x1b[2;1Htail\x1b[2K')
        self.assertEqual(screen.text(), 'new       \n          ')
        screen.resize(3, 12)
        self.assertEqual(screen.text(), '\n'.join([' ' * 12] * 3))
        with self.assertRaises(AssertionError):
            screen.resize(201, 10)
        with self.assertRaises(AssertionError):
            screen.feed(b'\x1b[1P')


def main(binary):
    def timeout(*_):
        raise TimeoutError('visual editor PTY exceeded 90 seconds')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(90)
    launcher_dir = tempfile.TemporaryDirectory(prefix='scope-visual-launcher-')
    launcher = Path(launcher_dir.name) / 'scope'
    launcher.write_text('#!/bin/sh\nexec ' + shlex.quote(str(binary)) + ' --writable workspace .\n')
    launcher.chmod(0o700)
    ui = Workspace(launcher)
    ui.screen = VisibleScreen()
    failures = []
    try:
        deadline = time.monotonic() + 10
        while 'F4 command' not in ui.read() and time.monotonic() < deadline:
            pass
        assert 'F4 command' in ui.screen.text(), ui.screen.text()
        path = Path(ui.workspace.name) / 'visual.scope'
        initial = '\tlet café = "猫";\n\tlet second = 2;'
        path.write_bytes(initial.encode())
        ui.command(':scope open visual.scope')
        ui.action('Focus editor')
        ui.send(b'\x1b[H')
        ui.send(b'X')  # literal typing before a literal source tab
        ui.paste('λ')
        ui.send(b'\x1b[C\x1b[D')  # navigation across the tab, no source change
        expected = 'Xλ' + initial

        def viewport():
            # Select only the outer editor pane, never inspector/transcript text.
            # The editor body has no nested border/title; a status row is present
            # only while dirty or reporting publication (undo can clear it).
            rows = ui.screen.cells
            border = next(i for i, row in enumerate(rows) if ''.join(row).startswith('┌editor'))
            left = 0
            right = rows[border].index('┐')
            bottom = border + 1
            while rows[bottom][left] == rows[bottom][right] == '│':
                bottom += 1
            status = ''.join(rows[bottom - 1][left + 1:right]).strip()

            # Status text is outside the source viewport; a clean undo has none.
            if status in ('Local draft', 'Published in memory'):
                bottom -= 1
            return [row[left + 1:right] for row in rows[border + 1:bottom]], border + 1, left + 1

        def source_cells(line):
            # Independent fixture oracle: four-column tab stops, BMP wide cells,
            # and combining accents. No dependency on renderer implementation.
            cells = []
            for ch in line:
                if ch == '\t':
                    cells.extend([' '] * (4 - len(cells) % 4))
                elif unicodedata.combining(ch):
                    c = len(cells) - 1
                    while cells[c] == '':
                        c -= 1
                    cells[c] += ch
                else:
                    cells.append(ch)
                    if unicodedata.east_asian_width(ch) in 'WF':
                        cells.append('')
            return cells

        top = 0

        def visible(label, cursor_row=0, cursor_column=2):
            nonlocal top
            actual, origin_row, origin_col = viewport()
            height, width = len(actual), len(actual[0])
            margin = min(3, (height - 1) // 2)
            if cursor_row < top + margin:
                top = max(0, cursor_row - margin)
            elif cursor_row >= top + height - margin:
                top = cursor_row - (height - margin - 1)
            left = max(0, cursor_column - width + 1)
            wanted_cursor = (origin_row + cursor_row - top, origin_col + cursor_column - left)
            actual_cursor = (ui.screen.row, ui.screen.col)
            if actual_cursor != wanted_cursor:
                failures.append(f'{label}: cursor expected {wanted_cursor}, actual {actual_cursor}')
            lines = expected.split('\n')
            for row, observed in enumerate(actual):
                cells = source_cells(lines[top + row]) if top + row < len(lines) else []
                wanted = cells[left:left + width]
                if wanted and wanted[0] == '':
                    wanted[0] = ' '  # left edge bisects a wide glyph
                if left + width < len(cells) and cells[left + width] == '':
                    wanted[-1] = ' '  # right edge bisects a wide glyph
                wanted += [' '] * (width - len(wanted))
                if observed != wanted:
                    failures.append(f'{label} row {top + row}: expected {wanted!r}; actual {observed!r}')
            print(f'EVIDENCE {label}: checked {height}x{width} cells, top={top}, left={left}', flush=True)

        tab_offsets = [i for i, byte in enumerate(ui.wire) if byte == 9]
        print('EVIDENCE emitted HT neighborhoods: ' + repr([
            bytes(ui.wire[max(0, i - 20):i + 40]) for i in tab_offsets[-4:]
        ]), flush=True)
        visible('typing/paste/navigation')
        ui.action('Focus output')
        ui.action('Focus editor')
        visible('focus roundtrip')
        ui.resize(38, 140)
        visible('resize smaller')
        ui.resize(44, 180)
        visible('resize restored')
        # This generic launcher opens a scratch, not an App document. Exercise
        # Ctrl-T focus/no-save safety, but do not claim App diagnostic publication.
        clean_draft = expected
        ui.send(b'\x1b[H')
        ui.paste('<<<<<<< ours\n')
        expected = '<<<<<<< ours\n' + clean_draft
        visible('unpublished diagnostic marker', 1, 0)
        ui.send(b'\x14')
        assert path.read_bytes() == initial.encode(), 'Ctrl-T unexpectedly saved'
        visible('Ctrl-T unpublished scratch and retained editor focus', 1, 0)
        print('EVIDENCE Ctrl-T diagnostic publication unavailable: generic '
              'workbench opens a scratch, not an App document; checking only '
              'visible draft integrity, editor focus and no-save safety', flush=True)
        ui.send(b'Q')
        expected = '<<<<<<< ours\nQ' + clean_draft
        visible('typing after Check stays in editor', 1, 1)
        ui.send(b'\x7f')
        expected = '<<<<<<< ours\n' + clean_draft
        ui.send(b'\x7f')  # join marker and source, then remove the marker
        expected = '<<<<<<< ours' + clean_draft
        visible('join after Check', 0, 12)
        ui.send(b'\x1b[H' + b'\x1b[3~' * 12)
        expected = clean_draft
        visible('remove diagnostic marker', 0, 0)
        ui.send(b'\x14')
        assert path.read_bytes() == initial.encode(), 'clean Ctrl-T unexpectedly saved'
        visible('Ctrl-T clean scratch retains focus', 0, 0)
        ui.send(b'\x1b[C' * 2)
        visible('navigation after Check', 0, 2)
        ui.send(b'\x13')  # authoritative in-memory scratch publication
        assert path.read_bytes() == initial.encode(), 'publication unexpectedly saved'
        ui.command(':scope save')
        assert path.read_bytes() == expected.encode(), (path.read_bytes(), expected.encode())
        print(f'EVIDENCE authoritative App save: {path.read_bytes()!r}', flush=True)
        ui.action('Focus editor')
        visible('published/saved')
        baseline = expected
        ui.send(b'\x1b[H\t')
        expected = '    ' + baseline
        visible('Tab indent', cursor_column=4)
        ui.send(b'\x1a')
        expected = baseline
        visible('undo Tab', cursor_column=0)
        ui.send(b'\x19')
        expected = '    ' + baseline
        visible('redo Tab', cursor_column=4)
        ui.send(b'\x1b[Z')  # xterm Shift-Tab
        expected = baseline
        visible('Shift-Tab dedent', cursor_column=0)
        ui.send(b'\x1a')
        expected = '    ' + baseline
        visible('undo Shift-Tab', cursor_column=4)
        ui.send(b'\x19')
        expected = baseline
        visible('redo Shift-Tab', cursor_column=0)

        for iteration in range(3):
            ui.send('é'.encode())
            expected = 'é' + baseline
            visible(f'typed accent {iteration}', cursor_column=1)
            ui.send(b'\x7f')
            expected = baseline
            visible(f'backspace {iteration}', cursor_column=0)
            ui.send(b'\x1b[3~')
            expected = baseline[1:]
            visible(f'Delete {iteration}', cursor_column=0)
            ui.send(b'\x1a')
            expected = baseline
            visible(f'undo Delete {iteration}', cursor_column=0)

        prefix = 'head\t猫e\u0301\n    middle\n'
        ui.paste(prefix)
        expected = prefix + baseline
        visible('multiline paste', cursor_row=2, cursor_column=0)
        # Reach the last line using real navigation, then paste beyond the
        # viewport height. Unique row labels make stale/duplicated rows fail.
        ui.send(b'\x1b[B' * 100 + b'\x1b[F')
        width = len(viewport()[0][0])
        long_line = 'a' * (width - 1) + '猫' + 'b' * (width - 3) + 'Z'
        addition = '\n' + '\n'.join(f'row-{i:02d}\t猫 λ' for i in range(55)) + '\n' + long_line
        ui.paste(addition)
        expected += addition
        last = len(expected.split('\n')) - 1
        column = len(source_cells(long_line))
        visible('vertical/horizontal scroll and left half-wide edge', last, column)
        ui.send(b'\x1b[H')
        visible('long line Home and right half-wide edge', last, 0)
        ui.send(b'\x1b[F')
        visible('long line End', last, column)
        ui.send(b'\x1b[D' * 5)
        visible('horizontal navigation', last, column - 5)
        ui.resize(38, 140)
        visible('scrolled resize smaller', last, column - 5)
        ui.resize(44, 180)
        visible('scrolled resize restored', last, column - 5)
        ui.action('Focus output')
        ui.action('Focus editor')
        visible('scrolled focus roundtrip', last, column - 5)
        ui.send(b'\x1b[H')
        visible('scrolled Home before boundary checks', last, 0)
        row = last
        # Observe every transition: a final-only assertion would miss a viewport
        # that jumps early and later happens to converge on the expected top.
        for step in range(8):
            ui.send(b'\x1b[A')
            row -= 1
            visible(f'upward boundary {step}', row, 0)
        for step in range(8):
            ui.send(b'\x1b[B')
            row += 1
            visible(f'downward boundary {step}', row, 0)
        ui.send(b'\x1b[A' * 8)
        row -= 8
        visible('scrolled edit boundary', row, 0)
        lines = expected.split('\n')
        original_line = lines[row]
        ui.send(b'X')
        lines[row] = 'X' + original_line
        expected = '\n'.join(lines)
        visible('scrolled insert', row, 1)
        ui.send(b'\x7f')
        lines[row] = original_line
        expected = '\n'.join(lines)
        visible('scrolled backspace', row, 0)
        ui.send(b'\x1b[3~')
        lines[row] = original_line[1:]
        expected = '\n'.join(lines)
        visible('scrolled Delete', row, 0)
        ui.send(b'\x1a')
        lines[row] = original_line
        expected = '\n'.join(lines)
        visible('scrolled undo Delete', row, 0)
        ui.send(b'\t')
        lines[row] = '    ' + original_line
        expected = '\n'.join(lines)
        visible('scrolled indent', row, 4)
        ui.send(b'\x1b[Z')
        lines[row] = original_line
        expected = '\n'.join(lines)
        visible('scrolled dedent', row, 0)
        # Exercise reduced margins at genuinely small editor heights, and
        # retain state when growing again rather than bottom-pinning anew.
        # Without nested editor chrome, 12 terminal rows (rather than 14)
        # are needed to retain the reduced-margin source-height coverage.
        ui.resize(12, 100)
        assert 1 <= len(viewport()[0]) <= 4, ui.screen.text()
        visible('small scrolled resize', row, 0)
        for step in range(4):
            ui.send(b'\x1b[A')
            row -= 1
            visible(f'small upward boundary {step}', row, 0)
        for step in range(4):
            ui.send(b'\x1b[B')
            row += 1
            visible(f'small downward boundary {step}', row, 0)
        ui.resize(44, 180)
        visible('small resize restored with state', row, 0)
        ui.send(b'\x1b[A' * 100 + b'\x1b[H')
        visible('navigate back to document start', 0, 0)
        ui.send(b'\x13')
        ui.command(':scope save')
        assert path.read_bytes() == expected.encode(), (
                    f'extended App save mismatch: actual={path.read_bytes()[:120]!r}; '
                    f'expected={expected.encode()[:120]!r}; visual failures={failures}'
                )
        print(f'EVIDENCE extended App save: {len(expected.encode())} bytes, {last + 1} lines exact', flush=True)
        assert not failures, '\n'.join(failures)
        ui.send(b'\x11')
    finally:
        ui.close()
        launcher_dir.cleanup()
        signal.alarm(0)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    args, rest = parser.parse_known_args()
    if args.binary:
        result = unittest.TextTestRunner(verbosity=2).run(
            unittest.defaultTestLoader.loadTestsFromTestCase(ScreenTests))
        if not result.wasSuccessful():
            raise SystemExit(1)
        main(args.binary.resolve())
    else:
        unittest.main(argv=[__file__] + rest)
