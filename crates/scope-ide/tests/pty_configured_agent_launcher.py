#!/usr/bin/env python3
"""Real F4 agent editing with a bounded, stdlib-only local OpenAI HTTP fixture.

CSI stripping checks emitted transcripts, not screen cell positions. Source text
and versions are asserted by the Rust downstream consumer, not inferred from UI.
"""
import http.server
import json
from pathlib import Path
import shlex
import sys
import tempfile
import threading
import time

from pty_editable_workspace import Workspace


class AgentWorkspace(Workspace):
    def read(self, timeout=4):
        # The TUI redraws continuously; don't wait four seconds for a quiet
        # interval after each key sequence. expect() still waits for prompts.
        return super().read(timeout=min(timeout, 0.5))


class Model(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.server.requests.append((self.path, request))
        decision = json.dumps({
            'type': 'tool', 'name': 'language.configured-fixture.replace',
            'arguments': {'candidate': 'new'},
        })
        body = json.dumps({
            'id': 'configured-pty', 'object': 'chat.completion', 'created': 0,
            'model': request['model'],
            'choices': [{'index': 0, 'message': {
                'role': 'assistant', 'content': decision}, 'finish_reason': 'stop'}],
            'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2},
        }).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def scenario(binary, denied=False, rejected=False, unavailable=False):
    with tempfile.TemporaryDirectory(prefix='scope-configured-agent-') as root:
        source = Path(root) / 'fixture.configured'
        source.write_text('old')
        other = Path(root) / 'other.configured'
        other.write_text('untouched')
        # Isolated endpoint: F4 must retain the launcher's parsed model settings.
        server = http.server.HTTPServer(('127.0.0.1', 0), Model)
        server.requests = []
        server.timeout = 1
        wrapper = Path(root) / 'consumer'
        wrapper.write_text(
            '#!/bin/sh\n'
            'export SCOPE_CONFIGURED_AGENT_PTY_CHILD=1\n'
            f'export SCOPE_CONFIGURED_AGENT_DENY={int(denied)}\n'
            f'export SCOPE_CONFIGURED_AGENT_REJECT={int(rejected)}\n'
            f'export SCOPE_CONFIGURED_AGENT_UNAVAILABLE={int(unavailable)}\n'
            f'export SCOPE_CONFIGURED_AGENT_BASE_URL=http://127.0.0.1:{server.server_port}/v1\n'
            f'cd {shlex.quote(root)}\n'
            f'exec {shlex.quote(binary)} --exact launcher_child --ignored --nocapture\n'
        )
        wrapper.chmod(0o700)
        # Fork the PTY before starting any HTTP threads.
        workspace = AgentWorkspace(wrapper)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        workspace.deadline = time.monotonic() + 180
        try:
            workspace.expect('Read-onlyinspector')
            assert b'\x1b[?1049h' in workspace.raw, 'launcher did not enter TUI'
            workspace.expect('scope> ', workspace.send(b'\x1bOQ'))
            opened = workspace.command(':scope scratch open fixture.configured')
            assert 'error:' not in opened, repr(opened)
            workspace.command(':scope semantic-target 1')
            workspace.expect('Read-onlyinspector', workspace.send(b':scope quit\r'))

            def command(text, expected=None):
                workspace.send(b'\x1bOS')
                workspace.send(text.encode() + b'\r')
                if expected is None:
                    return
                workspace.send(b'\x1bOS')
                workspace.send(b'pane output\r')
                workspace.send(b'\x1b[6~' * 12 + b'\x1b[A' * 5)
                workspace.expect('scope> ', workspace.send(b'\x1bOQ'))
                output = workspace.expect(
                    'Read-onlyinspector', workspace.send(b':scope quit\r'))
                assert expected.replace(' ', '') in output.replace(' ', ''), repr(output)
                return output

            accepted = not (denied or rejected or unavailable)
            if accepted:
                command('pane editor')
                workspace.send(b'DRAFT-NOT-SOURCE-')
                command(':scope agent edit "replace old with new"',
                        'commit or cancel all editor drafts before execution')
                assert not server.requests, 'dirty dispatch must not call the model'
                command(':scope show', 'old')
                command('pane editor')
                workspace.send(b'\x03')  # cancel, never commit the local draft
                command(':scope downstream consumer inspect draft-dispatch-cancelled')

            if unavailable:
                command(':scope scratch new unavailable-language')
                command(':scope agent edit "replace old with new"',
                        'matching the host language service')
                command(':scope source-proposals', '0 pending source proposals')
                assert not server.requests, 'incompatible language must fail before model dispatch'
                command(':scope downstream consumer inspect unavailable')
                workspace.send(b'\x11')
                assert workspace.wait_exit() == 0, bytes(workspace.raw)
                assert source.read_text() == 'old'
                assert other.read_text() == 'untouched'
                assert b'\x1b[?1049l' in workspace.raw
                return

            generated = command(':scope agent edit "replace old with new"',
                                'no proposal applied' if denied else 'source proposal 1 ready')
            if not denied:
                assert 'Passed' in generated, repr(generated)
            assert len(server.requests) == 1, server.requests
            path, request = server.requests[0]
            assert path == '/v1/chat/completions', path
            assert request['messages'], request
            command(':scope source-proposals',
                    '0 pending source proposals' if denied else '1 pending source proposals')
            if denied:
                command(':scope downstream consumer inspect denied')
            elif rejected:
                command(':scope downstream consumer inspect pending')
                command(':scope source-reject 1', 'source proposal 1: Rejected')
                command(':scope source-proposals', '0 pending source proposals')
                command(':scope source-accept 1')
                command(':scope downstream consumer inspect rejected')
            else:
                command(':scope downstream consumer inspect pending')
                command(':scope source-diff 1', '+new')
                command(':scope source-proposal 1', 'source proposal 1: buffer 1')
                command('pane editor')
                workspace.send(b'DRAFT-BEFORE-ACCEPT-')
                command(':scope source-accept 1',
                        'commit or cancel all editor drafts before execution')
                assert len(server.requests) == 1, 'dirty acceptance must not call the model'
                command(':scope show', 'old')
                command('pane editor')
                workspace.send(b'\x03')
                command(':scope downstream consumer inspect draft-accept-cancelled')
                # Selection is independent of the proposal owner and semantic
                # binding: acceptance must edit buffer 1, never this buffer 2.
                command(':scope scratch open other.configured')
                command(':scope source-accept 1', 'source proposal 1 applied')
                command(':scope downstream consumer inspect accepted')
                command(':scope source-accept 1')
                command(':scope downstream consumer inspect duplicate')
                command(':scope show', 'untouched')
            workspace.send(b'\x11')
            assert workspace.wait_exit() == 0, bytes(workspace.raw)
            assert len(server.requests) == 1, 'review/acceptance must not call the model'
            assert source.read_text() == 'old', 'agent editing must not save the source'
            assert other.read_text() == 'untouched', 'selection safety must not save either source'
            assert b'\x1b[?1049l' in workspace.raw, 'terminal was not restored'
        except BaseException:
            print(bytes(workspace.raw[-20000:]), file=sys.stderr)
            raise
        finally:
            workspace.close()
            server.shutdown()
            worker.join(timeout=3)
            server.server_close()


def main():
    scenario(sys.argv[1], denied=False)
    scenario(sys.argv[1], denied=True)
    scenario(sys.argv[1], rejected=True)
    scenario(sys.argv[1], unavailable=True)
    print('Real F4 agent edit: contracted pending/diff/review/accept, exact source '
          'versions 0/1, dirty-draft dispatch/acceptance guards, selection safety, '
                    'duplicate acceptance, rejection, authorization denial and unavailable language; disk unchanged')


if __name__ == '__main__':
    main()
