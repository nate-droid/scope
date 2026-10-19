# Standalone scripted harness

`examples/harness` is a downstream consumer with its own `[workspace]`, manifest,
lockfile, and build output. Its only direct dependency is `scope-ide` via
`../../crates/scope-ide`, with default features disabled and `features = ["tools"]`.
The `tools` feature is independent of any model integration: no `agent` feature,
OpenAI client, credentials, network calls, or new decision-framework dependencies.

From the Scope repository root:

```sh
cargo test --offline --manifest-path examples/harness/Cargo.toml
cargo run --offline --manifest-path examples/harness/Cargo.toml
```

The standard downstream check also tests and runs this harness, using its own
manifest and the check's shared build directory:

```sh
cargo run --offline -p xtask -- check-downstream --offline
```

Offline builds require the existing transitive dependency cache. The tests and
binary themselves never use the network. The binary runs an approved edit and a
denied edit, then exits; each fixture is deleted by an RAII guard, including on
ordinary errors and unwinding. Abrupt process termination can leave a temporary
fixture behind. No real project files are opened or edited by the runner.

## Ownership and execution

The runner creates a unique temporary directory containing `main.scope`, builds
an `App`/`Workspace` rooted there, and registers Scope's session provider plus a
custom read-only `harness.ping` provider. `session.catalog()` discovers definitions,
provider names, and optional output descriptions before dispatch. The harness's
own `ScriptedDecisions` adapter receives this catalog, calls the custom tool,
reads a document snapshot, and proposes a fixed edit from that snapshot. If the
catalog lacks the replace tool, it finishes without proposing an edit; missing
required read/custom tools produce a decision error. Discovery is not approval.

Host approval is separate from the decision adapter. Every dispatch uses
`invoke_observed_with_call_policy`; policy receives the actual definition and
call arguments before dispatch. Reads are permitted and writes require the host's
explicit `ApproveFixtureWrite` choice, restricted to the known replace tool and
fixture path. `DenyWrites` prevents dispatch. This is a scripted host approval,
not a human prompt or an autonomous model's consent. The harness owns the run ID,
step IDs, bounded tool-step budget, stop conditions, and event collection. Scope
owns per-session call IDs and tool dispatch. The bound limits dispatch count,
not execution time; providers and synchronous observers must terminate.

Native Rust providers are trusted host code, **not a Cell sandbox**. Effect
metadata and this policy are not an isolation boundary against a dishonest
provider. Workspace authorization still applies to built-in tools; registration
of a custom provider does not grant it extra workspace or Cell authority. The
custom provider here returns a constant and performs no filesystem operations.

## Bounded LLM decision adapter

The scripted harness above is intentionally unchanged and remains `tools`-only.
Separately, `scope-ide`'s optional `agent` feature provides a reusable bounded
adapter connected to `AgentSession`. CLI and shell `agent` commands use it with
built-in providers and read-only host policy, including when human shell commands
can edit/save. A model cannot authorize its own writes. The legacy
`CellAgent::answer` API remains single-shot text completion without tool dispatch.

Cell's current chat capability accepts text messages, not native function calls
or a server-enforced JSON schema. Each model decision must be exactly one of:

```json
{"type":"tool","name":"scope.document.read","arguments":{"path":"main.scope"}}
```

```json
{"type":"complete","content":"Final answer or explanation of missing capabilities."}
```

No markdown fences, extra fields, multiple decisions, or trailing text are
accepted. Arguments support the existing `ToolValue` kinds: null, boolean,
signed 64-bit integer, string, array, and object; floats/out-of-range integers
are rejected. Blank completion content and blank tool names are malformed.
The model receives the actual installed provider catalog. Assistant decisions
stay in the conversation; JSON `tool_result` feedback contains tool name, attempt
number, success value or error, and `ok`. Because chat is text-only, feedback is
sent as a user message, not a native tool-role message. Unknown tools, denied
calls, invalid arguments, and conflicts become feedback for the next decision.
Malformed decisions never dispatch and receive bounded correction feedback.
Tool feedback and project text are untrusted data; prompt instructions are not
a security boundary against prompt injection. Host policy remains decisive.

