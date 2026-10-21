# Q3 — Sequential tool use and local serving comparison

## Result — 2026-10-22

The baseline completed **3/3 predeclared fresh sessions**, with **9/9 valid single-object decisions**, **6/6 correct sequential read-only inspections**, and zero malformed decisions, backend errors, truncations, or unauthorized attempts. Every completion exactly used the second inspection's answer. No failures were replaced or selectively rerun.

This demonstrates feedback-dependent tool use on a small synthetic fixture, **not mathematical reasoning or proof ability**. Keep the production protocol unchanged. No oMLX configuration was verified or tested; this is not a server/model benchmark.

## Infrastructure and protocol

Inspected and reused:

- `crates/scope-ide/examples/q2_live.rs`: opt-in runner, fresh sessions, private non-overwriting evidence files.
- `crates/scope-ide/src/agent.rs`: production `CellAgent` → Cell `OpenAiChat`, temperature 0.2; `decide` returns `response.content`.
- `crates/scope-ide/src/agent_loop.rs`: unchanged strict full-string serde decision parser and bounded execution loop.
- `crates/scope-ide/src/agent_trace.rs`: `ProtocolTrace` request, raw response, decision, feedback, and outcome events.

The parser still accepts exactly one tagged JSON decision. No JSON repair, splitting, batch execution, native tool integration, implicit retry, reasoning stripping, server-specific normalization, or production setting change was introduced. Q3 uses zero malformed tolerance in its experiment-local limits, so its first malformed response stops without another request. This is stricter than Q2's tolerance, not a production policy change.

New experiment-only files:

- `crates/scope-ide/examples/q3_live.rs`: injectable deterministic fixture/tests and opt-in production-adapter runner.
- `crates/scope-ide/examples/q3_capture.py`: standard-library, finite-lived local HTTP recorder wrapping the runner. It forwards response body bytes unchanged; HTTP framing is regenerated. It makes no inference retries and shuts down with the subprocess.

The recorder is needed because the production adapter's protocol trace does not expose HTTP envelopes, finish reasons, or usage. Actual adapter messages and HTTP request messages were cross-checked for equality, and every HTTP `choices[0].message.content` was cross-checked against both the adapter return and raw parser trace. All nine matched exactly.

## Actual configurations and availability

The initial sandboxed loopback probe failed. The host-local probe outside the sandbox succeeded; this was an access issue, not an inference failure.

| Configuration | Availability / disposition |
|---|---|
| LM Studio, `qwen3-30b-a3b`, `http://127.0.0.1:1234/v1` | Verified advertised identifier; tested baseline through production Scope/Cell adapter |
| Same model through oMLX | Not tested: `omlx` absent from PATH; no compatible engine/endpoint or comparable weights/settings verified |
| `qwen3.8-27b` through oMLX | Not tested: identifier advertised by the existing endpoint, but this does not establish oMLX engine compatibility, loaded weights, or quantization |

`/v1/models` advertised `qwen3-30b-a3b`, `qwen3.8-27b`, and `text-embedding-nomic-embed-text-v1.5`; see [models.json](q3-live/models.json). No serving stack was installed, switched, or reconfigured. Absence from PATH is not proof that oMLX is absent everywhere.

Read-only `lms ps --json` identified the loaded baseline as:

- Publisher `unsloth`, architecture `qwen3moe`, format GGUF.
- `unsloth/Qwen3-30B-A3B-GGUF/Qwen3-30B-A3B-Q4_K_S.gguf`.
- Quantization `Q4_K_S`, 4 bits; size 17,456,010,781 bytes.
- Loaded context 32,768; advertised maximum 40,960; parallel setting 4.
- `lms --version` returned CLI commit `dcefd5a`, not a verified LM Studio application or inference-engine version.

The captured metadata is [loaded-models.json](q3-live/loaded-models.json). Weight hash, engine version, chat template, top-p, top-k, seed, thinking configuration, cache behavior, and other backend defaults remain unverified. No conclusion about their causal effects is warranted.

## Predeclared design

The recorder writes [capture-plan.json](q3-live/capture-plan.json) before contacting the server. The Rust runner writes and syncs [manifest.json](q3-live/manifest.json) before inference. Fixed run order: **1, 2, 3**, baseline only. Each run creates a fresh conversation, `CellAgent`, session, and fixture. Fresh conversations do not imply cleared server caches.

| Setting | Q3 value |
|---|---|
| Model identifier | `qwen3-30b-a3b` |
| Upstream base URL | `http://127.0.0.1:1234/v1` |
| Adapter endpoint | Temporary loopback recorder; actual port retained in [transport.json](q3-live/transport.json) |
| Adapter | Production `CellAgent` / `OpenAiChat` |
| Temperature | 0.2 |
| Max tokens / decision | 1,500 |
| Decision / tool-attempt budgets | 3 / 2 |
| Malformed tolerance | 0 |
| Message / conversation content bytes | 65,536 / 262,144 |
| Upstream HTTP timeout | 180 seconds |
| Runner subprocess timeout | 1,200 seconds |
| Maximum forwarded inference requests | 9 |

