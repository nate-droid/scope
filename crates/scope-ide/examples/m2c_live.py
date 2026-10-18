#!/usr/bin/env python3
"""Record three live correction runs with an explicitly injected first candidate.
Build example m2b_live first. Only the first response candidate is replaced with bad;
raw and delivered responses are retained. Subsequent responses are untouched.
Local credential-free fixture only; Python standard library only.
"""
import argparse
import http.client
import http.server
import json
import re
from pathlib import Path
import subprocess
import tempfile
import threading


COMMANDS = """:scope scratch open fixture.txt
:scope scratch list
:scope scratch show
:scope agent edit "replace old with new"
:scope source-proposals
:scope source-proposal 1
:scope source-diff 1
:scope scratch list
:scope scratch show
:scope quit
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", default="qwen3-30b-a3b")
    parser.add_argument("--output-dir", type=Path, default=Path("docs/design/m2c-live"))
    options = parser.parse_args()
    options.output_dir.mkdir(parents=True, exist_ok=True)
    for repetition in range(1, 4):
        exchanges = []

        class Proxy(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers["Content-Length"]))
                record = {"request_path": self.path, "request": json.loads(body)}
                exchanges.append(record)
                try:
                    connection = http.client.HTTPConnection("127.0.0.1", 1234, timeout=180)
                    connection.request("POST", self.path, body, {"Content-Type": "application/json"})
                    response = connection.getresponse()
                    data = response.read()
                    record.update(status=response.status, response=json.loads(data))
                    if len(exchanges) == 1 and response.status == 200:
                        delivered = json.loads(data)
                        decision = json.loads(delivered["choices"][0]["message"]["content"])
                        # Fail closed if the frozen fixture's expected tool shape changes.
                        assert decision["type"] == "tool"
                        assert decision["name"] == "language.configured-fixture.replace"
                        assert decision["arguments"]["expected_source"] == "old"
                        assert decision["arguments"]["expected_version"] == "0"
                        decision["arguments"]["arguments"]["candidate"] = "bad"
                        delivered["choices"][0]["message"]["content"] = json.dumps(decision)
                        record["injection"] = "first candidate replaced with bad; no later response altered"
                        record["delivered_response"] = delivered
                        data = json.dumps(delivered).encode()
                    self.send_response(response.status)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)
                    connection.close()
                except Exception as error:
                    record["error"] = str(error)
                    self.send_error(502)

            def log_message(self, *_):
                pass

        server = http.server.HTTPServer(("127.0.0.1", 0), Proxy)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        with tempfile.TemporaryDirectory(prefix="scope-m2c-") as workspace:
            target = Path(workspace) / "fixture.txt"
            target.write_bytes(b"old")
            args = ["target/debug/examples/m2b_live", "--workspace", workspace,
                    "--agent-model", options.model,
                    "--agent-base-url", f"http://127.0.0.1:{server.server_port}/v1", "shell"]
            try:
                result = subprocess.run(args, input=COMMANDS, text=True, capture_output=True, timeout=240)
                record = {"repetition": repetition, "argv": args, "commands": COMMANDS,
                          "exit_code": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
            except subprocess.TimeoutExpired as error:
                record = {"repetition": repetition, "commands": COMMANDS, "timeout_seconds": 240,
                          "stdout": (error.stdout or b"").decode(), "stderr": (error.stderr or b"").decode()}
            events = [json.loads(line) for line in record["stderr"].splitlines() if line.startswith("{")]
            stopped = re.search(r"configured editing stopped: (\w+).*?; (\d+) proposal attempts, (\d+) corrections", record["stdout"])
            ready = "source proposal 1 ready; pending; source unchanged; not saved" in record["stdout"]
            pending = re.search(r"(\d+) pending source proposals", record["stdout"])
            generated = any(e.get("response", {}).get("choices") for e in exchanges)
            record["observations"] = {
                "http_invocations": len(exchanges),
                "provider_events": events,
                "candidates": [e for e in events if e.get("event") == "candidate"],
                "evaluations": [e for e in events if e.get("event") == "evaluation"],
                "outcome": stopped[1] if stopped else ("PendingProposal" if ready else "see stdout"),
                "proposal_attempts": int(stopped[2]) if stopped else (sum(e.get("event") == "candidate" for e in events) if ready else None),
                "corrections": int(stopped[3]) if stopped else (sum(e.get("event") == "candidate" for e in events) - 1 if ready else None),
                "count_basis": "stopped shell counters, or inferred from dispatched candidates on successful fixed-fixture path; not runtime success counter events",
                "pending_proposals": int(pending[1]) if pending else None,
                "model_generation": "observed; inspect HTTP response" if generated else "not observed; inspect HTTP response",
            }
            record.update(exchanges=exchanges, source_before="old", source_after=target.read_text(),
                          source_unchanged=target.read_bytes() == b"old",
                          workspace_files=sorted(str(p.relative_to(workspace)) for p in Path(workspace).rglob("*") if p.is_file()))
            (options.output_dir / f"run-{repetition}.json").write_text(json.dumps(record, indent=2) + "\n")
            print(json.dumps({"repetition": repetition, "invocations": len(exchanges),
                              "source_unchanged": record["source_unchanged"], "exit_code": record.get("exit_code")}))
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
