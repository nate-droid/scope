#!/usr/bin/env python3
"""Opt-in live-model experiment; normal launcher, no mock/fallback/provider startup.

Run from repository root after building with --features agent:
python3 crates/scope-workbench/tests/pty_ux4_live.py --evidence-dir /existing/directory
Evidence contains source, intentions and workbench output; review before sharing.
By default proposals are only reviewed. --accept-reviewed exercises explicit
source-accept commands ONLY for the exact two candidates reviewed in the first
UX4 session. It is an experiment-driver opt-in, not product auto-accept behavior.
"""
import argparse
import json
from pathlib import Path
import signal
import time

from pty_ux3_documents import CargoWorkspace, ready, require


def command_completed(screen):
    """Only the restored footer proves the synchronous command returned."""
    lines = screen.splitlines()
    return bool(lines) and 'F4 command' in lines[-1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence-dir', type=Path, required=True)
    parser.add_argument('--model', default='qwen3-30b-a3b')
    parser.add_argument('--base-url', default='http://127.0.0.1:1234/v1')
    parser.add_argument('--accept-reviewed', action='store_true',
                        help='exercise acceptance only of the exact previously reviewed UX4 candidates')
    args = parser.parse_args()
    if not args.evidence_dir.is_dir():
        parser.error('evidence directory must already exist')
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('live experiment deadline')))
    signal.alarm(600)
    ui = CargoWorkspace(agent_flags=['--agent-local-binding', '--agent-model', args.model,
                                    '--agent-base-url', args.base_url, '--allow-read', '--allow-edit'])
    events = []

    def capture(label):
        ui.action('Focus output')
        ui.send(b'\x1b[6~' * 100)
        frames = [ui.screen.text()]
        for _ in range(220):
            before = ui.screen.text()
            frame = ui.send(b'\x1b[A')
            if frame == before:
                break
            frames.append(frame)
        # Retain one complete frame for identity/revision and bounded unique output
        # rows rather than megabytes of almost-identical scroll frames.
        # Deduplicated output rows are a reading aid, not a protocol trace.
        rows = []
        for frame in reversed(frames):
            section = frame.split('┌output', 1)[-1]
            for line in section.splitlines()[1:-1]:
                if line not in rows:
                    rows.append(line)
        text = '\n'.join(rows)
        (args.evidence_dir / (label + '.txt')).write_text(frames[0] + '\n\nOUTPUT ROWS (deduplicated, not a protocol trace):\n' + text)
        print(label + '\n' + text[-2400:], flush=True)
        return text

    def command(text, label, live=False):
        start = time.monotonic()
        ui.command(text)
        if live:
            # The command executes synchronously; wait for the workbench footer to
            # return and redraw after the busy command bar disappears.
            deadline = start + 150
            while time.monotonic() < deadline:
                screen = ui.read()
                if command_completed(screen):
                    break
                time.sleep(.2)
            else:
                (args.evidence_dir / (label + '-timeout.txt')).write_text(ui.screen.text())
                raise TimeoutError('model request exceeded 150 seconds; no further input sent')
        result = capture(label)
        events.append({'command': text, 'seconds': round(time.monotonic() - start, 2), 'evidence': label + '.txt'})
        (args.evidence_dir / 'events.json').write_text(json.dumps(events, indent=2))
        return result

    def new(source, label):
        ui.action('New Script')
        ui.paste(source)
        ui.send(b'\x13')
        (args.evidence_dir / (label + '.scope')).write_text(source)
        command(':scope show', label + '-source')
        ui.action('Focus editor')
        ui.send(b'\x1b[15~')
        return capture(label + '-run')

    try:
        ready(ui)
        new('let price = 6; let count = 3; let total = price * count; print(total); total + 2', 'A')
        command(':scope document-ask Explain the active Script. Inspect its committed source first. Describe its declarations, printed output and final value. Do not edit or claim to have run it.', 'A-ask', True)
        new('let price = 6; let count = 3; let total = price * count; print(total); total + 2', 'B-working')
        ui.action('Focus editor')
        ui.send(b'\x1b[H')
        old = 'let price = 6; let count = 3; let total = price * count; print(total); total + 2'
        ui.send(b'\x1b[3~' * len(old))
        broken = old.replace('total + 2', 'totla + 2')
        ui.paste(broken)
        ui.send(b'\x13')
        (args.evidence_dir / 'B.scope').write_text(broken)
        command(':scope show', 'B-source')
        ui.action('Focus editor')
        ui.send(b'\x1b[15~')
        capture('B-diagnostic')
        command(':scope document-propose Repair only the misspelled final variable totla to total. F5 printed 18 then reported an undefined variable for totla. Preserve all other source and behavior. Inspect the active source if needed; propose a correction, do not claim execution.', 'B-propose', True)
        diff = command(':scope source-diff 1', 'B-diff')
        command(':scope source-proposal 1', 'B-review')
        if args.accept_reviewed:
            require(diff, '+' + old)
            require(diff, '-' + broken)
            command(':scope source-accept 1', 'B-accept')
            command(':scope show', 'B-after')
            ui.action('Focus editor')
            ui.send(b'\x1b[15~')
            require(capture('B-after-run'), 'final: 20')
        new('let mut n = 6; n = 8; n = 12; n', 'C')
        command(':scope document-propose Simplify this active Script without changing its final value. Remove only redundant assignments; no unrelated rewrite or new syntax. Propose an edit, do not claim execution.', 'C-propose', True)
        diff = command(':scope source-diff 2', 'C-diff')
        command(':scope source-proposal 2', 'C-review')
        if args.accept_reviewed:
            require(diff, '+let mut n = 12; n ')
            require(diff, '-let mut n = 6; n = 8; n = 12; n')
            command(':scope source-accept 2', 'C-accept')
            command(':scope show', 'C-after')
            ui.action('Focus editor')
            ui.send(b'\x1b[15~')
            require(capture('C-after-run'), 'final: 12')
            ui.action('Focus editor')
            ui.send(b'\x1b[F')
            ui.paste(' + 1')
            ui.send(b'\x13')
            ui.send(b'\x1b[15~')
            require(capture('C-continued-run'), 'final: 13')
            command(':scope show', 'C-continued-source')
    finally:
        (args.evidence_dir / 'final-screen.txt').write_text(ui.screen.text())
        ui.close()
        signal.alarm(0)


if __name__ == '__main__':
    main()