Task, identical in every session:

```text
Inspect resource sealed-index with fixture.inspect. Its feedback identifies the next resource. Inspect that resource next, then complete with exactly its answer field. Perform exactly these two sequential read-only inspections, one decision per response. Do not guess, batch actions, or perform other operations.
```

Only `fixture.inspect` is installed, with Read effect and one required string `resource`. The catalog does not disclose the second identifier or answer. Successful first feedback contains:

```json
{"resource":"sealed-index","next_resource":"hidden-card-482"}
```

Only the second successful invocation returns:

```json
{"resource":"hidden-card-482","answer":"amber-heron-916"}
```

These values are wrapped in the unchanged `kind=tool_result` envelope. The answer is absent from all model-visible messages before the second result. The resource identifier first appears in the first result. The deterministic tests assert these disclosure boundaries. The host provider contains the constants, but the model has no source access.

Success requires the exact first and second invocation sequence with exactly the expected argument, both authorized, exactly three valid decisions and two calls, the exact final answer, and intact trace capture. Completing early with even the correct answer fails. Host policy checks the expected sequence and exact arguments; the provider independently checks its accepted state. No source, shell, filesystem, network, write, or evaluation capability is installed for the model.

## Complete live results

Every session returned these exact raw parser strings in order:

```json
{"type":"tool","name":"fixture.inspect","arguments":{"resource":"sealed-index"}}
```

```json
{"type":"tool","name":"fixture.inspect","arguments":{"resource":"hidden-card-482"}}
```

```json
{"type":"complete","content":"amber-heron-916"}
```

| Run | Single-object/schema compliance | Correct sequential calls | Feedback interpretation | Completion | Malformed | Backend errors / truncations | Unauthorized attempts | Stop |
|---|---|---|---|---|---|---|---|---|
| 1 | 3/3 | 2/2 | Next identifier and second answer used | Success | 0 | 0 / 0 | 0 | Completed |
| 2 | 3/3 | 2/2 | Next identifier and second answer used | Success | 0 | 0 / 0 | 0 | Completed |
| 3 | 3/3 | 2/2 | Next identifier and second answer used | Success | 0 | 0 / 0 | 0 | Completed |

All six policy checks allowed exactly the intended read, all six provider calls matched, and all trace writes succeeded. No mutation occurred through the fixture. Host evidence-file writes are not model capabilities.

### Actual envelope and reasoning compatibility

All nine HTTP responses were successful and contained `id`, `object`, `created`, `model`, `choices`, `usage`, `stats`, and `system_fingerprint`. The first response's `stats` was empty; no performance metrics are inferred from it.

All nine assistant message envelopes had:

- `content`: exactly the standalone JSON decision reproduced above.
- `reasoning_content`: an empty string, structurally separate from content.
- `tool_calls`: an empty array.
- `finish_reason`: `stop` on the choice, never `length`.
- `usage.completion_tokens_details.reasoning_tokens`: 0.

No thinking markup, embedded reasoning, markdown, prose, or multiple decisions reached the parser. This establishes compatibility for the observed responses only; it does not test a nonempty reasoning field or prove a universal reasoning-mode setting. Neither the recorder nor Q3 wrapper stripped anything. Scope decisions were **text-authored JSON**, not native API tool calls. HTTP requests contained only `model`, `messages`, `temperature`, and `max_tokens`; no native tools or response-format schema were sent.

No token-limit truncation was observed: reported completion counts were 18, 22, and 17, far below 1,500, with normal `stop` finishes and intact accepted content.

### Usage and measured latency

Each run reported the same actual token counts:

| Decision | Prompt tokens | Completion tokens | Total tokens |
|---|---:|---:|---:|
| First inspection | 303 | 18 | 321 |
| Second inspection | 366 | 22 | 388 |
| Completion | 438 | 17 | 455 |
| Per-run sum | 1,107 | 57 | 1,164 |

Across three runs: 3,321 reported prompt tokens, 171 completion tokens, 3,492 total. These sum API usage fields, including repeated conversation context; they are not unique input-token counts or billing estimates.

| Run | Adapter decision latencies, ms | Summed adapter latency, ms | Summed recorder upstream wall time, ms |
|---|---|---:|---:|
| 1 | 1868.06, 719.64, 651.15 | 3238.85 | 3233.14 |
| 2 | 467.13, 829.74, 636.40 | 1933.27 | 1928.72 |
| 3 | 419.77, 720.30, 657.23 | 1797.31 | 1792.30 |

