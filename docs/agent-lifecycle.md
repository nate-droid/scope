# H1 — typed agent turns

## Status and scope

The ordinary bounded `AgentSession` decision runner now produces one authoritative typed lifecycle. Ordinary, traced, observed, and compound-workflow entry points use that same owner. Retained scratch semantic asks bind it to the original source identity/revision; the workspace workbench renders its observations. Completed records can be inspected without the UI or a model.

**This is the reusable H1 core, not complete unification of every agent entry point.** Specialized `EditRun` editing/investigation and legacy one-shot `CellAgent::answer*` paths remain separate existing contracts. They have not been relabeled as typed turns. Proposal creation/review references, declarative authorization descriptions, and full execution-configuration descriptions are not yet bridged into `AgentTurn`. Consequently the milestone's broadest acceptance criteria, particularly inspection across proposal-producing execution paths, remain open. No second proposal lifecycle was introduced to conceal that gap.

## Architecture audit

| Contract | Existing reusable functionality | H1 adaptation / remaining limitation |
| --- | --- | --- |
| `scope-types::{Registry, TypeId, Port, Transformation}` | Versioned nominal identities, typed transformation ports, graph validation and planning | Reuse `TypeId` through `scope_meta` for turn discovery metadata. A retained execution is not a transformation; no fake registry entries or parallel schema system. |
| `scope-language::SemanticOperationDescription` | Language-local operation discovery, argument kinds, effects, target requirements, evaluation purpose | Remains downstream-owned. Semantic operations have JSON argument/result contracts, not nominal result ports. No speculative extension of the general type system. |
| `scope-ide::tool::{ToolProvider, AgentSession, SessionCatalog}` | Namespaced discovery, structured `ToolValue`, explicit per-call policy, typed dispatch errors | Read-only `AgentTurnToolProvider` uses the same catalog and invocation path. Inspection is not tied to a language service. |
| `SemanticToolProvider` | Source preconditions, per-operation authorization, native semantic outcomes, proposal registration via existing controller | Unchanged authority. Retained runner pins original source and projects arguments. No tool inputs or native outputs published by introspection. |
| `scope-meta` | Existing `TypeId`, graph entities/relations, source locators, producer provenance/completeness | Not needed as a second retained-turn store or workflow graph. Nominal output metadata alone does not validate payloads. |
| Source attribution | Tagged IDE source IDs, owner-local revisions; separately process-local language source identities | Retained asks record tagged scratch ID, revision and language. IDs are owner-local, not durable filesystem identities. Exact text remains in the existing borrowed snapshot, not the public record. |
| Bounded decision runner | Model protocol, installed catalog, malformed correction, tool/decision/byte limits, workflow shared budgets | Owns `AgentTurn`, transitions, usage, model response metadata and typed dispatch outcomes directly at execution boundaries. |
| Existing tool events | Started/Finished and typed dispatch categories | No inference from submitted calls. Turn-level and workflow inner-call evidence is recorded at the runner's invocation boundaries. |
| Protocol JSONL | Explicit, sensitive, opt-in raw capture; bounded retained scratch capture | Remains diagnostic evidence. Summaries no longer reconstruct lifecycle or operations from traces. Old traces do not become fabricated typed histories. |
| Proposals | Existing controllers, validation, freshness, explicit review/acceptance | Unchanged. Read-only retained asks expose no proposal tools. Specialized proposal runners are not yet adapted to this turn envelope. |
| Workbench | Borrowed synchronous execution, output transcript, ratatui pane rendering | Same turn snapshots redraw the ordinary output area at execution boundaries. No worker scheduling or separate UI lifecycle. |
| Cancellation | Synchronous trusted models/providers must return themselves | Cloneable cancellation request checked before/after calls; never claims to interrupt an external call. |

### Genuine interface blockers

* A policy closure cannot enumerate its authorization decisions in advance. The installed catalog is **not** an authorized-capability list. Cell grants and provider authorization remain separate; a future description must be supplied by the host, never reverse-engineered from discovery.
* Models expose arbitrary `trace_settings()` for explicitly sensitive tracing. Introspection deliberately does not copy that object. Safe model/provider identity and token/count/byte limits are available; complete safe generation settings need an explicit adapter-owned contract.
* `ToolValue` cannot losslessly represent all downstream JSON numbers. Lifecycle keeps the existing tool transport output in-process, not a universal new evidence schema. `SemanticToolProvider::native_result()` and existing downstream evidence remain authoritative for lossless semantic observations.
* Editing/investigation has real planning phases and proposal validation that are not equivalent to generic decision-loop states. Bridging its observed boundaries requires an explicit adapter, not retrospective event inference.
* Existing workbench execution borrows source, model, service, and authorization synchronously. Redraw callbacks are possible; responsive keyboard cancellation during an in-flight call is not. Thread-safe owned execution is not established by these interfaces.

## Typed model and ownership

Implementation: `crates/scope-ide/src/agent_lifecycle.rs`, integrated by `agent_loop.rs`.

