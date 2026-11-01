# H2: revision-bound semantic context

## Baseline audit

The ordinary path is `AgentSession` → `run_session[_observed]` →
`execute_session` → `DecisionModel::decide` → the existing Cell chat adapter.
`AgentSession::catalog()` snapshots installed providers. `SessionCatalog::to_value()`
exports tool definitions, argument contracts, provider/output descriptions and
provider metadata. The runner serializes `{kind: request, request, catalog}` as a
user message following a fixed system protocol. Each decision and structured tool
feedback extends that same conversation. No second agent loop is involved.

Entry-point differences matter:

- `CellAgent::run_app` first renders `AgentContext`: workspace paths, the full open
  in-memory document, diagnostics and outline, plus a static command description.
  This prose is not a revision-attributed semantic contract. Static commands can
  duplicate or disagree with actual session discovery.
- `run_retained_scratch_agent_observed` supplies the caller's request and filtered
  semantic catalog, **not** the retained source text or earlier semantic results.
  Its immutable scratch borrow pins text, language, owner-local ID and revision.
  Its private evaluation identity distinguishes independent buffer owners.
- `InspectionBinding` already captures exact document text/revision and operation
  description and rechecks both freshness and authorization when invoking. Its
  narrow discovery bridge supports read/document operations without arguments;
  it is not a general parameterized-operation source registry.

`SemanticToolProvider` owns semantic source preconditions, argument projection,
validation, explicit authorization and post-operation freshness checks. Its
`SemanticOperationDescription` carries purpose, effect, target requirement,
argument/result metadata and invocation schemas. These facts exist before a model
request. Generic ordinary context previously did not tie them together with the
source snapshot, evidence selection or completeness.

`ToolValue` is the existing model transport (including signed integer but not
floating-point values). Native results can contain information that cannot be
transported losslessly. Semantic feedback becomes JSON **text** in a user-message
envelope, rather than a model-native semantic object. `Ok` dispatch is execution
evidence, not downstream domain verification. H1's `AgentEvidence::Tool.output`
is trusted in-process data and skipped during serialization. `AgentTurnStore`
retains sanitized append-only snapshots and `AgentTurnToolProvider` exports those
same records; neither exports protected native payloads.

`scope-meta` reexports nominal `TypeId` identities. Provenance and completeness
there are descriptive, not authorization, freshness or mathematical truth. H1's
`scope.agent.AgentTurn@1` discovery identity remains the retained-turn identity;
H2 adds evidence variants rather than a registry or context graph.

The runner already says discovery is not authorization, but previously supplied
no enumerable permission description. An arbitrary `FnMut` policy cannot safely
be queried or reverse-engineered into a grant list. Message and conversation
limits are checked by the existing push/accounting path before dispatch. These
count accepted UTF-8 message content and existing workflow charges, not tokens,
HTTP framing or cumulative re-transmission. Initial full catalogs are all-or-stop,
not selectively bounded. Source/prose rendering in `run_app` can be redundant or
larger than necessary; this experiment does not measure that different entry point.

Relevant implementation: `crates/scope-ide/src/{agent.rs,agent_loop.rs,tool.rs,
semantic_inspection.rs,agent_lifecycle.rs}`, `tool/semantic.rs`,
`tool/agent_turns.rs`, `cli/retained_agent.rs` and `crates/scope-meta/src/`.

## Minimal contract and selection

`crates/scope-ide/src/semantic_context.rs` composes existing
`AgentSourceBinding`, `SessionCatalog` and `AgentLimits`. `SemanticContextBuilder`
produces an immutable packet, a model-facing JSON value and a payload-free summary.
There is no source loader, evidence database or new execution framework.

The packet contains:

- A host purpose label and exact owner-local source identity, revision and language.
- Existing execution limits and a separate packet/source/operation byte cap.
- `Unknown` or explicitly `HostDeclared` advisory permissions. Declarations never
  grant capabilities, even when a tool is listed as permitted.
- Safe operation definitions with argument kinds, required keys and descriptions.
- Exact UTF-8 source content or a strict prefix, with `complete`, `bounded`,
  `omitted` or `unavailable` completeness. Empty complete source is different from
  unavailable source. A bounded prefix is never labeled complete.
- Explicit host-selected evidence **references**, each with operation, its own
  source binding and an authority category: execution, downstream verification,
  computation or model claim. References are not dereferenced automatically.
  No selection means `unavailable`; an explicitly empty selection means complete
  **for that host selection**, not that no evidence exists anywhere.
- Omitted operation and metadata counts. Provider metadata is intentionally omitted
  wholesale rather than copying arbitrary provider payloads into model context.

Selection is intentionally simple: the separate user request is mandatory; packet
purpose/binding/permissions/limits and explicit evidence selection are mandatory;
whole operation records are considered in stable name/canonical-value order;
source receives remaining space. Operation descriptions and the entire packet
have serialized JSON byte caps. Argument contracts are never clipped mid-record.
UTF-8 source clipping also accounts for JSON escaping. The same ordered evidence
selection, source, catalog, permission description and options produce the same
packet. Histories are not expanded automatically. Mandatory overflow returns a
structured builder error; a message/conversation overflow uses H1's ordinary
budget stop without invoking the model. Nothing drops the task to fit a packet.

