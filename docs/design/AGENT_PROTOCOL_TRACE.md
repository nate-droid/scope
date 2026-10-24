# Agent protocol trace design

## Scope

The opt-in runner trace API observes the ordinary REQUEST decision loop in
`crates/scope-ide/src/agent_loop.rs`, its Cell adapter, and host presentation.
It is not a configured edit lifecycle, source-proposal workflow, or wire recorder.
Tracing must not change parsing, prompts, budgets, dispatch, authorization,
retries, or completion behavior.

## Current path and observation boundaries

`run_cli_agent_model` wraps the user request with `AgentContext::from_app(app).render()`, installs the app and session Scope tools, and optionally installs `InteractiveInspectionProvider` for the active interactive session. It calls `agent::run_session` with default limits and `read_only_policy`. Non-completed stops become `AgentError::Stopped`; a completed run becomes `AgentResponse { content }`.

The top-level CLI prints that content and adds a trailing newline if needed. The ordinary shell agent branch uses the same adapter, supplies the interactive session, and writes either the content with a newline or `error: {error}`. Configuration failures can occur before a runner exists. These presentation paths discard the raw decision envelopes and run counters; their displayed text cannot reconstruct a transcript. Legacy `answer` and proposal/edit paths are separate APIs.

| Boundary | What can be observed | What it does not establish |
| --- | --- | --- |
| Runner request | Ordered role/content messages actually passed to `DecisionModel::decide`, including protocol, wrapped request, catalog, retained decisions and feedback; requested token limit | Exact HTTP serialization or server interpretation |
| Model return | Exact Rust string returned by `decide`, before byte-budget checks or parsing | Raw HTTP bytes, other choices, finish reason, usage, or whether the server truncated generation |
| Decision validation | Strict parser acceptance or rejection and precise protocol error | Tool authorization, successful dispatch, language correctness, or task success |
| Feedback | Generated tool-result/protocol-error envelope and whether it entered the conversation | A generated but unretained envelope was not sent to the model |
| Runner outcome | Completed/budget stop, completion content and counters, or returned error | The final text displayed by the host or semantic accuracy |
| Host presentation | Text explicitly supplied by the host for presentation recording | Terminal delivery, flush/durability, or transformations the host did not record |
| Cell transport | Only upstream instrumentation can expose the actual request/response boundary | A runner trace is not a wire trace |

The current `DecisionModel` returns only text. `CellAgent::decide` constructs a Cell `ChatRequest` with messages, temperature 0.2 and the supplied `max_tokens`, then keeps only `response.content`. Settings metadata on the Cell `DecisionModel` implementation exposes the configured model identifier and temperature, not API keys, HTTP headers/status/bodies, finish reason, usage, or other transport metadata. Generic models may report settings as unavailable; do not invent values or infer server settings from a model name. Reported model/temperature are client configuration, not confirmation of the served model or effective sampling configuration.

## Opt-in API

Public surface under `agent`:

```text
ProtocolTrace<W: std::io::Write>
ProtocolTrace::new(writer)
ProtocolTrace::failed() -> bool
ProtocolTrace::record_presentation(&str)

run_session_traced(model, session, request, limits, policy, &mut trace)
```

The free `run_session_traced` function accepts the same model, mutable session, request, limits and call-aware policy as `run_session`, plus the caller-owned trace. It returns the same `Result<AgentRun, AgentError>`. Existing `run_session` remains unchanged at its public boundary, with tracing disabled by default. No automatic CLI flag, environment toggle, trace file, logger hookup, or network destination is implied.

Both entry points use a shared internal execution path rather than a competing runner. Trace failures must not turn into model/tool failures, consume decisions, trigger retries, or bypass policy. A serialization/write failure sets a sticky `failed()` flag and stops further trace writes; the original session result remains authoritative. A partial final JSONL line is possible. The host must check `failed()` and treat that artifact as incomplete, not a complete history. The trace flushes the writer after every JSONL event; durable storage remains a host responsibility. Successful flushes are not a durability guarantee. Writers are trusted synchronous host code and must not panic or block forever.