For a downstream host that already assembled a session:

```rust
use scope_ide::agent::{AgentLimits, AgentStop, CellAgent, read_only_policy};

// `agent` is a host-configured CellAgent; `session` has installed providers.
let report = agent.run_session(
    &mut session,
    "Inspect the project and explain its entry point",
    AgentLimits::default(),
    read_only_policy,
)?;
if report.stop == AgentStop::Completed {
    println!("{}", report.content);
} else {
    eprintln!("Incomplete agent run: {:?}", report.stop);
}
```

Use free `scope_ide::agent::run_session(&mut model, &mut session, request, limits,
policy)` with a custom `DecisionModel`; `DecisionMessage` is re-exported so a
consumer does not need a direct OpenAI dependency to implement it. The policy is
`FnMut(&ToolDefinition, &ToolCall) -> bool` and sees actual arguments immediately
before dispatch. Explicitly granting a write does not bypass workspace checks or
the snapshot/edit contract. The session runner does not implicitly add App
context: supply selected context in the request or let the model read via tools.
`CellAgent::run_app`, used by CLI/shell, additionally supplies active App context.

`AgentLimits` defaults to 12 decisions, 8 tool attempts (denied and unknown calls
count), 2 malformed corrections, 64 KiB/message, 256 KiB total message content,
and 1500 requested output tokens/decision. Public fields allow hosts to select
smaller or larger finite budgets. Malformed responses also consume decisions.
A completion may follow the final permitted tool attempt; an additional attempt
stops before dispatch. A zero decision/token budget makes no model calls; a zero
tool budget still permits completion. Exhausting correction allowance or a
count/byte budget returns a distinct `AgentStop`, not completion. CLI/shell report
these stops as errors. Model transport failures return `AgentError`; no hidden
transport retry is added by Scope's runner.

Byte limits apply to retained conversation content (including catalog, request,
model text, and tool feedback), not total process memory or HTTP payload size.
Over-limit responses stop rather than truncate data into misleading JSON. A tool
may have executed before an oversized feedback result stops the run; nothing is
rolled back. These budgets do not cancel synchronous model/provider calls, impose
wall-clock deadlines, or independently verify endpoint token/billing limits.
No transcript is persisted by default. For an explicitly opted-in investigation,
`agent::run_session_traced` records ordered raw protocol events to a host-owned
`ProtocolTrace<W: std::io::Write>` without changing validation or dispatch. See
[Agent protocol trace](docs/design/AGENT_PROTOCOL_TRACE.md) for sensitive-data
handling, downstream integration, transport limitations, and the deterministic
read-only fixture. Do not send raw traces to normal logs.
Context and feedback are transmitted
to the configured endpoint, so choose providers and retention policies accordingly.

Scope supplies no unrestricted agent shell or automatic build/run/test adapters.
For isolated changes, the host can separately create a bounded exact-path
`Proposal` and register only `ProposalToolProvider` in a fresh session, or call
`CellAgent::run_proposal`. The existing scripted harness remains unchanged and
uses baseline fixture edits, not proposals. `scope.document.create` is available
**only through the proposal provider**, with `path`/`text` inputs; it refuses
existing files and edits only in-memory state. Proposal replacements use
zero-based half-open UTF-8 byte coordinates and an integer proposal revision,
not the baseline provider's opaque disk revision or `save` option.

