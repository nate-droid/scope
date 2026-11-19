#!/usr/bin/env python3
"""Real downstream Compare acceptance; stdlib, temporary fixtures, no Sprout edits.

Build Sprout's scope example against current Scope into Scope target first.
Run: python3 -B crates/scope-ide/tests/pty_sprout_compare.py --sprout /path/to/sprout
Assertions concern emitted text after forced redraws, not screen cell positions.
Native JSON is execution evidence; preview.text is an opaque string, not typed Sprout.
"""
import argparse
import os
import fcntl
from pathlib import Path
import pickle
import re
import select
import shlex
import struct
import tempfile
import termios
import time

from pty_editable_workspace import Workspace


def compact(text):
    return re.sub(r"\s+", "", text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--sprout', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=Path(__file__).resolve().parents[3] / 'target/debug/examples/scope')
    args = parser.parse_args()
    sprout = args.sprout.resolve(strict=True)
    binary = args.binary.resolve(strict=True)
    component = (sprout / 'apps/mainframe/sprout-c2.component.wasm').resolve(strict=True)
    original = (sprout / 'examples/programs/nested_values.sprout').read_bytes()
    with tempfile.TemporaryDirectory(prefix='scope-sprout-compare-') as root:
        source = Path(root) / 'nested_values.sprout'
        source.write_bytes(original)
        wrapper = Path(root) / 'launcher'
        wrapper.write_text('#!/bin/sh\nexport SPROUT_COMPONENT=' + shlex.quote(str(component))
                           + '\nexec ' + shlex.quote(str(binary)) + ' workspace ' + shlex.quote(root) + '\n')
        wrapper.chmod(0o700)
        workspace = Workspace(wrapper)
        # Same 240-second retained-evidence allowance as Sprout's maintained PTY.
        workspace.deadline = time.monotonic() + 240
        try:
            def redraw():
                output = ''
                for width in (221, 220):
                    fcntl.ioctl(workspace.slave, termios.TIOCSWINSZ, struct.pack('HHHH', 50, width, 0, 0))
                    output += workspace.read(timeout=.5)
                return output

            def send(keys):
                return workspace.send(keys)

            def check(marker, output=''):
                output += redraw()
                if marker not in compact(output):
                    output += workspace.read(timeout=30, completed=lambda text: marker in compact(text))
                assert marker in compact(output), (marker, output, workspace.text(workspace.raw)[-12000:])
                return output

            def action(query):
                send(b'\x1bOP')  # F1, not a semantic API or test seam
                send(query.encode())
                return send(b'\r')

            workspace.expect('Ctrl-Oopen')
            check('Opendocument:', send(b'\x0f'))
            send(b'nested_values')
            send(b'\t')
            send(b'\r')
            checked = check('Noerrors', send(b'\x1b\r'))
            assert 'Outcome:' not in checked
            check('Sproutpreview(complete):42', action(''))
            # With just one completed result Compare must remain unavailable.
            unavailable = action('Compare Results') + redraw()
            assert 'twocompletedinspectionresults' in compact(unavailable), unavailable
            send(b'\x1b')

            # Real editor edit: width(make(3,45)) -> width(make(3,50)), 42 -> 47.
            offset = original.decode().index('45))')
            send(b'\x1b[C' * offset)
            send(b'\x1b[3~' * 2)
            send(b'\x1b[200~50\x1b[201~')
            check('Sproutpreview(complete):47', action(''))
            comparison = check('CompareResults', action('Compare Results'))
            for marker in ('Baseline:STALE', 'observedrevision0', 'Latest:CURRENT',
                           'observedrevision1', 'Structuralcomparisononly', 'Comparison:changed'):
                assert marker in compact(comparison), (marker, comparison)

            # Navigate real differences and assert a changed structural leaf.
            found = False
            for _ in range(30):
                output = send(b'n') + redraw()
                normalized = compact(output)
                if 'Selectedpath:$/execution/preview/text' in normalized:
                    assert 'Old:"42"' in normalized and 'New:"47"' in normalized, output
                    found = True
                    print('EVIDENCE $/execution/preview/text: old="42", new="47" (opaque preview strings)', flush=True)
                    break
            assert found, output
            send(b'p')
            send(b'\x1b[B\x1b[A')
            for side, revision in ((b'1', '0'), (b'2', '1')):
                explorer = check('ResultExplorer', send(side))
                assert 'observedrevision' + revision in compact(explorer), explorer
                passive = send(b'\r') + redraw()
                assert 'Inspectcurrentdocument?' not in compact(passive), passive
                check('CompareResults', send(b'\x1b'))
            passive = check('CompareResults', send(b'\r'))
            assert 'Inspectcurrentdocument?' not in compact(passive), passive
            send(b'\x1b')  # comparison -> inspection
            send(b'\x1b')  # inspection -> workspace

            # Review then reject an explicit pending authorization. Neither the
            # cancelled review nor passive Compare may displace the retained pair.
            opened = action('evaluate') + redraw()
            check('explicithostapprovalrequired', opened)
            check('Inspectcurrentdocument?', send(b'\r'))
            send(b'\x1b')
            send(b'\x1b')
            preserved = check('CompareResults', action('Compare Results'))
            for marker in ('Baseline:STALE', 'observedrevision0', 'Latest:CURRENT', 'observedrevision1'):
                assert marker in compact(preserved), (marker, preserved)
            print('EVIDENCE F1 Compare: baseline revision=0 STALE; latest revision=1 CURRENT; '
                  '1/2 explorers, n/p/arrows, passive Enter, cancelled authorization preserve pair', flush=True)
            send(b'\x1b')
            send(b'\x1b')
            check('Unsaved', send(b'\x11'))
            assert source.read_bytes() == original
            send(b'\x11')
            assert workspace.wait_exit() == 0
            assert source.read_bytes() == original
            assert select.select([workspace.restore_read], [], [], 1)[0]
            assert pickle.loads(os.read(workspace.restore_read, 65536)) == workspace.original
            assert b'\x1b[?1049l' in workspace.raw and b'\x1b[?25h' in workspace.raw
            print('PASS: two contained Sprout computations 42 -> 47; disk unchanged; exit=0; terminal restored', flush=True)
        finally:
            workspace.close()


if __name__ == '__main__':
    main()
