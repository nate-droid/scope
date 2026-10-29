# Agents and harnesses

This is the canonical guide for model-independent tool orchestration, the optional
model loop, approval, proposals, and sensitive evidence. For build/features see
[Development](development.md); for language registration, sessions, and component
execution see [Integration](integration.md).

## Choose an agent surface

| Surface | Feature | Authority and purpose |
| --- | --- | --- |
| `AgentSession`, catalog and call-aware dispatch | `tools` (default on `scope-ide`) | Host-built orchestration; no model or implicit approval |
| `CellAgent::answer` | `agent` | Single-shot response, no tool dispatch |
| `run_session` / `CellAgent::run_session` | `agent` | Bounded decisions against installed providers and concrete host policy |
| Semantic investigation / retained semantic agent | `agent` | Explicit host target, operation allowlist and source-bound evidence |
| Source edit / contracted edit | `agent` for model generation | Candidate evaluation and proposal production remain separate from acceptance and save |
| Exact-path filesystem `Proposal` | No feature for snapshot; `tools`/`agent` for adapters | Isolated changes; explicit host acceptance on trusted filesystems only |

The stock CLI/shell investigation path denies writes, even with `--writable`.
That does not mean all embedding APIs deny writes: a host can explicitly authorize
baseline tool writes or proposal production. No model output can approve itself.
Native services, providers, workflows and observers are trusted synchronous host
code; neither read-only parameters nor effect labels sandbox them. Prompt injection
must be contained by actual dispatch policy, source checks and capability policy.

## Run the optional model integration

Enable the optional build feature with `--features agent` before Cargo's `--`.
The default `scope-workbench` package forwards this feature to `scope-ide/agent`:

```sh
cargo run --locked -p scope-workbench --features agent -- shell
```

At the shell prompt use `agent <request>` or `ask <request>`; there is no
`--agent` runtime switch. Reads are allowed by default without `--allow-read`.
Runtime model options cannot enable support in a binary built without the feature.
Other downstream hosts need their own forwarding feature, for example
`agent = ["scope-ide/agent"]`. Without `agent`, model commands/options report an
explicit enablement error rather than changing ordinary shell behavior.
For a one-shot request using the standalone CLI:

```sh
cargo run --locked -p scope-ide --features agent -- --workspace . agent --open README.md 'summarize this file'
```

The default endpoint is `http://localhost:1234/v1`, model `qwen3-30b-a3b`;
a server must already be running. Put global options before `agent`:
`--agent-model NAME`, `--agent-base-url URL`, `--agent-openai`, and
`--agent-api-key-env NAME`. The last reads a secret from the named environment
variable. Never commit credentials. Selected source, catalog and tool feedback
leave the process for the configured endpoint; remote retention, billing and rate
limits are provider concerns. Live calls are not required for deterministic tests.

## Standalone scripted harness

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
[Agent protocol trace](design/AGENT_PROTOCOL_TRACE.md) for sensitive-data
handling, downstream integration, transport limitations, and the deterministic
read-only fixture. Do not send raw traces to normal logs.
Context and feedback are transmitted
to the configured endpoint, so choose providers and retention policies accordingly.