`AgentTurn` has private fields and owner-private mutations. Public code reads snapshots; imported records are observations, not runnable handles. Each runner invocation allocates a monotonically increasing process-local ID. Importing a retained record does not resume it or allocate capabilities. Do not merge records from unrelated processes or owners using the numeric ID alone.

States:

* `Ready`: created, before context/model execution.
* `Deciding { request_ordinal }`: the runner reached a model-request boundary, not proof that the request returned.
* `ToolRunning { request_ordinal, call_ordinal, name }`: the runner reached an attempted operation boundary, not proof of authorization or success.
* `Completed`: the protocol accepted a completion decision.
* `BudgetExhausted(AgentStop)`: decision, tool, malformed, message or conversation budget stopped execution.
* `Cancelled`: the owner confirmed termination at a cooperative boundary.
* `Failed(AgentFailure)`: categorized backend/configuration/setup/application failure without copying sensitive error text.

`AgentTransition` records turn ID, ordered transition ordinal, previous/next states, trigger and request/call attribution. Source attribution belongs to the enclosing immutable turn, avoiding redundant per-event copies. Duplicate transitions, invalid edges and terminal mutation are rejected. Retained deserialization validates chains, attribution, evidence and usage consistency. Record loading is deterministic inspection, **not model replay**.

`AgentEvidence` records requested decisions, returned/error model response metadata and byte length, malformed decisions, and tool outcomes attributed to requests/calls. `AgentToolOutcome::Success` means provider `Ok`, not domain acceptance. Native `ToolValue` output is retained for trusted in-process readers but skipped during serialization and import. Model prompts, response text, tool arguments, provider error messages, credentials and HTTP settings are not part of the public envelope.

Usage records decisions, tool attempts, malformed decisions and accepted conversation/charged workflow bytes. These are neither billing-token counts nor timeouts. Rejected oversized messages are not charged as accepted context. Tool-start transitions and tool-result evidence are distinct, including when cancellation prevents dispatch.

**Completion ≠ domain verification ≠ human approval.** Path's kernel and other downstream services still own semantic verification. Scope still owns mutations and proposal acceptance.

## Semantic inspection

Implementation: `crates/scope-ide/src/tool/agent_turns.rs`.

The host creates `AgentTurnStore`, publishes observed snapshots, and installs `AgentTurnToolProvider` into an ordinary session. The store retains sanitized serialized observations only. Publication accepts duplicates and append-only progress, checks monotonic usage and immutable bindings, and rejects delayed regressions or changes after terminal publication.

Discoverable read operations:

| Name | Input | Result |
| --- | --- | --- |
| `scope.agent.active` | none | Newest retained nonterminal turn, or null |
| `scope.agent.list` | none | Retained turns in ascending ID order |
| `scope.agent.get` | `id`: decimal u64 string | Exact retained sanitized turn; unknown ID is an argument error |

The host decides which observations to retain and expose. The store is not a global registry, scheduler, durable session database or authority service. It currently has no automatic eviction; use a per-session store and host-selected retention. An abandoned host must not leave its last nonterminal observation advertised as proof of ongoing external work.

A non-UI consumer uses the existing tool contract:

```rust,ignore
let store = AgentTurnStore::new();
// Inside run_session_observed / run_retained_scratch_agent_observed:
// store.publish(turn)?;
let mut inspection = AgentSession::builder()
    .provider(AgentTurnToolProvider::new(store.clone()))
    .build()?;
let value = inspection.invoke(
    ToolCall::new("scope.agent.get")
        .with_argument("id", completed_turn_id.to_string().into()),
)?;
```

Runnable offline demonstration:

```sh
cargo run --locked --offline -p scope-ide --example retained_semantic_agent --features agent
```

This runs an ordinary stream-shell semantic ask with a deterministic model, then inspects its completed turn through a separate tool session. No downstream language changes are needed.

### Authorization boundary

The inspection provider has no publish, invoke-agent-tool, accept-proposal, resume, save, or cancel operation. Snapshot contents are data, never reusable execution grants. The execution policy and semantic authorization callback are still consulted on each actual dispatch. Sensitive native output is available only to the trusted host's in-process evidence reader, not to discovery or inspection tools. Safe adapters must also avoid putting secrets in their explicit identity strings.

## Workbench behavior

`cli/workspace_command.rs` exposes an observation path to the existing document ask runner. `tui/composition.rs` redraws the existing output region from those callbacks, preserving the rest of the last workbench frame. `composition/progress.rs` formats the same authoritative state, model/provider, bound source revision, most recent operation and usage.

At **80×24** and **120×30**, tests render actual observed model → operation submitted → operation result → completion snapshots. Submitted calls explicitly say "not success". Names/identities are bounded and terminal controls escaped. Native output and model claims are never used as progress text. The retained ordinary transcript uses the same turn evidence, not JSONL reconstruction.

These are synchronous boundary updates. Input/cancellation is unavailable during blocking calls; no background execution or asynchronous interruption is claimed. Detailed structured histories remain available through the programmatic provider and retained typed record; raw JSONL is optional debugging data, not required for the ordinary summary.

## Retention and provenance

