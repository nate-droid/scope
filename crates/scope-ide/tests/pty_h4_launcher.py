#!/usr/bin/env python3
"""Focused H4 ordinary-launcher keyboard path; no shell or model seam."""
import os
import pickle

import select
import shlex
import sys
import tempfile
import time
from pathlib import Path

from pty_editable_workspace import Workspace

# Reconstruct bounded visible cells at the transport's actual terminal geometry.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scope-workbench/tests'))
from visible_screen import VisibleScreen


def main():
    with tempfile.TemporaryDirectory(prefix='scope-h4-launcher-') as root:
        source = Path(root) / 'fixture.h4'
        source.write_text('original')
        wrapper = Path(root) / 'launcher'
        wrapper.write_text(
            '#!/bin/sh\nexport SCOPE_H4_PTY_CHILD=1\n'
            f'cd {shlex.quote(root)}\n'
            f'exec {shlex.quote(sys.argv[1])} --exact h4_launcher_child --ignored --nocapture\n'
        )
        wrapper.chmod(0o700)
        workspace = Workspace(wrapper)
        # Run/Check and negative observations retain the transport's full waits;
        # allow headroom for the longer journey, still bounded.
        workspace.deadline = time.monotonic() + 240
        screen = VisibleScreen(rows=50, cols=220)
        rendered_bytes = 0

        def rendered():
            nonlocal rendered_bytes
            screen.feed(bytes(workspace.raw[rendered_bytes:]))
            rendered_bytes = len(workspace.raw)
            return screen.text()

        def send_frame(keys, predicate):
            # A title can precede the rest of a diff. Require the renderer's
            # complete cursor trailer as well as the gesture's visible state.
            start = len(workspace.raw)

            def completed(_):
                visible = rendered()
                reset = b'\x1b[39m\x1b[49m\x1b[59m\x1b[0m'
                cursor = f'\x1b[?25h\x1b[{screen.row + 1};{screen.col + 1}H'.encode()
                output = bytes(workspace.raw[start:])
                return (output.endswith((reset + b'\x1b[?25l', reset + cursor))
                        and predicate(visible))

            assert time.monotonic() < workspace.deadline, 'H4 scenario deadline exceeded'
            os.write(workspace.fd, keys)
            output = workspace.read(completed=completed)
            assert completed(output), (keys, rendered())
            return output

        def path_selected(visible, path):
            # A row label alone is not evidence of the current selection.
            return any(line.strip(' │') == f'Selected path: ${path}'
                       for line in visible.splitlines())

        try:
            workspace.expect('Read-onlyinspector')
            assert b'\x1b[?1049h' in workspace.raw
            # Positive transitions use complete visible frames, not idle silence.
            # Keep send() below for the existing negative-observation windows.
            send_frame(b'\x0f', lambda v: 'Open document:' in v)
            send_frame(b'fixture.h4\r',
                lambda v: 'original' in v and 'Ctrl-O Open · Ctrl-R Run' in v,
            )
            quiet = rendered()
            assert 'original' in quiet, quiet
            assert 'Ctrl-O Open · Ctrl-R Run' in quiet, quiet
            for detail in ('publish', 'revision', 'source id', 'source_id', 'Ctrl-S'):
                assert detail.lower() not in quiet.lower(), (detail, quiet)
            # Discover Check without a shortcut; cancel a no-match query first.
            workspace.expect('Actions:', send_frame(b'\x07', lambda v: 'Actions:' in v and 'Esc cancel' in v))
            send_frame(b'not-an-action',
                lambda v: 'Actions: not-an-action' in v and 'No matching actions' in v,
            )
            assert 'No matching actions' in rendered(), rendered()
            send_frame(b'\x15ChEcK', lambda v: 'Actions: ChEcK' in v and 'Check' in v)
            assert 'Check' in rendered(), rendered()
            workspace.send(b'\r')
            assert source.read_text() == 'original'
            # Retain the conventional Alt-Enter compatibility-alias exercise.
            # Actual Ctrl-T draft publication/visible diagnostics are checked below.
            checked = workspace.send(b'\x1b\r')
            assert 'H4_EXACT_SOURCE_OK' not in checked, repr(checked)
            assert 'scope> ' not in checked, repr(checked)
            assert source.read_text() == 'original'
            workspace.send(b'\x04')  # App editor Ctrl-D is not shell EOF.
            workspace.send_until(b'\x1bOQ', 'scope> scope> ')
            workspace.send_until(b':scope quit\r', 'Ctrl-OOpen·Ctrl-RRun')
            workspace_start = len(workspace.raw)
            # Esc must neither hand off to the REPL nor leave alternate screen.
            escaped = workspace.send(b'\x1b')
            assert 'scope> ' not in escaped, repr(escaped)
            assert b'\x1b[?1049l' not in workspace.raw[workspace_start:], 'Esc left the workspace'
            send_frame(b'\x1b[200~published-draft-\x1b[201~',
                lambda v: 'published-draft-original' in v,
            )
            workspace.send(b'\x13')  # publish revision 1, never save
            assert source.read_text() == 'original', 'Ctrl-S saved to disk'
            send_frame(b'\x1b[200~run-draft-\x1b[201~',
                lambda v: 'published-draft-run-draft-original' in v,
            )
            workspace.send(b'\x12')  # Run captured revision 2 directly, no Enter.
            result = rendered()
            assert any(line.strip(' │') == '16' for line in result.splitlines()), result
            assert 'Run current program?' not in result, result
            assert 'H4_EXACT_SOURCE_OK' not in result, 'opaque evidence leaked into summary'
            assert 'STALE' not in result, result
            assert source.read_text() == 'original', 'Ctrl-R saved to disk'
            # Inspect the retained native result, never authorize another Run.
            workspace.expect('Actions:', send_frame(b'\x07', lambda v: 'Actions:' in v and 'Esc cancel' in v))
            send_frame(b'result.inspect\r',
                lambda v: 'Result Explorer' in v and 'path' in v.lower(),
            )
            explorer = rendered()
            assert 'Result Explorer' in explorer, explorer
            assert '/evidence' not in explorer, explorer  # root starts collapsed
            assert '/nested' not in explorer, explorer
            # Right expands the selected root.
            send_frame(b'\x1b[C',
                lambda v: 'evidence' in v and 'nested' in v,
            )
            expanded = rendered()
            assert 'evidence' in expanded and 'nested' in expanded, expanded

            def selected_path(path):
                visible = rendered()
                assert path_selected(visible, path), (path, visible)
                return visible

            # Down selects the first object field.
            send_frame(b'\x1b[B',
                lambda v: path_selected(v, '/evidence'),
            )
            selected_path('/evidence')
            send_frame(b'\x1b[B', lambda v: path_selected(v, '/nested'))
            selected_path('/nested')
            # Enter expands nested, but keeps it selected.
            send_frame(b'\r',
                lambda v: '"items": array' in v,
            )
            selected_path('/nested')
            send_frame(b'\x1b[B', lambda v: path_selected(v, '/nested/items'))
            selected_path('/nested/items')
            send_frame(b'\r', lambda v: '[0]:' in v and '[1]:' in v)  # Expand the nested array.
            selected_path('/nested/items')
            send_frame(b'\x1b[B', lambda v: path_selected(v, '/nested/items/0'))
            selected_path('/nested/items/0')
            send_frame(b'\x1b[B', lambda v: path_selected(v, '/nested/items/1'))
            leaf = selected_path('/nested/items/1')
            assert 'leaf' in leaf, leaf
            # Back to the array container.
            send_frame(b'\x1b[A\x1b[A',
                lambda v: path_selected(v, '/nested/items'),
            )
            selected_path('/nested/items')
            # Enter also collapses an expanded container.
            send_frame(b'\r',
                lambda v: '▶ "items": array' in v and '[0]:' not in v and '[1]:' not in v,
            )
            collapsed = selected_path('/nested/items')
            assert '/nested/items/0' not in collapsed, collapsed
            assert '/nested/items/1' not in collapsed, collapsed
            workspace.send(b'\x1b')  # Esc returns to the normal semantic modal.
            normal = rendered()
            assert '┌Result Explorer' not in normal, normal
            assert 'Semantic inspection' in normal, normal
            assert 'd toggles Details' in normal, normal
            send_frame(b'd', lambda v: 'H4_EXACT_SOURCE_OK' in v and 'leaf' in v)
            details = rendered()
            assert 'H4_EXACT_SOURCE_OK' in details, details
            assert 'leaf' in details, details
            send_frame(b'd', lambda v: 'Semantic inspection' in v and 'H4_EXACT_SOURCE_OK' not in v)
            summary = rendered()
            assert 'H4_EXACT_SOURCE_OK' not in summary, summary
            workspace.send(b'\x1b')  # Close inspection and restore editor focus.
            assert source.read_text() == 'original', 'result inspection saved to disk'
            # Details is also discoverable without entering evaluation.
            workspace.expect('Actions:', send_frame(b'\x07', lambda v: 'Actions:' in v and 'Esc cancel' in v))
            send_frame(b'details\r', lambda v: 'H4_EXACT_SOURCE_OK' in v and 'leaf' in v)
            assert 'H4_EXACT_SOURCE_OK' in rendered(), rendered()
            workspace.send(b'\x1b')
            # No further focus key: typing must still edit the document.
            send_frame(b'\x1b[200~after-run-\x1b[201~',
                lambda v: 'after-run-' in v and 'STALE (unpublished editor draft)' in v
                and any(line.strip(' │') == '16' for line in v.splitlines()),
            )
            stale = rendered()
            assert 'after-run-' in stale, stale
            assert 'STALE (unpublished editor draft)' in stale, stale
            assert any(line.strip(' │') == '16' for line in stale.splitlines()), stale
            # Start with an unpublished error draft. Ctrl-T must publish/check
            # that exact source, not the prior Run revision or the file on disk.
            send_frame(b'\x1b[200~check-error-\x1b[201~',
                lambda v: 'published-draft-run-draft-after-run-check-error-original' in v,
            )
            draft = rendered()
            assert 'published-draft-run-draft-after-run-check-error-original' in draft, draft
            error_cursor = (screen.row, screen.col)
            assert screen.cells[screen.row][screen.col] == 'o', draft
            workspace.send(b'\x14')  # actual Ctrl-T control byte
            checked = rendered()
            assert (screen.row, screen.col) == error_cursor, checked
            assert '1:37:49: h4: error: remove check-error-' in checked, checked
            for absent in ('No errors', 'Check document diagnostics', 'revision', 'scope> ',
                           'Run current program?', 'H4_EXACT_SOURCE_OK', 'STALE'):
                assert absent not in checked, (absent, checked)
            assert not any(line.strip(' │') == '16' for line in checked.splitlines()), checked
            assert source.read_text() == 'original', 'Ctrl-T saved error draft'

            # Typing/deleting without a focus gesture proves Check left editing
            # active; the next Check must clear the previous diagnostic cells.
            send_frame(b'\x7f' * len('check-error-'),
                lambda v: 'published-draft-run-draft-after-run-original' in v,
            )
            clean_draft = rendered()
            assert 'published-draft-run-draft-after-run-original' in clean_draft, clean_draft
            clean_cursor = (screen.row, screen.col)
            assert clean_cursor == (error_cursor[0], error_cursor[1] - len('check-error-'))
            assert screen.cells[screen.row][screen.col] == 'o', clean_draft
            workspace.send(b'\x14')
            clean = rendered()
            assert (screen.row, screen.col) == clean_cursor, clean
            assert any(line.strip(' │') == 'No errors' for line in clean.splitlines()), clean
            for absent in ('remove check-error-', 'Check document diagnostics', 'revision',
                           'scope> ', 'Run current program?', 'H4_EXACT_SOURCE_OK', 'STALE'):
                assert absent not in clean, (absent, clean)
            assert source.read_text() == 'original', 'Ctrl-T saved clean draft'
            workspace.send(b'\x04')  # App editor Ctrl-D is still not shell EOF.
            escaped = workspace.send(b'\x1b')  # bare workspace Esc also stays
            assert 'scope> ' not in escaped, repr(escaped)
            assert b'\x1b[?1049l' not in workspace.raw[workspace_start:], 'Esc left the workspace'
            workspace.expect('Actions:', send_frame(b'\x07', lambda v: 'Actions:' in v and 'Esc cancel' in v))
            actions = rendered()
            assert 'More actions' in actions, actions
            assert 'Run selected Script' not in actions, actions
            send_frame(b'workspace.more-actions\r',
                lambda v: 'More actions' in v and 'Run selected Script' in v,
            )
            advanced = rendered()
            assert 'More actions' in advanced, advanced
            assert 'Run selected Script' in advanced, advanced
            workspace.send(b'\x1b')  # Advanced picker cancellation stays in workspace
            workspace.expect('Unsaved', workspace.send(b'\x11'))
            cancelled = workspace.send(b'\x1b')
            assert 'scope> ' not in cancelled, repr(cancelled)
            assert b'\x1b[?1049l' not in workspace.raw[workspace_start:], 'cancelled quit left workspace'
            # Cancellation resets the grant: another first Ctrl-Q must review again.
            workspace.expect('Unsaved', workspace.send(b'\x11'))
            assert b'\x1b[?1049l' not in workspace.raw[workspace_start:], 'first Ctrl-Q discarded work'
            workspace.send(b'\x11')
            assert workspace.wait_exit() == 0, bytes(workspace.raw)
            assert source.read_text() == 'original'
            assert b'\x1b[?1049l' in workspace.raw, 'Ctrl-Q did not restore terminal'
            assert b'\x1b[?25h' in workspace.raw, 'Ctrl-Q did not show cursor'
            assert select.select([workspace.restore_read], [], [], 1)[0], 'missing terminal attributes'
            restored = pickle.loads(os.read(workspace.restore_read, 65536))
            assert restored == workspace.original, 'terminal attributes were not restored'
            print('H4 Ctrl-O/S/R/T/G: direct Run, visible scalar/error/No errors cells, retained Check cursor, editor focus/stale '
                  'feedback, nested Result Explorer navigation and retained Details, advanced access, no save, dirty quit cancellation/confirmation '
                  'and terminal restoration passed; child verified exact Run revision 2 once and Check revisions 3/4')
        except BaseException:
            print(bytes(workspace.raw[-16000:]), file=sys.stderr)
            raise
        finally:
            workspace.close()


if __name__ == '__main__':
    main()