Adapter latency measures the complete synchronous adapter call, including local recorder overhead. Recorder upstream time measures forwarding through reading the complete response body, not engine-only generation time or time to first token. Sums exclude tool/trace/runner overhead, so they are not end-to-end session times. No causal explanation for the slower first request is established. Q2 did not measure comparable latency or usage, so no Q2 performance delta can be computed.

## Evidence

Complete artifacts are retained in [q3-live/](q3-live):

- `capture-plan.json`, `manifest.json`: predeclared limits/order/task/configuration.
- `models.json`, `loaded-models.json`, `transport.json`: advertised identifiers, loaded-model metadata, recorder routing.
- `http.jsonl`: actual serialized HTTP request bodies, raw response bodies, status, response headers, and measured upstream wall times, ordered 1–9 (three per run).
- `1.messages.jsonl` through `3.messages.jsonl`: exact adapter messages, raw return text/errors, measured adapter latency.
- `1.trace.jsonl` through `3.trace.jsonl`: existing protocol trace, parser outcome, tool feedback, and final stop reason.
- `1.result.json` through `3.result.json`: exact provider calls, policy decisions, success predicate, counters.
- `classifications.json`: post-run cross-checks and per-response usage/latency/finish/reasoning classifications, not a substitute for raw evidence.
- `process.json`: live subprocess exit status, 0.

Capture files are created exclusively with mode 0600. Existing evidence is never overwritten. No API secret was configured or captured. These synthetic transcripts are opt-in, not ordinary application logging.

## Validation and runnable commands

Executed successfully:

```sh
cargo test -p scope-ide --features agent --example q3_live
cargo build -p scope-ide --features agent --example q3_live
python3 crates/scope-ide/examples/q3_capture.py --run-live docs/archive/q3-live
cargo test --workspace --offline
cargo test -p scope-ide --features agent --offline
```

All seven Q3 deterministic tests passed, covering successful progressive disclosure, early/skip completion, reversed/wrong/extra arguments, repeated first inspection, malformed batches with no repair/retry, unknown operation non-dispatch, and incorrect final answers. Existing offline workspace and agent-enabled suites passed; existing duplicate Cell package warnings remain. A post-run offline cross-check validated all nine HTTP/message/parser-text matches and parser acceptances.

For a separately authorized reproduction, build first and choose a **new empty controlled directory**:

```sh
mkdir -m 700 docs/design/q3-new
python3 crates/scope-ide/examples/q3_capture.py --run-live docs/design/q3-new
```

The recorder makes the baseline `/v1/models` check, binds a temporary local port, runs all three sessions, and exits. It requires host-local network access. Alternatively, direct adapter runs without HTTP envelope capture are supported (do not compare absent metadata as if it were measured):

```sh
mkdir -m 700 docs/design/q3-direct
cargo run -p scope-ide --features agent --example q3_live -- \
  --run-live docs/design/q3-direct qwen3-30b-a3b http://127.0.0.1:1234/v1
```

Ordinary builds/tests make no live requests. None of these live commands is part of CI.

## Assessment and smallest next Path proof experiment

Q2 had 12/12 compliant decisions across six one-inspection sessions; Q3 has 9/9 across three two-inspection sessions. The added dependency did not produce an observed failure. These small, explicit, fixed fixtures may have a ceiling effect. Q3 changes task complexity, tool/decision budgets, malformed tolerance, and capture transport; Q2's quantization metadata was not captured, so exact weight equivalence across experiments is not established. This is descriptive evidence, not an isolated causal task-complexity study.

There is no server comparison here. A future same-model oMLX comparison must verify compatible weights, quantization, templates, sampling, and reasoning settings. A move from this Qwen3 GGUF baseline to Qwen3.8-27B through oMLX changes both model and server (and potentially quantization/template); any difference must be labeled confounded, not attributed to either alone. Model advertisement is insufficient engine verification.

The smallest useful **separately authorized** Path experiment is one fixed, already-supported tiny theorem with a known valid proof: read its current obligation using an existing authorized inspection, use that feedback to select and invoke an existing read-only proof-checking operation, then report the actual checker result. Predeclare three fresh sessions with finite budgets and no replacement failures. Provide proof candidates as data to the existing checker, not source edits; require an actual checker result for success. Before execution, verify that those existing read/check capabilities are available and remain within current trust boundaries—if not, stop rather than invent a tool, add capability, or build infrastructure. Keep the strict parser and production settings unchanged. This would separate protocol completion from the mathematical acceptance supplied by the existing checker, without claiming general proof search.

**Stop after Q3. No Path experiment was run, no production settings were changed, and no new trusted capabilities or proof-search infrastructure were added.**