`AgentRun.turn` retains the final owned record. `run_session_observed` and `run_session_traced_observed` deliver terminal observations even when the runner returns `Err`; hosts must publish/copy them in the callback if failed-run retention is required. Source/service/provider construction errors before runner entry have no allocated turn. The default unobserved API still returns the original error contract.

`run_retained_scratch_agent_observed` preserves the original immutable source borrow, allowlist, per-call authorization, trace cap, and result/error behavior. `source_is_fresh(&ScratchBuffer)` compares the existing process-local evaluation source identity as well as tagged owner-local identity, revision and language. Equal numeric scratch IDs from different buffer owners are not considered fresh. Editing the source later makes retained attribution stale; inspection never substitutes a new revision or reloads a disk path. The semantic provider's existing invocation and post-execution freshness checks remain authoritative.

Sanitized records can be serialized and validated on import. They omit native outputs by design: durable full-evidence retention remains the host/downstream's existing protected responsibility. No universal export of sensitive verification payloads is added. Old raw JSONL lacks full typed boundaries; there is no automatic migration or claim of complete history. Serialization is an H1 format, not a promised long-term stable storage protocol.

## Validation and real exercise

Offline regression coverage includes lifecycle edge enforcement, duplicate events, terminal immutability, cancellation before/after blocking calls, retained returned tool evidence on cancellation, each budget category through existing runner tests, model failures, model-response retention, source staleness, denied dispatch, sanitized publication, delayed observations, validated loading, and compact rendering.

* `cargo test --locked --offline -p scope-ide --features agent` passed during integration; final `--lib` run: **636 passed**.
* `cargo test --locked --offline -p scope-ide --features tui,agent --lib`: **716 passed**.
* `cargo fmt --all -- --check` and `git diff --check` passed.
* The offline `retained_semantic_agent` example ran successfully and inspected its retained turn through `scope.agent.get`.
* The full workspace test command exceeded its 120-second limit while running existing downstream-launcher PTY coverage; this is not a complete workspace pass.

See the tracked [Path harness and retained evidence](../examples/h1-path-agent/README.md). A local LM Studio model performed a real kernel-backed Path operation; an independent tool session inspected the completed turn. A deterministic decision model exercised the same real service without a model dependency. The example is optional and outside the root workspace, so routine CI does not require Path or a running model.

## Semantic context and verification feedback

The [semantic agent guide](agents.md) describes opt-in bounded initial context with compact assembly and delivery evidence. The [Path feedback control](../examples/h1-path-agent/README.md#h3-feedback-driven-offline-control) exercises explicitly selected, revision- and invocation-attributed verification metadata through the ordinary tool-feedback path. `Completed` remains protocol completion, not domain success; native payloads remain excluded from public snapshots.

## Historical H2 prerequisites and deferred decisions

The next smallest improvement is a **host-supplied, revision-bound semantic context description using the existing catalog, source binding and retained evidence**, with explicit completeness and safe payload selection. It should distinguish installed operations from permitted operations, and committed-source evidence from candidate evaluation. Reuse `SessionCatalog`, `InspectionBinding` and existing semantic purpose metadata rather than creating a context graph framework.

Before claiming complete H1 coverage across all agent APIs:

1. Adapt existing `EditRun` and one-shot proposal/answer boundaries to this owner envelope, preserving their planning and proposal contracts rather than inferring progress afterward.
2. Attach existing proposal IDs and human-decision observations through trusted producer metadata; never guess review state from arbitrary tool JSON or model prose.
3. Add a host-owned safe authorization/configuration description. Opaque closures and arbitrary tracing settings cannot safely supply that information automatically.
4. Decide how hosts retain/retrieve protected native semantic evidence by reference without exposing sensitive outputs through ordinary read discovery.

Deliberately deferred: durable conversation sessions, resumption, model re-execution/replay guarantees, asynchronous task scheduling, unified cross-process IDs, new registries, downstream migrations and automatic orchestration.

### Final design answer

Scope can now describe **ordinary decision-loop execution** through its existing semantic tool discovery/invocation infrastructure: nominally identified typed turn records, observed transitions, source revisions, bounded usage and retained outcome metadata are shared by UI and non-UI consumers. The type system did not need to become a workflow engine. It does **not yet describe every specialized agent path or domain verification payload** through that envelope, and nominal `TypeId` metadata is not a general structural schema for semantic results. H2 should first bind safe, attributed context and evidence references to these existing contracts—not introduce another abstraction layer.

## Opt-in hypothetical construction objective

`run_semantic_investigation_with_objective_and_discovery` accepts
`SemanticInvestigationObjective::HypotheticalConstruction` to prioritize a checked
hypothetical artifact. Only the system objective changes: permitted read operations,
host authorization, source binding, discovery, argument schemas, budgets, duplicate
suppression and host-assessed completion remain unchanged. This grants no persistent
mutation authority, and prose or guidance is not checker evidence.

The existing discovery runner retains its exact investigation objective;
`Investigation` is the enum default. Existing signatures and options struct literals
remain compatible. This additive public API requires Full verification under
[the verification policy](verification.md); focused investigation tests alone do not
satisfy that obligation.