This is a transport-selection policy, not a relevance ranker. Operation omissions
are counted; individual omission reasons are the per-record or whole-packet cap.
All provider metadata is intentionally excluded, including invocation schemas
that some complex tools may need. Such interfaces may therefore be insufficiently
described by this initial projection: do not claim general tool-contract fidelity
for schema-dependent operations. The Path fixture takes no arguments and does not
need those schemas. A narrowly allowlisted safe schema projection is future work.

Host-supplied descriptions, purpose and reference IDs must already be suitable for
public/model context. The builder cannot detect a secret deliberately placed in a
public description; it does not inspect credentials, provider settings, raw traces,
H1 native output or hidden metadata. Evidence references carry attribution, not a
claim that this generic builder independently checked their authority.

## Ordinary runner integration and freshness

Existing entry points retain their default behavior. Opt in through:

- `agent::run_session_semantic_observed`, for hosts supplying a built packet and
  the current binding.
- `cli::run_retained_scratch_agent_semantic_observed`, which constructs the packet
  directly from the borrowed scratch source and current filtered session catalog.

The semantic path substitutes `{kind, request, semantic_context}` for the initial
full-catalog envelope. It uses the same protocol/adapter, loop, per-call policy,
source checks, budgets, feedback and terminal lifecycle. A system sentence labels
semantic context as untrusted data rather than instructions or authority. Source
instructions or permission declarations cannot override dispatch authorization.

Generic hosts are responsible for providing the current owner-local binding and
pinning their source during invocation. An unequal identity/revision/language is
rejected before model dispatch; the API does not reload or rebuild a stale packet.
A serialized binding is not globally unique across owners. The retained convenience
entry point provides the stronger invariant: assembly and invocation happen under
one immutable scratch borrow, with the existing private evaluation identity kept
for `source_is_fresh`. It cannot accidentally substitute a newer revision.

Retained authorization remains `Unknown`, even with an allowlist: the allowlist is
a ceiling and the additional opaque semantic authorization callback can still deny
any call. No policy closure is evaluated during context assembly.

The semantic retained branch currently captures the source-binding trace event,
not rich protocol JSONL. It documents this limitation rather than pretending that
the capture is a full transcript. The measurement wrapper in the experiment uses
ordinary model messages directly and retains counts only.

### Lifecycle and safe non-UI inspection

No new lifecycle state. `ContextAssembly` is compact serialized `AgentEvidence`
with success, serialized packet bytes, budget-bounded status and fixed sanitized
failure category. Its revision is the turn's source binding. A stale binding or
builder failure produces a failed terminal callback and no model request. Builder
callers retain the detailed mandatory-overflow error; public failure metadata is
intentionally generic. Absence of assembly evidence denotes a conventional turn.

`ContextSupplied` records exact packet and initial user-message content bytes at
the first model-dispatch boundary. It does not retain source, task or full packet.
A packet assembled but blocked by a message/conversation cap has no supply record.
Zero decisions does not itself mean assembly failed. Supply evidence is recorded
without a callback between the final cancellation check and model invocation;
callbacks see it with the response notification. This avoids letting a callback
cancel at a misleading pre-dispatch “supplied” notification. As with existing
blocking calls, cancellation cannot interrupt a call already dispatched.

`AgentTurnStore` and `scope.agent.get` retain/export these evidence variants through
the existing sanitized path. Import validation checks ordering, successful assembly,
first-request delivery, sizes and uniqueness. Older H1 records still deserialize.
Consumers compiled against the old exhaustive Rust evidence enum need to handle
the added variants; the full packet and native results remain absent from snapshots.

## Controlled Path experiment

Extended the existing `examples/h1-path-agent` harness rather than adding an agent
framework. `fake` versus `fake-h2` uses the same deterministic two-decision model,
exact borrowed Path fixture, task, native `path_scope::LANGUAGE`, authorization
callback and execution limits (3 decisions, 1 tool attempt, 1 malformed response,
320 output tokens, 65,536 message bytes, 196,608 conversation bytes). The native
kernel actually runs in both variants. The script does not evaluate prompt quality.

Reproduce, with the existing sibling Path checkout and cached dependencies:

```sh
cargo build --offline --locked --manifest-path examples/h1-path-agent/Cargo.toml
examples/h1-path-agent/target/debug/scope-h1-experiment fake
examples/h1-path-agent/target/debug/scope-h1-experiment fake-h2
```

Sanitized results: `examples/h1-path-agent/evidence/h2-baseline-summary.json` and
`h2-semantic-summary.json`. Path remains a local working-tree dependency, not a
pinned external service; the harness README describes the layout. No downstream
repository changes were made. Historical H1 evidence was not overwritten.

