#!/usr/bin/env python3
"""Focused H4 ordinary-launcher keyboard path; no shell or model seam."""
import shlex
import sys
import tempfile
from pathlib import Path

from pty_editable_workspace import Workspace


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
        try:
            workspace.expect('Read-onlyinspector')
            assert b'\x1b[?1049h' in workspace.raw
            workspace.expect('Open document:', workspace.send(b'\x0f'))
            workspace.send(b'fixture.h4\r')
            # Alt-Enter is ESC+CR on conventional terminals. Check keeps the
            # editor active and must not invoke candidate evaluation.
            checked = workspace.send(b'\x1b\r')
            assert 'H4_EXACT_SOURCE_OK' not in checked, repr(checked)
            assert 'scope> ' not in checked, repr(checked)
            assert source.read_text() == 'original'
            workspace.send(b'\x04')  # App editor Ctrl-D is not shell EOF.
            workspace.send_until(b'\x1bOQ', 'scope> scope> ')
            workspace.send_until(b':scope quit\r', 'Read-onlyinspector')
            workspace_start = len(workspace.raw)
            # Esc must neither hand off to the REPL nor leave alternate screen.
            escaped = workspace.send(b'\x1b')
            assert 'scope> ' not in escaped, repr(escaped)
            assert b'\x1b[?1049l' not in workspace.raw[workspace_start:], 'Esc left the workspace'
            workspace.send(b'\x1b[200~published-draft-\x1b[201~')
            workspace.send(b'\x13')  # publish revision 1, never save
            assert source.read_text() == 'original', 'Ctrl-S saved to disk'
            workspace.send(b'\x1b[200~run-draft-\x1b[201~')
            review = workspace.expect(
                'Runcurrentprogram?', workspace.send(b'\x12'))
            assert 'published-draft-run-draft-original' in review, repr(review)
            assert 'H4_EXACT_SOURCE_OK' not in review, 'Run executed before confirmation'
            assert source.read_text() == 'original', 'Run review saved to disk'
            result = workspace.send(b'\r')  # authorize exactly the captured revision 2
            workspace.expect('H4_EXACT_SOURCE_OK', result)
            assert source.read_text() == 'original', 'Ctrl-R saved to disk'
            workspace.send(b'\x04')  # Ctrl-D must not leave/toggle the modal.
            workspace.send(b'\x1b')  # dismiss retained read feedback
            escaped = workspace.send(b'\x1b')  # bare workspace Esc also stays
            assert 'scope> ' not in escaped, repr(escaped)
            assert b'\x1b[?1049l' not in workspace.raw[workspace_start:], 'Esc left the workspace'
            workspace.expect('Actions:', workspace.send(b'\x07'))
            workspace.send(b'\x1b')  # Ctrl-G picker cancellation stays in workspace
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
            print('H4 Ctrl-O/S/R/G, explicit Run confirmation, workspace Esc and dirty quit '
                              'cancellation/confirmation passed; child verified exact revision 2 once')
        except BaseException:
            print(bytes(workspace.raw[-16000:]), file=sys.stderr)
            raise
        finally:
            workspace.close()


if __name__ == '__main__':
    main()
