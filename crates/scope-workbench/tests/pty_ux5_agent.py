#!/usr/bin/env python3
"""Actual root-launcher PTY pending/completion walkthrough; loopback mock only.

Run: python3 -B crates/scope-workbench/tests/pty_ux5_agent.py
The mock response is gated until pending assertions pass, not merely sleep-delayed.
"""
import http.server
import re
import signal
import threading
import time

from pty_ux3_documents import CargoWorkspace, ready, require, transcript
from pty_ux3b_agent import Model, check


class DelayedModel(Model):
    def do_POST(self):
        self.server.arrived.set()
        if not self.server.release.wait(30):
            self.send_error(504, 'PTY driver did not release response')
            return
        super().do_POST()


def main():
    def timeout(*_):
        raise TimeoutError('UX5 240-second deadline')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(240)
    server = http.server.HTTPServer(('127.0.0.1', 0), DelayedModel)
    server.requests = []
    server.arrived = threading.Event()
    server.release = threading.Event()
    flags = ['--agent-local-binding', '--agent-model', 'ux5-mock',
             '--agent-base-url', f'http://127.0.0.1:{server.server_port}/v1',
             '--allow-read', '--allow-edit']
    # Match UX3b: fork the actual cargo launcher before starting the HTTP thread.
    ui = CargoWorkspace(agent_flags=flags)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        ready(ui)
        assert not server.requests and not server.arrived.is_set(), 'startup dispatched'
        ui.action('New Script')
        ui.paste('print("ux5-human"); 42')
        ui.send(b'\x13')
        baseline = ui.screen.text()
        source = re.search(r'Selected scratch (\d+)', baseline).group(1)
        revision = re.search(r'Revision: RevisionId\((\d+)\)', baseline).group(1)
        print(f'EVIDENCE selected committed {source}; {revision}', flush=True)

        def delayed(action, name, expected, previous_requests):
            server.arrived.clear()
            server.release.clear()
            ui.action(action)
            started = time.monotonic()
            screen = ui.send(b'explain active source\r')
            assert server.arrived.wait(5), 'mock did not receive request'
            require(screen, f'Pending {name}')
            require(screen, f'source {source}, revision {revision} (committed)')
            require(screen, 'UI waits synchronously until dispatch returns')
            require(screen, 'input and cancellation unavailable')
            require(screen, 'No progress updates; request may still fail validation.')
            assert len(server.requests) == previous_requests, 'response escaped gate'
            # Read again without sending input: pending must remain while held.
            time.sleep(.5)
            require(ui.read(), f'Pending {name}')
            print(f'EVIDENCE {name}: pending visible with response withheld; {source}; '
                  f'held {time.monotonic() - started:.2f}s; honest synchronous limitations', flush=True)
            server.release.set()
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                screen = ui.read()
                if 'F4 command' in screen.splitlines()[-1]:
                    break
            else:
                raise AssertionError('completion did not restore footer:\n' + screen)
            assert f'Pending {name}' not in screen, screen
            output = transcript(ui)
            require(output, expected)
            # Keep actual source/result rows in the command's visible transcript.
            print('EVIDENCE completion rows:\n' + '\n'.join(dict.fromkeys(
                row.strip() for row in output.splitlines()
                if any(term in row.lower() for term in
                       ('ux3b', 'source', 'revision', 'complete', 'proposal', 'model', 'agent')))), flush=True)
            return output

        ask = delayed('Ask Agent', 'document-ask', 'UX3B inspected committed Script', 0)
        require(ask, 'Source: SourceId(0)')
        require(ask, f'Revision: RevisionId({revision})')
        assert len(server.requests) == 2, server.requests
        proposal = delayed('Propose Edit', 'document-propose', 'source proposal 1 ready', 2)
        require(proposal, f'target: owner=shell-buffer source=Scratch({source}) version={revision}')
        assert len(server.requests) == 3, server.requests
        check(ui, ':scope source-proposal 1', 'source proposal 1: buffer')
        check(ui, ':scope source-diff 1', r'+print(\"accepted\"); 43')
        check(ui, ':scope show', 'print("ux5-human"); 42')
        for path, request in server.requests:
            assert path == '/v1/chat/completions', path
            assert request['model'] == 'ux5-mock', request
        assert 'ux5-human' in str(server.requests[1]), server.requests[1]
        assert len(server.requests) == 3, 'review dispatched a model'
        print('PASS: UX5 actual cargo PTY; ask + proposal pending/completion; '
              '3 loopback requests; committed source unchanged; no live model', flush=True)
    finally:
        server.release.set()
        ui.close()
        server.shutdown()
        worker.join(3)
        server.server_close()
        signal.alarm(0)


if __name__ == '__main__':
    main()