| Measurement | Baseline | H2 |
|---|---:|---:|
| First user-message content bytes | 1,355 | 1,566 |
| Full first model input bytes, roles + contents | 2,768 | 3,046 |
| Cumulative input bytes across model requests | 12,129 | 12,685 |
| Semantic packet bytes | none | 1,239 |
| Source completeness | source not supplied/unspecified | complete pinned fixture |
| Evidence selection completeness | unspecified | unavailable; none imported |
| Provider metadata completeness | full exported metadata | intentionally omitted |
| Available operations | 1 | 1 |
| Authorized operation description | unknown | explicitly unknown |
| Successful host authorizations | 1 | 1 |
| Model decisions | 2 | 2 |
| Tool attempts / semantic successes | 1 / 1 | 1 / 1 |
| Domain verification | native kernel checked, proof check passed | same |
| Source freshness | revision 0, unchanged, fresh | same |
| Lifecycle integrity | store/get/import passed; no publish errors | same, plus assembly/supply evidence |
| Terminal outcome | Completed | Completed |
| H1 charged conversation bytes | 9,416 | 9,694 |

The wrapper counts role strings plus contents for model-input measurements, **not**
wire bytes. `initial_user_message_bytes` and `ContextSupplied.initial_message_bytes`
count just serialized user content. H1 usage counts accepted content once, while
the cumulative input measurement counts repeated conversation on each call.
The legacy `initial_model_message_bytes` field measures the first/system message
including its role; it is not the whole initial context. Byte counts are not token,
cost or latency measurements.

H2 adds 278 first-input bytes and 556 cumulative input bytes (about 4.6%). It does
not reduce prompt material here. It supplies structured purpose, source, revision,
completeness and permission uncertainty that the baseline omits. Metadata removal
also removes information; that tradeoff is explicit, not an assumed quality win.
There is no observed behavior or task-performance improvement: the deterministic
model makes identical choices. The new overhead is one bounded packet selection,
additional JSON fields and compact lifecycle evidence—not another runner.

This is **compiled fixture inspection**, not proof synthesis, editing or verification
of arbitrary changed source. Only the downstream kernel result supplies domain
verification; model completion and successful dispatch do not. No earlier native
result was automatically imported into initial context. Safe retrieval/classification
of general retained domain observations is still blocked by the lack of an explicit
safe authorized native-result projection; H1's sanitized snapshots intentionally
do not contain those results. H2 exposes host-selected references, not a bypass.

Optional `real` / `real-h2` modes use the existing local `qwen3-30b-a3b` configuration
without changing generation settings. Neither local-model variant nor availability
probe was run for this milestone. No paid endpoint was used. The earlier H1 run
is not evidence of an H2 model-quality improvement; stochastic paired repetitions
would be needed to investigate that question.

## Regression validation

- `cargo test -p scope-ide --lib --features agent --offline`: 646 passed at the
  final focused-suite run. Includes stable ordering, revision attribution, UTF-8
  and escaped-byte cap boundaries, mandatory overflow, whole-record omission,
  known versus opaque permissions, safe evidence reference provenance, protected
  metadata/output exclusion, stale binding and assembly failure, message and
  conversation caps, H1 retained round-trips, cancellation and exact supply bytes.
- Adversarial source requests to ignore authorization plus a host-declared tool
  permission still meet a denying dispatch policy; provider execution stays zero
  and only denied execution evidence is retained.
- Offline locked Path build and both deterministic variants passed against the
  real downstream service. Safe summaries are checked independently of model prose.
- `cargo test --workspace --offline` reached the existing PTY launcher integration
  test `source_proposals_through_real_launcher` and exceeded the 200-second bound.
  Earlier displayed suites passed; the full workspace command is **not** reported
  as passed. No unrelated launcher changes were made.

## Research conclusions and H3

1. **What additional structured knowledge?** Scope now supplies exact target
   identity/revision/language, purpose, bounded source selection, discovered
   contracts, explicit permission uncertainty/declarations, attributed evidence
   references, completeness/omissions and byte limits without asking the model to
   reconstruct those facts from prose. It does not infer missing domain evidence.
2. **Quality, efficiency or performance?** Attribution and selection inspectability
   improve measurably; efficiency does not on this fixture (556 extra cumulative
   bytes). Scripted task performance is identical. Model-context usefulness and
   real-model task performance remain unestablished.
3. **Most useful abstraction?** The retained `SourceSnapshot`/scratch binding,
   backed by the existing semantic provider's source preconditions, provides the
   key exact-revision invariant. `SessionCatalog` and H1 evidence make it possible
   to integrate that invariant without a new registry or runner.
4. **Smallest justified H3 step?** Add one explicit, host-authorized safe projection
   of the Path fixture's kernel classification, attributed to its operation and
   source revision, then test a bounded ordinary continuation that distinguishes
   verified, rejected and unavailable results. Keep the existing loop and budgets.
   Do not turn tool success into proof or build a general verification framework.
