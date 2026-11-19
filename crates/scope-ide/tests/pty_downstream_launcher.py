#!/usr/bin/env python3
"""Stdlib-only real launcher driver, invoked by downstream_launcher.rs.

Picker assertions reconstruct visible cells and wait for complete frames;
callback assertions verify exact live source.
"""
import os
from pathlib import Path
import shlex
import sys
import tempfile
import time

# Reuse the existing bounded PTY transport (including cursor-query replies,
# terminal sizing and process-group cleanup), not any application test seam.
from pty_editable_workspace import Workspace

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scope-workbench/tests'))
from visible_screen import VisibleScreen


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
        screen = VisibleScreen(rows=50, cols=220)
        rendered_bytes = 0

        def rendered():
            nonlocal rendered_bytes
            screen.feed(bytes(workspace.raw[rendered_bytes:]))
            rendered_bytes = len(workspace.raw)
            return screen.text()

        def send_frame(keys, predicate):
            # A picker title can arrive before its rows and the cursor trailer.
            start = len(workspace.raw)

            def completed(_):
                visible = rendered()
                reset = b'\x1b[39m\x1b[49m\x1b[59m\x1b[0m'
                cursor = f'\x1b[?25h\x1b[{screen.row + 1};{screen.col + 1}H'.encode()
                output = bytes(workspace.raw[start:])
                return (output.endswith((reset + b'\x1b[?25l', reset + cursor))
                        and predicate(visible))

            assert time.monotonic() < workspace.deadline, 'downstream scenario deadline exceeded'
            os.write(workspace.fd, keys)
            output = workspace.read(completed=completed)
            assert completed(output), (keys, rendered())
            return rendered()

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

            action = 'Consumer inspect committed text'

            def action_rows(visible):
                # Exclude the title/query echo: only an actual action row counts.
                return [line.strip(' │') for line in visible.splitlines()
                        if line.strip(' │').lstrip('> ').startswith(action)]

            def picker(available):
                send_frame(b'\x1bOP', lambda v:
                    'Actions:' in v and 'Open' in v and 'More actions' in v
                    and 'Esc cancel' in v)
                # F1 starts compact; downstream registrations live in More actions.
                send_frame(b'\x1b[B\r', lambda v:
                    'More actions:' in v and 'Esc cancel' in v)
                # Search before inspecting: the registered action may be below
                # the first page as built-in actions are added.
                def matched(visible):
                    rows = action_rows(visible)
                    if available:
                        result = any(row == f'> {action}' for row in rows)
                        result = result and 'No matching actions' not in visible
                    else:
                        result = 'No matching actions' in visible and not rows
                    return f'More actions: {action}' in visible and result

                return send_frame(action.encode(), matched)

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
            picker(available=True)
            dispatched = workspace.send(b'\r')
            workspace.expect('CONSUMER_DISPATCH_OK_1', dispatched)

            # Same host registration must disappear for a different language.
            shell()
            command(':scope scratch new scope-script')
            resume()
            picker(available=False)
            absent = workspace.send(b'\r')
            assert 'CONSUMER_DISPATCH_OK' not in absent, repr(absent)
            workspace.send(b'\x1b')  # dismiss the empty picker

            # Returning to the original source restores the registered action.
            shell()
            command(':scope scratch select 1')
            resume()
            picker(available=True)
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