`record_presentation` is explicitly host-invoked after the host chooses the text it will display. Record completion text, stop/error formatting, or downstream wrappers as appropriate, separately from the runner outcome. Include the actual newline/prefix if it is part of the chosen presentation. This method does not intercept stdout/stderr or prove output delivery. Hosts that do not call it have no presentation evidence.

## JSONL event contract

Emit one serialized JSON object per line, escaping embedded newlines and control characters. Preserve strings exactly at the observed boundary; do not trim, repair JSON, extract Markdown, truncate, or normalize content. The following describes implemented fields and ordering, not captured live data or a stable versioned interchange format. Use one trace per session.

| Event | Required evidence |
| --- | --- |
| `request` | `ordinal` (one-based model invocation within this run), ordered `messages` with role/content, `max_tokens`, and `settings` metadata |
| `response` | Matching ordinal and `raw`, the exact returned decision string |
| `decision` | Matching ordinal and accepted/rejected status; accepted decision shape, or rejected category and detailed parser error |
| `feedback` | Matching ordinal, full generated `envelope`, and boolean `retained` |
| `outcome` | Stop/content/counters (`decisions`, `tool_calls`, `malformed`) for an `AgentRun`, or the returned error |
| `presentation` | Host-supplied text from `record_presentation`, distinct from completion content |

Emit a request immediately before invoking the model, and a response immediately after a successful text return, before retention/parsing. A model error has a matching `response` with an `error` field instead of `raw`, followed by an error outcome, not a fabricated completion or rejected decision. Ordinals identify actual calls, not successful completions.

A decision event describes an actual parser invocation. The current loop checks assistant-message retention before parsing: if its byte budget fails, record the response and outcome but no invented parser verdict. Accepted means protocol-valid, not authorized or successful. Tool lookup, argument validation, denial, conflict and provider errors remain tool feedback, not protocol rejection. Keep rejection categories (`incomplete_response`, `malformed_response`, `invalid_decision`) and exact details; valid JSON with an invalid decision schema is `invalid_decision`, while invalid JSON/framing is `malformed_response`. See [strict decisions](../AI2_PROTOCOL_ROBUSTNESS.md).

Record feedback after constructing the envelope and attempting retention. `retained=false` preserves the result even when message/conversation limits prevent appending it. `retained=true` does not guarantee another model call: a decision-budget stop may follow. Do not invent feedback for paths that generate none: completion, exhausted malformed tolerance, exhausted tool budget, or assistant retention failure. Preserve existing accounting, including denied/unknown tool attempts and malformed decisions. A budget stop is not completion; errors are not converted into natural-language answers.

Every normal return or error from the traced runner should attempt one outcome, even if initial messages exceed budget or no model call occurs. A trace failure or process interruption can prevent that record. Missing outcome is not evidence of a particular stop. Scope does not gain rollback or cancellation by tracing.

## Sensitive trace policy and host-owned storage

A trace is a sensitive, potentially complete conversation export. Requests include source context and catalogs; repeated requests repeat prior decisions and tool results. Responses, provider errors, feedback, outcomes and presentations can contain proprietary source, paths, personal data, credentials embedded in source, and untrusted instructions/control characters. Tracing grants no new tool or filesystem authority.

- Require explicit host/user opt-in for the particular investigation. Keep tracing off by default.
- The host chooses and opens the writer, destination, retention period and access controls. The trace type accepts a writer; it must not choose a path, create directories, infer workspace consent, or upload anything.
- Do not route traces into normal application logs, Cell logging, telemetry, crash reports, CI artifacts, or automatic uploads. Ordinary operational logs may report that capture failed without including payloads or sensitive error text.
- Never intentionally collect authentication headers, API keys, or credential-bearing URLs as settings. Payloads can still contain secrets; review locally before sharing. Treat trace text as data, never executable commands or trusted instructions.
- Prefer a private directory outside source control and cloud-synced/shared locations. Use a unique new file, restrictive permissions at creation, and a bounded retention policy. Do not rely on applying permissions after writing. On non-Unix systems use an equivalent restrictive ACL.
- Preserve the original locally under restricted access. Share only a separately redacted derivative with explicit approval; label redaction and do not claim byte-exact evidence for modified fields. Remove related backups/copies according to the host's retention policy.

