#!/usr/bin/env python3
"""Stdlib-only ordinary-launcher PTY test; no model or private application seam."""

import json

from pathlib import Path
import shlex
import sys
import tempfile
import time

from pty_editable_workspace import Workspace


def main():
    with tempfile.TemporaryDirectory(prefix='scope-source-proposal-launcher-') as root:
        source = Path(root) / 'fixture.txt'
        original = 'original 猫\nsecond\n'
        source.write_text(original)
        wrapper = Path(root) / 'consumer'
        wrapper.write_text(
            '#!/bin/sh\n'
            'export SCOPE_SOURCE_PROPOSAL_PTY_CHILD=1\n'
            f'cd {shlex.quote(root)}\n'
            f'exec {shlex.quote(sys.argv[1])} --exact launcher_child --ignored --nocapture\n'
        )
        wrapper.chmod(0o700)
        workspace = Workspace(wrapper)
        workspace.deadline = time.monotonic() + 300

        try:
            workspace.expect('Read-onlyinspector')
            assert b'\x1b[?1049h' in workspace.raw, 'launcher did not enter TUI'
            workspace.expect('scope> ', workspace.send(b'\x1bOQ'))
            opened = workspace.command(':scope scratch open fixture.txt')
            assert 'error:' not in opened, repr(opened)
            workspace.command(':scope semantic-target 1')
            workspace.expect('Read-onlyinspector', workspace.send(b':scope quit\r'))

            def command(text, expected=None):
                # Every operation opens and submits a fresh F4 command line.
                workspace.send(b'\x1bOS')
                workspace.send(text.encode() + b'\r')
                if expected is None:
                    return
                workspace.send(b'\x1bOS')
                workspace.send(b'pane output\r')
                workspace.send(b'\x1b[6~' * 12 + b'\x1b[A' * 5)
                # CSI stripping is not a screen emulator. Force a full redraw
                # before checking emitted transcript text, not cell positions.
                workspace.expect('scope> ', workspace.send(b'\x1bOQ'))
                output = workspace.expect(
                    'Read-onlyinspector', workspace.send(b':scope quit\r'))
                if expected is not None:
                    assert expected.replace(' ', '') in output.replace(' ', ''), repr(output)
                return output

            def propose(replacement, end):
                payload = {'edits': [{'range': {
                    'start': {'line': 0, 'column': 0},
                    'end': {'line': 0, 'column': end}},
                    'replacement': replacement}], 'rationale': 'deterministic PTY fixture'}
                return ':scope source-propose ' + json.dumps(payload, ensure_ascii=False)

            command(propose('committed é', len('original 猫'.encode())),
                    'source proposal 1 created')
            command(':scope source-proposals', '1 pending source proposals')
            command(':scope source-diff 1', '+committed é')
            command(':scope downstream consumer inspect pending')
            command(':scope source-accept 1', 'source proposal 1 applied')
            command(':scope downstream consumer inspect accepted')
            command(':scope source-accept 1')
            command(':scope downstream consumer inspect duplicate')
            command(propose('REJECTED', len('committed é'.encode())),
                    'source proposal 2 created')
            command(':scope source-reject 2', 'source proposal 2: Rejected')
            command(':scope source-proposals', '0 pending source proposals')
            command(':scope source-accept 2')
            command(':scope downstream consumer inspect rejected')
            workspace.send(b'\x11')
            assert workspace.wait_exit() == 0, bytes(workspace.raw)
            assert source.read_text() == original, 'proposal lifecycle saved the source'
            assert b'\x1b[?1049l' in workspace.raw, 'terminal was not restored'
            print('Separate F4 propose/list/diff/accept, duplicate acceptance, rejection; '
                  'public callback verified exact text and versions 0/1; disk unchanged')
        except BaseException:
            print(bytes(workspace.raw[-16000:]), file=sys.stderr)
            raise
        finally:
            workspace.close()


if __name__ == '__main__':
    main()
