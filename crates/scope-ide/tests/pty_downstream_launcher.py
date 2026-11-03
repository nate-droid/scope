#!/usr/bin/env python3
"""Stdlib-only real launcher driver, invoked by downstream_launcher.rs.

Assertions concern emitted text, not reconstructed screen cells. Each picker
check starts with a full redraw; callback assertions verify exact live source.
"""
import os
from pathlib import Path
import shlex
import sys
import tempfile

# Reuse the existing bounded PTY transport (including cursor-query replies,
# terminal sizing and process-group cleanup), not any application test seam.
from pty_editable_workspace import Workspace


def main():
    with tempfile.TemporaryDirectory(prefix='scope-consumer-launcher-') as root:
        source = Path(root) / 'fixture.consumer'
        source.write_text('original-disk-text')
        wrapper = Path(root) / 'consumer'
        wrapper.write_text(
            '#!/bin/sh\n'
            'export SCOPE_CONSUMER_PTY_CHILD=1\n'
            f'cd {shlex.quote(root)}\n'
            f'exec {shlex.quote(sys.argv[1])} --exact launcher_child --ignored --nocapture\n'
        )
        wrapper.chmod(0o700)
        workspace = Workspace(wrapper)
        try:
            workspace.expect('Read-onlyinspector')
            assert b'\x1b[?1049h' in workspace.raw, 'launcher did not enter TUI'

            # The CLI prints a prompt before the raw-mode reader is ready.
            # Its second prompt follows raw-mode setup and the cursor query.
            def shell():
                workspace.send_until(b'\x1bOQ', 'scope> scope> ')

            def command(text):
                output = workspace.command(text)
                return workspace.expect('scope> scope> ', output)

            def resume():
                workspace.send_until(b':scope quit\r', 'Read-onlyinspector')

            def picker():
                compact = workspace.expect('Actions:', workspace.send(b'\x1bOP'))
                assert 'Open' in compact and 'Moreactions' in ''.join(compact.split()), repr(compact)
                # F1 starts compact; downstream registrations live in More actions.
                expanded = workspace.expect('More', workspace.send(b'\x1b[B\r'))
                return ''.join(expanded.split())

            # Commit selected text using shell commands, then deliberately bind
            # a different scratch. Discovery must follow selection, not binding.
            shell()
            opened = command(':scope scratch open fixture.consumer')
            assert 'error:' not in opened, repr(opened)
            command(':scope line 1 "selected-live-text"')
            command(':scope scratch new text')
            command(':scope line 1 "decoy-bound-text"')
            command(':scope semantic-target 2')
            command(':scope scratch select 1')
            resume()
            discovered = picker()
            assert 'Consumerinspectcommittedtext' in discovered, repr(discovered)
            workspace.send(b'Consumer inspect committed text')
            dispatched = workspace.send(b'\r')
            workspace.expect('CONSUMER_DISPATCH_OK_1', dispatched)

            # Same host registration must disappear for a different language.
            shell()
            command(':scope scratch new scope-script')
            resume()
            filtered = picker()
            assert 'Consumerinspectcommittedtext' not in filtered, repr(filtered)
            workspace.send(b'Consumer inspect committed text')
            absent = workspace.send(b'\r')
            assert 'CONSUMER_DISPATCH_OK' not in absent, repr(absent)
            workspace.send(b'\x1b')  # dismiss the empty picker

            # Returning to the original source restores the registered action.
            shell()
            command(':scope scratch select 1')
            resume()
            restored = picker()
            assert 'Consumerinspectcommittedtext' in restored, repr(restored)
            workspace.send(b'Consumer inspect committed text')
            workspace.send(b'\r')
            # The transcript viewport can remain on historical output. Do not
            # confuse visibility with dispatch: the child asserts exactly two
            # calls (and exact source/arguments in each) after the launcher exits.
            workspace.expect('Unsaved', workspace.send(b'\x11'))
            workspace.send(b'\x11')
            assert workspace.wait_exit() == 0, bytes(workspace.raw)
            assert source.read_text() == 'original-disk-text', 'action saved the scratch'
            assert b'\x1b[?1049l' in workspace.raw, 'terminal was not restored'
            print('F1 discovery, language filtering, and exact live-source dispatch passed')
        finally:
            workspace.close()


if __name__ == '__main__':
    main()
