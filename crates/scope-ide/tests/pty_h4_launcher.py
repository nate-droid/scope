#!/usr/bin/env python3
"""Focused H4 ordinary-launcher keyboard path; no shell or model seam."""
import os
import pickle
import select
import shlex
import sys
import tempfile
from pathlib import Path

from pty_editable_workspace import Workspace

# Reuse the stdlib VT model, matching the transport's actual terminal geometry.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scope-workbench/tests'))
from pty_workspace import Screen


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
        screen = Screen(rows=50, cols=220)
        rendered_bytes = 0

        def rendered():
            nonlocal rendered_bytes
            screen.feed(bytes(workspace.raw[rendered_bytes:]))
            rendered_bytes = len(workspace.raw)
            return screen.text()

        try:
            workspace.expect('Read-onlyinspector')
            assert b'\x1b[?1049h' in workspace.raw
            workspace.expect('Open document:', workspace.send(b'\x0f'))
            workspace.send(b'fixture.h4\r')
            quiet = rendered()
            assert 'original' in quiet, quiet
            assert 'Ctrl-O Open · Ctrl-R Run' in quiet, quiet
            for detail in ('publish', 'revision', 'source id', 'source_id', 'Ctrl-S'):
                assert detail.lower() not in quiet.lower(), (detail, quiet)
            # Alt-Enter is ESC+CR on conventional terminals. Check keeps the
            # editor active and must not invoke candidate evaluation.
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
            workspace.send(b'\x1b[200~published-draft-\x1b[201~')
            workspace.send(b'\x13')  # publish revision 1, never save
            assert source.read_text() == 'original', 'Ctrl-S saved to disk'
            workspace.send(b'\x1b[200~run-draft-\x1b[201~')
            workspace.send(b'\x12')  # Run captured revision 2 directly, no Enter.
            result = rendered()
            assert any(line.strip(' │') == '16' for line in result.splitlines()), result
            assert 'Run current program?' not in result, result
            assert 'H4_EXACT_SOURCE_OK' not in result, 'opaque evidence leaked into summary'
            assert 'STALE' not in result, result
            assert source.read_text() == 'original', 'Ctrl-R saved to disk'
            # No focus key or modal dismissal: typing must still edit the document.
            workspace.send(b'\x1b[200~after-run-\x1b[201~')
            stale = rendered()
            assert 'after-run-' in stale, stale
            assert 'STALE (unpublished editor draft)' in stale, stale
            assert any(line.strip(' │') == '16' for line in stale.splitlines()), stale
            workspace.send(b'\x04')  # App editor Ctrl-D is still not shell EOF.
            escaped = workspace.send(b'\x1b')  # bare workspace Esc also stays
            assert 'scope> ' not in escaped, repr(escaped)
            assert b'\x1b[?1049l' not in workspace.raw[workspace_start:], 'Esc left the workspace'
            workspace.expect('Actions:', workspace.send(b'\x07'))
            actions = rendered()
            assert 'More actions' in actions, actions
            assert 'Run selected Script' not in actions, actions
            workspace.send(b'workspace.more-actions\r')
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
            print('H4 Ctrl-O/S/R/G: direct Run, rendered scalar output, editor focus/stale '
                  'feedback, advanced access, no save, dirty quit cancellation/confirmation '
                  'and terminal restoration passed; child verified exact revision 2 once')
        except BaseException:
            print(bytes(workspace.raw[-16000:]), file=sys.stderr)
            raise
        finally:
            workspace.close()


if __name__ == '__main__':
    main()
