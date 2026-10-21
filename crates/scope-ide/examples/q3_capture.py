#!/usr/bin/env python3
"""Opt-in Q3 byte-preserving local HTTP recorder; no inference retries.
Build q3_live first. Only Python's standard library is required.
"""
import argparse
import http.client
import http.server
import json
import os
from pathlib import Path
import subprocess
import threading
import time


def exclusive(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as out:
        json.dump(data, out, indent=2)
        out.write('\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-live', action='store_true', required=True)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    if not output.is_dir():
        parser.error('output must be an existing controlled directory')
    # Fixed baseline only. Neither model nor sampling is changed by this recorder.
    exclusive(output / 'capture-plan.json', {
        'upstream': 'http://127.0.0.1:1234/v1', 'model': 'qwen3-30b-a3b',
        'order': [1, 2, 3], 'maximum_requests': 9, 'http_timeout_seconds': 180,
        'process_timeout_seconds': 1200, 'retry': False,
        'transport': 'byte-preserving response body forwarding; HTTP framing regenerated',
    })
    conn = http.client.HTTPConnection('127.0.0.1', 1234, timeout=10)
    conn.request('GET', '/v1/models')
    response = conn.getresponse()
    models = json.loads(response.read())
    conn.close()
    exclusive(output / 'models.json', models)
    if 'qwen3-30b-a3b' not in [m['id'] for m in models['data']]:
        raise RuntimeError('baseline model not advertised')
    records = os.open(output / 'http.jsonl', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    log = os.fdopen(records, 'w')

    class Recorder(http.server.BaseHTTPRequestHandler):
        ordinal = 0

        def log_message(self, *_):
            pass

        def do_POST(self):
            Recorder.ordinal += 1
            body = self.rfile.read(int(self.headers['Content-Length']))
            start = time.perf_counter()
            connection = http.client.HTTPConnection('127.0.0.1', 1234, timeout=180)
            record = {'ordinal': Recorder.ordinal, 'path': self.path,
                      'request_raw': body.decode('utf-8')}
            try:
                if self.path != '/v1/chat/completions' or Recorder.ordinal > 9:
                    raise RuntimeError('unexpected path or request budget exceeded')
                connection.request('POST', self.path, body=body,
                                   headers={'Content-Type': 'application/json'})
                result = connection.getresponse()
                raw = result.read()
                record.update(status=result.status, response_raw=raw.decode('utf-8'),
                              response_headers=result.getheaders(),
                              upstream_wall_ms=(time.perf_counter()-start)*1000)
                log.write(json.dumps(record) + '\n')
                log.flush()
                self.send_response(result.status)
                self.send_header('Content-Type', result.getheader('Content-Type', 'application/json'))
                self.send_header('Content-Length', str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
            except Exception as error:
                record['error'] = repr(error)
                log.write(json.dumps(record) + '\n')
                log.flush()
                self.send_error(502, 'Q3 recorder/upstream error; no retry')
            finally:
                connection.close()

    server = http.server.HTTPServer(('127.0.0.1', 0), Recorder)
    endpoint = f'http://127.0.0.1:{server.server_port}/v1'
    exclusive(output / 'transport.json', {'adapter_endpoint': endpoint,
              'upstream_endpoint': 'http://127.0.0.1:1234/v1'})
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        result = subprocess.run(['target/debug/examples/q3_live', '--run-live', str(output),
                                 'qwen3-30b-a3b', endpoint], timeout=1200)
        exclusive(output / 'process.json', {'returncode': result.returncode})
        raise SystemExit(result.returncode)
    finally:
        server.shutdown()
        server.server_close()
        log.close()


if __name__ == '__main__':
    main()
