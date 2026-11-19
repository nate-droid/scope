#!/usr/bin/env python3
"""Real imported-producer launcher acceptance; stdlib PTY transport, no model calls.

Build with cargo run/build first, then run from any directory. Assertions inspect
fresh forced-redraw text, not reconstructed screen cells or historical stdout.
"""
import argparse
import fcntl
import os
from pathlib import Path
import pickle
import re
import select
import shlex
import struct
import sys
import tempfile
import termios
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'crates/scope-ide/tests'))
from pty_editable_workspace import Workspace


def compact(text):
    return re.sub(r'\s+', '', text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifact', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/scope-imported-producer')
    parser.add_argument('--loom', action='store_true', help='inspect the opt-in Loom CZ computations')
    args = parser.parse_args()
    names = (['numerator_00', 'numerator_01', 'numerator_10', 'numerator_11', 'sqrt2_exponent']
             if args.loom else ['free_shipping', 'items', 'shipping_cents', 'subtotal_cents', 'total_cents'])
    before = [1, 1, 1, -1, 2] if args.loom else [False, 3, 500, 3600, 4100]
    after = [1, 1, 1, 1, 2] if args.loom else [False, 5, 250, 4500, 4750]
    changed_index = 3 if args.loom else 4
    artifact = args.artifact.resolve(strict=True)
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='scope-imported-producer-') as root:
        wrapper = Path(root) / 'launcher'
        wrapper.write_text('#!/bin/sh\ncd ' + shlex.quote(root) + '\nexec '
                           + shlex.quote(str(binary)) + ' ' + shlex.quote(str(artifact))
                           + (' --loom' if args.loom else '') + ' --tui\n')
        wrapper.chmod(0o700)
        workspace = Workspace(wrapper)
        # Two real contained Wasm computations precede the interactive journey.
        workspace.deadline = time.monotonic() + 300
        try:
            def redraw():
                output = ''
                for width in (221, 220):
                    fcntl.ioctl(workspace.slave, termios.TIOCSWINSZ,
                                struct.pack('HHHH', 50, width, 0, 0))
                    output += workspace.read(timeout=.15)
                return output

            def check(marker, output=''):
                output += redraw()
                assert marker in compact(output), (marker, output)
                assert 'Inspectcurrentdocument?' not in compact(output), output
                return output

            def action(query):
                workspace.send(b'\x1bOP')
                workspace.send(query.encode())
                return workspace.send(b'\r')

            workspace.expect('Ctrl-Oopen')
            assert b'\x1b[?1049h' in workspace.raw
            if args.loom:
                # Discover the ordinary registered Run action without authorizing
                # another execution or replacing either retained observation.
                check('Runregisteredclosure?', action('Run'))
                workspace.send(b'\x1b')
                print('EVIDENCE registered Run discoverable; review cancelled without dispatch', flush=True)
            comparison = check('CompareResults', action('Compare Results'))
            for marker in ('Baseline:STALE', 'Latest:CURRENT', 'Comparison:changed',
                           'Registeredclosureobservation', 'Structuralcomparisononly'):
                assert marker in compact(comparison), (marker, comparison)

            # Traverse actual changed leaves, not just the comparison summary.
            for _ in range(25):
                output = redraw()
                if f'Selectedpath:$/value/fields/{changed_index}/value/value' in compact(output):
                    assert f'Old:{before[changed_index]}' in compact(output) and f'New:{after[changed_index]}' in compact(output), output
                    break
                workspace.send(b'n')
            else:
                raise AssertionError('computed changed leaf not found in Compare')
            print(f'EVIDENCE Compare $/value/fields/{changed_index}/value/value: '
                  f'{before[changed_index]} -> {after[changed_index]}', flush=True)

            # Open each side through real Compare shortcuts and depth-first walk
            # the canonical record. Right expands; Down advances one visible row.
            for side, freshness, expected in (
                (b'1', 'STALE', before),
                (b'2', 'CURRENT', after),
            ):
                output = check('ResultExplorer', workspace.send(side))
                assert freshness in output, output
                down, right, home = b'\x1b[B', b'\x1b[C', b'\x1b[H'
                # Expand root -> value -> fields, then each record field.
                workspace.send(home + right + down * 2 + right + down + right)
                for i, (name, value) in enumerate(zip(names, expected)):
                    # Each previously expanded field contributes five visible rows.
                    workspace.send(home + down * (4 + 5 * i) + right + down)
                    output = check(f'Selectedpath:$/value/fields/{i}/name')
                    assert f'Selectedvalue:"{name}"' in compact(output), output
                    workspace.send(down + right + down * 2)
                    output = check(f'Selectedpath:$/value/fields/{i}/value/value')
                    assert f'Selectedvalue:{str(value).lower()}' in compact(output), output
                print(f'EVIDENCE Explorer side={side.decode()} {freshness}: names and values {expected}', flush=True)
                details = check('fullretainedevidence', workspace.send(b'd'))
                assert 'Registeredclosureobservation' in compact(details), details
                assert 'Invocationfeedback(notaScopevalidationcertificate)' in compact(details), details
                check('CompareResults', workspace.send(b'\x1b'))

            # Passive Enter and navigation preserve the pair, with no execution.
            check('CompareResults', workspace.send(b'\rp\x1b[B\x1b[A'))
            workspace.send(b'\x1b')
            workspace.send(b'\x1b')
            quitting = workspace.send(b'\x11')
            # Registered source documents are in-memory, destination-free copies.
            if 'Unsaved' in quitting:
                workspace.send(b'\x11')
            assert workspace.wait_exit() == 0, quitting
            output = workspace.read(timeout=2)
            transcript = workspace.text(workspace.raw)
            assert 'Launcher activity: authorizations=0 dispatches=0' in transcript, (output, transcript[-3000:])
            assert select.select([workspace.restore_read], [], [], 1)[0]
            assert pickle.loads(os.read(workspace.restore_read, 65536)) == workspace.original
            assert b'\x1b[?1049l' in workspace.raw and b'\x1b[?25h' in workspace.raw
            print('PASS: real Explorer/Compare/Details; browsing authorizations=0 dispatches=0; terminal restored', flush=True)
        finally:
            workspace.close()


if __name__ == '__main__':
    main()