Human shell commands `proposal <paths...>`, `proposal-agent <request>`,
`proposal-review`, `proposal-accept`, and `proposal-discard` separate path approval,
model edits, review, and acceptance. No acceptance tool is registered. Acceptance
preflights every changed baseline path, then rechecks and applies sequentially;
hosts must inspect both applied paths and errors, since failure may partially
apply and is not rolled back. Path checks are not race-resistant: concurrent
leaf/parent/root-ancestor symlink swaps can redirect a later open outside the
approved root, even for exclusive creation through a swapped parent. Hard links
are not detected. Use only trusted filesystems without concurrent hostile
mutation. See [isolated text proposals](DOWNSTREAM_GUIDE.md#isolated-text-proposals)
for snapshot bounds, binary behavior, conflict detection, and public APIs.
Custom native providers are still trusted host code, not a sandbox. Completion
means the model ended the loop, not that its claims or a development result were
independently validated. Deterministic unit tests under `agent_loop_tests.rs`
exercise the adapter without network; the standalone scripted harness continues
to exercise model-independent host orchestration and fixture-only edits.

## Reviewable headless edits

For a single persisted document, use `DocumentSession`'s revision-bound editing
workflow (no `App`, UI, or agent feature required):

```rust,ignore
use scope_ide::document::EditDecision;

let base = session.snapshot()?;
let proposal = session.propose(&base.revision, "replacement text\n", "host operation")?;
// Analyze proposal.candidate().text, not the base, to obtain diagnostics.
let review = session.review(&proposal, diagnostics)?;
println!("{}", review.diff());
// Only the trusted host/human decides; never infer this from model output.
let approval = session.authorize(&review, EditDecision::Allow)?;
let applied = session.apply(&review, &approval)?;
assert_eq!(&applied.revision, &proposal.candidate().revision);
```

`EditProposal` retains immutable base/candidate snapshots and provenance;
`EditReview` retains candidate diagnostics and a full-replacement display diff
(not a machine-applicable patch). Each candidate has a unique session-local revision.
Diagnostics are supplied by the host, which must compute them from that candidate;
Scope does not verify their analysis origin. Approval receipts cannot be forged
through public fields and are bound to the exact candidate, not arbitrary text.
`EditDecision::Deny` prevents apply; proposing, reviewing, and authorizing never save.

With `tools`, `OperationRegistry::propose_edit(session, expected, call, policy)`
uses the existing schema-validated, bounded, explicitly authorized operation dispatch.
Its successful `ToolValue::String` output is interpreted as a full replacement;
other output kinds produce `OperationEditError::ExpectedReplacement`.
`OperationEdit` retains the original structured call, validated result, and proposal
via `call()`, `result()`, and `proposal()`. Review it through the same session API.
Invocation policy does not authorize persistence, and no authorize/apply agent tool
is added. Native providers remain trusted code, not sandboxed guests.

Stale/foreign-session proposals, wrong-candidate receipts, denied decisions, and
save failures return typed `EditError` values. Apply requires the session's writable
workspace capability and reuses `Workspace::save_file_checked`, **not** the older
multi-file proposal's host filesystem path. The confined save offers best-effort
conflict detection, **not atomic compare-and-swap**: another writer can race its
revision check and replacement. Save failure may have filesystem side effects;
there is no rollback guarantee. Failed apply never accepts candidate session state,
though observing changed/unreadable source can invalidate the cached base. Reload
and propose again after a conflict; retry a save failure only while the base remains
current. Keep the review after apply to retain diagnostics and provenance.

This headless full-replacement API does not edit unsaved `App` buffers or replace
the existing shell/multi-file proposal contract.

## Optional typed pipeline tools

A downstream native `ToolProvider` can explicitly call
`scope_execution::execute` to run a typed WASM pipeline through Cell. Add
`scope-types`, `scope-execution`, and Cell dependencies to that consumer and
register a namespaced tool with an appropriate input contract and effect
classification. Pipelines and their bindings do not automatically appear in
`session.catalog()`: the provider supplies the tool definition and adapts its
arguments into typed pipeline inputs. The checked-in harness does not implement
this integration; its dependencies and test coverage remain as described above.

A separate standalone consumer validates that this execution API accepts a
downstream-owned compatible component without changing Scope. From the Scope root:

```sh
cargo test --manifest-path examples/downstream/Cargo.toml --test wasm_binding --offline
```

That test binds a byte-reversal guest with custom nominal types and the
non-default `reverse-bytes` export, checks binary payloads, and rejects an output
type-version mismatch without publishing artifacts or recording completion.
See [downstream transformation validation](DOWNSTREAM_GUIDE.md#validate-a-downstream-owned-transformation)
for the fixture and compatibility contract. This is component execution coverage
in `examples/downstream`, not a pipeline-backed harness tool or an end-to-end test
of tool approval plus guest execution. The standard `check-downstream` command
includes this consumer test; the harness's own dependencies remain unchanged.

Keep three distinct checks in place:

- **Tool approval:** call-aware host policy approves the actual tool invocation
  and its arguments before native provider dispatch. Describe effects based on
  the whole operation, including guest start functions and capability use—not
  merely whether the selected output is a value.
- **Pipeline preflight:** the executor validates graph wiring, nominal types,
  external inputs, the selected output, host-supplied planning grants, component
  links, and export ABIs before instantiating any guest. Transformation requirements
  and advisory capability metadata are not automatic planning grants.
- **Runtime authority:** the host separately installs narrowly scoped capability
  implementations on the Cell passed to execution. Cell links guest imports and
  capability implementations enforce their operation policies. Tool approval or
  a planning grant cannot install an import or override a runtime denial.

These checks complement rather than replace one another. The provider and its
host-side argument/result adapters remain trusted native code; only the bound
transformations execute inside Cell components.

Inspect `ExecutionResult.errors` before reporting tool success. Deliberately map
its selected output or failures into the tool response, and decide whether any
intermediate artifacts should be exposed. On runtime or output failure, earlier
successful artifacts and completion records remain, but earlier guest side
effects are **not rolled back**. Neither the pipeline nor the surrounding tool
call is a transaction.

Existing `ToolExecutionEvent` observation covers the outer tool invocation, not
individual pipeline nodes. `ExecutionResult.completed` records successful nodes,
but is not a node-level event stream or replay log. Do not automatically add
artifact bytes, source text, arguments, or result payloads to the metadata-only
trace; any richer diagnostics need an explicit privacy and retention policy.

One pipeline-backed tool invocation counts as one harness tool dispatch even if
it executes multiple nodes. The existing step budget therefore bounds neither
pipeline node count nor execution time. The executor is synchronous and supplies
no cancellation or timeout policy; downstream hosts must choose acceptable graphs
and components and establish any additional execution limits separately.

## Snapshot/edit contract

`scope.document.read` supplies text and disk `revision`; copy the opaque revision
into the mandatory `expected_revision` on `scope.document.replace`. Positions
are zero-based; the script replaces columns 3 through 6 on line 0. It explicitly
sets `save=true`. An intervening disk change produces `ToolError::Conflict` and
leaves that changed disk content untouched.

When the read matches the active document, it also supplies `document_id` and
`version`. An edit of that active document must supply both as
`expected_document_id` and `expected_version`, in addition to the baseline disk
revision. Identity prevents applying a snapshot to a different buffer instance;
version prevents applying it after in-memory edits. The adapter forwards both
when present. A disk revision alone is insufficient for active-buffer edits.
The default runner has no active document, so its tests exercise the disk branch.

Nonactive `save=false` is unsupported: unsaved edits require the matching active
document. Editing another path while the active buffer is dirty is rejected;
snapshot identity fields for another path are also rejected. These protections
avoid silently discarding or confusing buffer state.

Checked saving is **best-effort, not filesystem compare-and-swap (CAS)**. Revision
checks detect observed changes but cannot rule out every concurrent-writer race.
The checked-save implementation is limited to existing files on Unix; this is
not a portable new-file creation API or a transactional multi-file editor.

## Trace and validation

The observer records only `ToolExecutionEvent` metadata: tool name, session call
ID, start/finish, duration, and outcome, wrapped with harness run/step IDs. It
records no arguments, source text, result payloads, revisions, or error messages.
Snapshots are held separately in memory for decisions. This trace is **not a
replay log**; it lacks the payloads/state needed for replay. Durations vary and
are deliberately not compared in tests.

Seven deterministic tests cover successful write, denied write, stale revision
conflict, custom provider dispatch, step-budget exhaustion (including zero),
catalog-dependent decisions, and path-specific approval.
All mutations, including the simulated external change, affect only fixtures.
The consumer is deliberately outside Scope's parent Cargo workspace; it exercises
Scope's public APIs rather than adding harness orchestration to the core.