Host-owned Unix file creation (standard library only):

```rust
use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

fn create_private_trace(path: &Path) -> io::Result<std::fs::File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
```

The host supplies a path inside an already trusted private directory. `create_new(true)` rejects an existing final path, including a final symlink, rather than overwriting it; it does not secure untrusted parent directories. Unix mode 0600 requests owner-only read/write permissions (the umask may further restrict them). Protect the directory and account as well. After running, the host checks trace failure and explicitly flushes/synchronizes its chosen writer as required.

Existing native logging is a separate opt-in facility: `crates/scope-ide/src/logging.rs` and `examples/native_logging.rs` use a host-authorized Cell logger, with deny-by-default bindings. Logging permission is not protocol-capture consent, and a logging receipt is not a durability guarantee. This design must not silently attach the sensitive trace to that facility.

## Investigation procedure

Obtain the real task, source fixture, installed catalog, host policies, dependency
revisions, endpoint/model identifiers, generation settings, and budgets before
attempting reproduction. Unknown historical inputs must remain unknown; a new
fixture is a new baseline, not a reproduction.

Capture only with explicit consent into private host-owned storage. Use fresh
equivalent sessions, retain every attempt, and record final host presentation
separately. Classify raw response, strict parser outcome, dispatch, structured
semantic outcome, and task correctness independently before changing behavior.
A parser's incomplete-response classification alone cannot establish token
truncation; upstream finish evidence is required.

Runner tracing cannot recover metadata already discarded by Cell. Exact HTTP
observation requires separately authorized transport instrumentation; decoded and
reserialized JSON is not byte-exact wire evidence. Do not add a competing client
or silently change authentication, serialization, or response handling. The
remaining `q3_capture.py` recorder is a fixed, credential-free synthetic experiment
for the existing `q3_live` launcher, not a general authenticated endpoint proxy.

Predeclare matched comparisons before interventions. Hold fixture, policy,
settings, and budgets constant; distinguish injected output from unchanged
baseline output. Review captures locally, share only approved redacted derivatives,
and apply the chosen retention policy. Never use a small fixture's completion
rate as a general reliability or semantic-correctness guarantee.

## Enabling capture in a downstream host

With the `agent` feature enabled, replace only the ordinary runner invocation for an explicitly opted-in investigation:

```rust
use scope_ide::agent::{run_session_traced, ProtocolTrace};

// file: securely opened host-owned File; model: existing CellAgent or DecisionModel.
let mut trace = ProtocolTrace::new(file);
let result = run_session_traced(
    &mut model, &mut session, request, limits, policy, &mut trace,
);
// Apply the host's existing success/error formatting without changing it.
let displayed = existing_host_format(&result);
trace.record_presentation(&displayed);
if trace.failed() {
    // Report only a non-sensitive diagnostic; the run result is still authoritative.
}
// Present `displayed` using the existing shell/UI path.
```

`existing_host_format` denotes the downstream presentation logic, not a new Scope API. There is intentionally no automatic environment variable or CLI trace destination. The downstream shell must opt in at its actual runner call and record its presentation. For a `CellAgent`, the free function accepts `&mut agent.clone()` if the host holds an immutable agent. Normal `run_session` does not call metadata hooks or build trace events.

## Deterministic fixture

Reproduce the synthetic read-only session, without a backend:

```sh
cargo run --offline -p scope-ide --features agent --example read_only_trace
```

`crates/scope-ide/examples/read_only_trace.rs` emits JSONL to stdout, explicitly chosen by this diagnostic example. Redirect only to a protected host-selected file. It uses synthetic inspections and a malformed completion followed by correction,
not downstream language semantics or live reliability evidence.
