#!/usr/bin/env python3
"""Real root cargo launcher/editor + deterministic loopback provider; stdlib only.

Run: python3 crates/scope-workbench/tests/pty_ux3b_agent.py
No external provider, credentials, or implementation/test-only launcher is used.
"""
import http.server
import json
from pathlib import Path
import signal
import threading

from pty_ux3_documents import CargoWorkspace, ready, require, transcript


class Model(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.requests.append((self.path, request))
        index = len(self.server.requests)
        decision = ({'type': 'tool', 'name': 'language.scope-script.inspect', 'arguments': {}}
                    if index == 1 else
                    {'type': 'complete', 'content': 'UX3B inspected committed Script'}
                    if index == 2 else
                    {'type': 'tool', 'name': 'language.scope-script.replace',
                     'arguments': {'candidate': 'print("accepted"); 43' if index == 3
                                   else '999'}})
        body = json.dumps({'id': 'ux3b', 'object': 'chat.completion', 'created': 0,
                           'model': request['model'], 'choices': [{'index': 0,
                           'message': {'role': 'assistant', 'content': json.dumps(decision)},
                           'finish_reason': 'stop'}], 'usage': {'prompt_tokens': 1,
                           'completion_tokens': 1, 'total_tokens': 2}}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def check(ui, command, expected):
    ui.command(command)
    ui.action('Focus output')
    screen = ui.send(b'\x1b[6~' * 100)
    frames = [screen]
    for _ in range(160):
        if expected in screen:
            break
        before = screen
        screen = ui.send(b'\x1b[A')
        frames.append(screen)
        if screen == before:
            break
    require('\n'.join(frames), expected)
    print(f'EVIDENCE {command}: {expected}', flush=True)


def main():
    def timeout(*_):
        raise TimeoutError('UX3b 240-second deadline')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(240)
    server = http.server.HTTPServer(('127.0.0.1', 0), Model)
    server.requests = []
    # Fork before starting the provider thread.
    ui = CargoWorkspace(agent_flags=[])
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        ready(ui)
        ui.action('New Script')
        ui.paste('42')
        ui.send(b'\x13')
        check(ui, ':scope document-ask explain', 'host has no semantic agent binding')
        assert not server.requests, 'feature alone must not dispatch a backend'
        print('EVIDENCE --features agent without opt-in: no backend request', flush=True)
    finally:
        ui.close()
        server.shutdown()
        worker.join(3)
    flags = ['--agent-local-binding', '--agent-model', 'ux3b-mock',
             '--agent-base-url', f'http://127.0.0.1:{server.server_port}/v1',
             '--allow-read', '--allow-edit']
    ui = CargoWorkspace(writable=True, agent_flags=flags)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        ready(ui)
        assert not server.requests, 'startup must not dispatch'
        ui.action('New Script')
        ui.paste('print("human"); 42')
        ui.send(b'\x13')
        ui.send(b'\x1b[15~')
        require(transcript(ui), 'final: 42')
        print('EVIDENCE create/editor/Ctrl-S/F5: final: 42', flush=True)
        check(ui, ':scope document-ask explain', 'UX3B inspected committed Script')
        check(ui, ':scope document-propose increment', 'source proposal 1 ready')
        check(ui, ':scope source-diff 1', r'+print(\"accepted\"); 43')
        check(ui, ':scope source-proposal 1', 'source proposal 1: buffer')
        check(ui, ':scope show', 'print("human"); 42')
        check(ui, ':scope source-accept 1', 'source proposal 1 applied')
        check(ui, ':scope show', 'print("accepted"); 43')
        check(ui, ':scope document-propose reject-this', 'source proposal 2 ready')
        check(ui, ':scope source-proposal 2', 'source proposal 2: buffer')
        check(ui, ':scope source-reject 2', 'Rejected')
        check(ui, ':scope show', 'print("accepted"); 43')
        assert len(server.requests) == 4, server.requests
        for path, request in server.requests:
            assert path == '/v1/chat/completions', path
            assert request['model'] == 'ux3b-mock', request
        assert 'human' in json.dumps(server.requests[1]), server.requests[1]
        ui.action('Focus editor')
        ui.send(b'\x1b[F')
        ui.paste(' + 1')
        ui.send(b'\x13')
        check(ui, ':scope show', 'print("accepted"); 43 + 1')
        ui.action('Focus editor')
        ui.send(b'\x1b[15~')
        require(transcript(ui), 'final: 44')
        destination = Path(ui.workspace.name) / 'new.scope'
        assert not destination.exists()
        check(ui, ':scope save-as new.scope', 'new.scope')
        assert destination.read_text() == 'print("accepted"); 43 + 1'
        check(ui, ':scope open new.scope', 'new.scope')
        check(ui, ':scope show', 'print("accepted"); 43 + 1')
        assert (destination.parent / 'existing.scope').read_text() == '41'
        assert len(server.requests) == 4, 'review/run/save must not call model'
        print('EVIDENCE continued editor draft published/run: 44; Save As created new.scope; reopened exact source; unrelated file unchanged', flush=True)
    finally:
        ui.close()
        server.shutdown()
        worker.join(3)
        server.server_close()
        signal.alarm(0)
    print('PASS: UX3b real root cargo run --locked --features agent; 4 loopback requests only', flush=True)


if __name__ == '__main__':
    main()