Scope supplies no unrestricted agent shell or automatic build/run/test adapters.
For isolated edits use the separate [exact-path proposal workflow](#isolated-text-proposals),
not baseline harness writes. It registers no acceptance tool; model completion is
neither approval nor independent validation. Budget stops/errors may leave a
proposal available for review. The deterministic `agent_loop_tests.rs` tests use
no network.

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

A native `ToolProvider` can explicitly adapt structured arguments to
`scope_execution` inputs and map outputs/errors into a tool response. Pipelines do
not automatically enter `session.catalog()`. The checked-in harness has no such
tool; the separate [downstream component test](integration.md#validate-a-downstream-owned-transformation)
checks component interoperability, not approval plus guest execution end to end.

Keep these checks separate:

- **Tool approval:** call-aware policy authorizes actual arguments. Classify effects
  for the entire operation, including guest starts and capability use.
- **Pipeline preflight:** graph/types, inputs, selected output, planning grants,
  component links and ABI are checked before guest instantiation.
- **Runtime authority:** capabilities must be installed separately on the executing
  Cell; tool approval/planning grants cannot install imports or override denial.

Require empty `ExecutionResult.errors` and a present selected output. Choose which
artifacts to expose; earlier guest effects are not rolled back on failure. Native
providers/adapters remain trusted code. [Integration](integration.md#execute-a-typed-pipeline-through-cell)
contains the complete component contract and example pointers.

One outer tool attempt can execute several nodes: the harness step budget bounds
neither node count nor execution time. Prefer [managed execution](integration.md#managed-component-execution)
for component limits; it cannot forcibly interrupt native providers or bound
compilation. `ToolExecutionEvent` observes the outer call, not individual nodes;
`completed` is not an event stream/replay log. Payload-rich traces require a separate
privacy/retention policy.

## Snapshot/edit contract

App saving uses checked existing-file replacement. `Workspace::save_file` remains
the legacy unchecked create/write API; opt into `save_file_checked` for revision
checks. Write approval cannot bypass workspace capability denial.

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

## Model-independent harness discovery

With `features = ["tools"]`, use `scope_ide::tool::AgentSession` to register
Scope and downstream `ToolProvider`s. `session.catalog()` returns an owned
`SessionCatalog` snapshot of the tools actually registered in that session:

```rust
let mut session = scope_ide::tool::AgentSession::for_app(&mut app)?;
let catalog = session.catalog();
if let Some(entry) = catalog.lookup("scope.document.read") {
    // Present entry.definition, entry.provider, and entry.output_description
    // to your own decision adapter.
}
let discovery_payload = catalog.to_value();
```

`to_value()` exports a dependency-free `ToolValue` object with a `tools` array.
Each entry has `definition` (name, description, effect, inputs), `provider`, and
`output_description`. Input definitions retain name, kind, description, and
requiredness. Effects and kinds use lowercase strings; unavailable metadata is
`Null`. Output descriptions are advisory prose, not validated output schemas.
Providers can override the default `provider_name()` and
`output_description(tool_name)` methods without changing their tool definitions.

The catalog describes exposed tools, not all installed Cell interfaces and not
permission to execute. It is a snapshot; request a fresh catalog when needed.
Provider metadata and effect declarations are trusted host metadata, not proof
of isolation or behavior. No tool calls or model requests occur during discovery.

For argument-aware authorization, call `invoke_with_call_policy` or
`invoke_observed_with_call_policy`. Their policy receives `(&ToolDefinition,
&ToolCall)` immediately before dispatch, allowing path- or argument-specific
checks without cloning the proposed call. The existing definition-only policy
APIs remain available. Unknown tools return `ToolError::UnknownTool` without
calling policy; rejected known calls return `ToolError::Denied` without invoking
the provider. Observed dispatch reports both outcomes without recording payloads.

Downstream hosts own decision adapters, approvals, budgets and completion criteria;
see the [bounded model adapter](#bounded-llm-decision-adapter) for Scope's optional loop.

## Refinement-aware semantic edit budgets

Candidate evaluation is a purpose, independent of side effects. Mark a read-only
operation with
`operation.with_purpose(SemanticOperationPurpose::CandidateEvaluation)?`.
This stores `scope_operation_purpose` in the existing constraints object; the
semantic adapter exposes normalized `purpose` catalog metadata. Missing or unknown
purpose retains legacy exploration accounting. Keep returning typed `Evaluated`
evidence and declaring `semantic_operation_produces_evaluation` as before: purpose
alone never establishes validation, grants authorization, or permits writes.
Non-object constraints cannot be annotated with the builder; migrate them to an
object explicitly rather than silently replacing them.

`EditBudgets` defaults to 3 exploratory inspections, 2 candidate refinements,
1 initial proposal, 1 proposal correction, 6 model invocations, and no tolerated
duplicate calls. Marked evaluations have a total cap of `1 + refinements`;
`refinements` counts dispatched evaluations after unresolved candidate evidence,
not proposals or protocol retries. A failed evaluation at that cap returns
`RefinementBudgetExhausted` with `refinement_stopped=refinement_budget_exhausted`.
An extra exploration call returns `ExplorationBudgetExhausted`; reaching the
exploration limit does not hide evaluation tools or force the proposal phase when
an evaluator is available. Return `complete` to advance to proposal creation.
Languages without marked evaluators retain the existing automatic phase advance.

Every decision, including completion, discovery, suppressed duplicates and protocol
recovery, remains bounded by both `model_invocations` and transport `max_decisions`
(default 12). All dispatches and discovery calls share transport `max_tool_calls`
(default 8); malformed recovery remains bounded by `max_malformed` (default 2).
Global exhaustion returns `OperationBudgetExhausted`. No additional lifecycle or
retry mechanism is introduced. Two inspections, two evaluations, a completion and
a proposal require **7 model invocations**: explicitly configure at least 7 for
that workflow (and sufficient transport limits). Phase allowances never override
the global limits.

Failed marked evaluations retain a focused continuation containing the exact
candidate invocation, typed evaluation, provider observations, remaining refinement
opportunities, and next action, without unrelated inspection results or model
reasoning. Duplicate suppression still compares the entire call and relevant host
state: a revised candidate is distinct, an identical unchanged call is suppressed.
`EditRun`, `EditSummary`, and trace events separately expose inspections,
evaluations, refinements, proposal attempts, and proposal corrections; protocol
retries remain separately observable. Candidate success does not authorize or
apply a proposal, which still goes through host policy and normal validation.
Downstream exhaustive outcome/event matches and complete `EditBudgets` literals
must accommodate the new variants/field; prefer `..Default::default()` for budgets.

## Isolated text proposals

Start the interactive shell (`scope-ide --workspace <root> open`) with feature
`agent` enabled for model calls. Approval is a human action, separate from model
output; a request does not approve paths. For example:

```text
proposal src/main.rs src/new.rs
proposal-agent Implement the requested change in these approved files
proposal-review
proposal-accept
```

Use `proposal-discard` instead to drop the isolated snapshot. Only one proposal
is active per shell; it is not persisted and is dropped on exit. Normal `agent`
commands remain read-only. The proposal provider registers only list/read/create/
replace tools, never acceptance, shell, build, or arbitrary native providers.
Review shows a changed-file listing and complete before/after text, not a unified
line diff. A budget stop or model error leaves the proposal available for review;
it is not a successful completion or approval.

Public APIs (no optional feature required for snapshots):

```rust
use scope_ide::{path::ScopePath, workspace::ProposalLimits};
let paths = vec![ScopePath::new("src/main.rs")?, ScopePath::new("src/new.rs")?];
let mut proposal = app.proposal(&paths, ProposalLimits::default())?;
let changes = proposal.changes()?;
// Human review and explicit approval occur here, outside the model.
let result = app.accept_proposal(&mut proposal);
// Always inspect BOTH result.applied and result.error.
```

With `tools`, install `ProposalToolProvider::new(&mut proposal)` in a fresh
`AgentSession::builder()` with **no baseline providers**. Hosts supply dispatch
policy explicitly; do not infer approval from model arguments. With `agent`,
`CellAgent::run_proposal` installs only that provider and uses the existing bounded
loop. `scope.document.create` takes `path`/`text` and refuses overwrite.
`scope.document.replace` keeps the existing zero-based half-open line/byte-column
edit convention, but takes an integer `expected_revision` returned by proposal
reads; it always edits the isolated snapshot, with no `save` argument. Proposal
revision numbers are not baseline disk revisions. `Proposal::read`, `create`,
`replace`, `changes`, `discard`, and `accept` also support model-independent hosts.

The snapshot contains only exact approved files, not a recursive workspace copy.
Absent files can be approved for creation, but their parent directories must
already exist. Unapproved paths, traversal, symlink leaves/parents (including
dangling links), special files, and path components `.git`, `target`, `build`,
and `node_modules` are rejected. Defaults: 64 paths, 256 KiB per file, 2 MiB
aggregate text; both original and proposed text are retained (up to twice the
aggregate bound, plus review/tool output copies). Oversize, invalid UTF-8 and
NUL-containing binary files are rejected, never silently omitted. No deletion,
directory creation, rename, binary patching, or permission-change support.

Acceptance compares original text/absence for **every changed path before any
write**, then checks again before each write. New files use exclusive creation;
replacements are sequential writes, not atomic compare-and-swap or multi-file
transactions. `Acceptance.applied` includes paths opened for mutation even if a
write later fails; `error` reports conflicts/I/O failures, including partial apply.
Successfully applied paths are rebased in the retained proposal on partial failure;
a partially written path remains conflicted and needs human inspection. Discard
never rolls back already-applied writes. Same-content external edits are not
distinguished from the original baseline.

This is a trusted-host workflow using host filesystem APIs for explicit human
acceptance, not a sandbox against concurrent hostile filesystem mutation.
Symlinks are checked at snapshot and acceptance, but these pathname checks are
**not race-resistant**: a leaf, parent, or root-ancestor rename/symlink swap
between check and open can redirect reads or writes outside the approved root.
Exclusive new-file creation prevents overwriting an existing final entry, not
parent-directory redirection. Hard links are not detected; replacement truncates
the existing inode and can affect other links. Use only a trusted filesystem,
without concurrent hostile mutation. Content changes between the last check and
write can also be overwritten. A read-only baseline cannot be accepted. App acceptance
refuses dirty active buffers and never silently replaces any open buffer; reopen
accepted files to refresh them. Disk contents, including pre-existing user edits,
are the snapshot baseline; unsaved editor text is neither copied nor overwritten.

## Compact discovery and bounded compound tools

[CE1 context economy](CE1_CONTEXT_ECONOMY.md) documents the opt-in compact
investigation catalog and canonical detail requests. Discovery remains metadata,
not authorization. Its measured message-content bytes are not token counts or
proof of improved model task completion.

[CE2 bounded workflows](CE2_AGENT_WORKFLOWS.md) uses `AgentWorkflow`,
`AgentWorkflowContext::invoke`, and `run_session_with_workflows` in the existing
loop. Each outer attempt **and each inner attempted call** consumes the shared
`max_tool_calls`; inner calls re-enter the actual call-aware policy and must be
Read. Inner argument/result bytes also consume run allowances. The context exposes
neither raw session nor App; workflows cannot recursively dispatch workflows.
Adapters remain trusted code and must terminate. Result-size checks happen after
provider return and cannot bound allocations or undo effects. This is not an
arbitrary agent-authored Script execution permission.

[CE3 evidence-guided refinement](CE3_EVIDENCE_GUIDED_REFINEMENT.md) distinguishes
provider rejection evidence from model completion and proposal acceptance. A
successful returned envelope does not by itself prove a candidate. Interpret the
actual provider contract; do not infer generic `Evaluated` evidence from a domain
status string. Retain full source-bound evidence separately from compact model
feedback. Protocol traces are opt-in sensitive payloads; metadata-only tool events
are not replay logs, and trace writer failure is not required durable persistence.
