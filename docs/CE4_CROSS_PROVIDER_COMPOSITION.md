# CE4 — Cross-provider typed composition

## Result

**A narrow real integration is demonstrated:** Scope Forth executes `: result 2 2 + ;`; an explicit authorized host adapter consumes its actual integer stack; independently developed Path kernel-checks a theorem comparing that integer with separately specified Church addition. Scope Script coordinates the calls. The same Script is available through CE2's normal opt-in `AgentWorkflow` and unchanged decision protocol.

No generic Scope type machinery, downstream semantics, kernel, or agent loop changed. All implementation is in `examples/ce4-composition`, a standalone workspace. This is not Cross, a universal arithmetic translation, automatic capability wiring, or a proof of arbitrary Forth programs.

## 1. Existing architecture audit

Inspected the SI contracts and implementations, CE1–CE3 docs/examples, Script hosts/interpreter, semantic tool adapter, agent workflows and source/evaluation contracts. Relevant implementation locations:

- `crates/scope-types/src/lib.rs`: nominal transformation registry and graph checking.
- `crates/scope-execution/src/lib.rs`: Cell-backed typed-port execution.
- `crates/scope-ide/src/tool/operations.rs`: recursive operation schemas and checked invocation.
- `crates/scope-language/src/semantic.rs`: language operation descriptions, argument validation and evaluation outcomes.
- `crates/scope-ide/src/tool/semantic.rs`: language-to-tool conversion, source preconditions, read-only enforcement and projections.
- `crates/scope-script/src/{host,semantic,runner}.rs`: registry/semantic Script hosts and retained runs.
- `crates/scope-ide/src/agent_loop.rs`: shared workflow authority and budgets.
- `crates/scope-ide/tests/structural_interoperability.rs` and `crates/scope-script/tests/contract_first_interoperability.rs`: SI structural comparison probes.
- `docs/design/INTEROPERABILITY.md`, `docs/SOURCE_EVALUATION.md`, `docs/CE1_CONTEXT_ECONOMY.md`, `docs/CE2_AGENT_WORKFLOWS.md`, `docs/CE3_EVIDENCE_GUIDED_REFINEMENT.md`.

### Answers to the nine audit questions

| Question | Existing capability and boundary |
|---|---|
| 1. Compose registered operations through Script? | Yes. `Host::invoke`, `RegistryHost`, `SemanticHost`, record/field expressions and `scope.invoke` support deterministic data-dependent composition. No new DSL is needed. The supplied semantic runner binds one language service and one target; a downstream Host can explicitly route independent providers. |
| 2. Compare output and input contracts? | `scope_types::TypeId` compares exact nominal names and opaque versions; transformation discovery finds explicit single-input routes. `OperationDescriptor` publishes recursive input/output `OperationSchema`. Existing SI structural comparisons are private tests, **not a public general assignability checker**. Language semantic descriptions have shallow argument kinds and no general recursive result schema. |
| 3. Structural guarantees? | Recursive registry schemas validate null/bool/integer/string/arrays/closed objects, required fields and returned values. They do not establish meaning, units or correctness. Nominal graph checking checks port identities, connections, requirements and cycles; route discovery is planning, not execution authority. |
| 4. Reject before execution? | Known nominal graph mismatches, invalid recursive registry inputs, shallow semantic argument errors, stale bindings and denied grants can reject before provider dispatch. Language result internals need explicit runtime validation; JSON shape alone proves no semantic compatibility. |
| 5. Explicit adapters? | Registry/semantic Script hosts and `SemanticToolProvider` adapt public APIs into Script/tool transport. `with_projected_arguments` supplies host-bound source preconditions. Nominal transformation declarations describe explicit conversion routes; they are not executable semantic conversions without bindings. There is no existing Forth-integer-to-Path-numeral adapter. |
| 6. Surviving identities? | Semantic envelopes retain operation ID and returned evaluation metadata; host snapshots retain source ID, version and text. Registry results retain document/revision/value. There is no uniform provider version or invocation ID. Script result capture is not a complete attempted-call ledger. Language source/revision IDs, edit SourceIds, session Revision tokens and nominal TypeIds are distinct namespaces, not interchangeable durable identities. |
| 7. Nested budgets and grants? | Every CE2 `AgentWorkflowContext::invoke` re-enters call-aware session policy and requires read effect. Outer and inner attempts share agent call limits; cumulative argument/result traffic shares byte budgets. Other workflows cannot be nested dispatch targets. Script has its own bounds. Return-byte checks occur after execution; they are not native allocator limits or cancellation. Planning capabilities do not install Cell runtime capabilities. |
| 8. Retain intermediate/partial failures? | Successful envelopes can be retained by Script and investigation evidence. Existing capture is not a universal attempt/failure/skipped ledger. CE3 distinguishes evaluation outcomes from agent completion. A small report-local extension is justified for CE4; no database is needed. |
| 9. Normal agent invocation? | Yes, a host-defined `AgentWorkflow` executes the existing interpreter using context dispatch. The generic runner need not know either provider. CE4 exercises this with a deterministic `DecisionModel`, not a live model. |

`scope_script::Value` is the existing `ToolValue`, not a separate universal type algebra. Tool transport supports signed i64, not floating point. Schema identity, structural shape, semantic meaning, successful execution and independent acceptance remain separate assertions.

## 2. Provider selection

Inspected real public operations in sibling Forth, Path and Hilb, plus music, garden and Glyph candidates. Forth search followed by Forth evaluation would reuse one provider/evaluator and does **not** meet the independence requirement. Hilb run/inspect similarly reads one simulator's evidence. Forth integers can control music transposition, but that establishes a different musical operation rather than independently checking the arithmetic property.

Selected independently developed `scope_forth::LANGUAGE` and `path_scope::LANGUAGE`. Path has no native integer arithmetic input contract. Its existing functions, transparent definitions, identity types and definitional equality can nevertheless check bounded Church-encoded arithmetic without changing its kernel. The mismatch is resolved by one small visible host adapter, not a universal translator.

Both dependencies are real sibling path dependencies; the lockfile does not make local checkouts portable or attest their versions. No sibling files were modified.

## 3. Concrete boundary contracts

### Producer

Public semantic operation: `language.example.forth.evaluate_candidate`.

Host-bound source: an immutable empty Forth scratch document. The candidate is a **hypothetical argument**, not a committed revision of that document:

```json
{"source":": result 2 2 + ;","tests":[{"input":[],"expected":[4]}]}
```

Its real read-only envelope includes `value.tests[0].actual: [4]`, test outcome, analysis, `evaluation.outcome: Satisfied`, rationale and diagnostics. The producer's expected stack is only a finite test oracle; the adapter reads **actual**, never expected. A negative control uses candidate `: result 5 ;` with expected `[5]`: Forth succeeds but Path rejects.

### Explicit adaptation

Host-owned contract identifier: `host.ce4.church-adapter.v1`. This is **not** a claimed provider version. The adapter:

1. Requires the exact completed preceding producer evidence, not a caller-invented passing envelope.
2. Requires a read-only result, exactly one passed test and one actual signed integer.
3. Requires `0 <= n <= 8`; strings, multiple values, negative and oversized values are not implicitly coerced.
4. Encodes `n` as exactly `n` applications of `f` to `x`.
5. Inserts that expression into a fixed Path theorem with independent left side `add two two A f x`.

The semantic bridge is explicit: a nonnegative observed Forth integer is interpreted as a Church iteration count. This is a trusted, inspectable host contract, not a semantic equivalence inferred from field names or a verified compiler.

### Consumer

Public semantic operation: `language.language.path.check_source`, arguments `{}`, bound to a newly created immutable Path scratch containing the **actual adapted text**. The fixed definitions are:

```text
def two (A : Type 0) (f : Pi (a : A). A) (x : A) : A := f (f x)
def add (m : Pi (A : Type 0). Pi (f : Pi (a : A). A). Pi (x : A). A) (n : Pi (B : Type 0). Pi (g : Pi (b : B). B). Pi (y : B). B) (C : Type 0) (h : Pi (c : C). C) (z : C) : C := m C h (n C h z)
theorem result (A : Type 0) (f : Pi (a : A). A) (x : A) : Id A (add two two A f x) (f (f (f (f (x))))) := refl (add two two A f x)
```

Path independently checks this submitted theorem. Success requires the read-only envelope, `value.outcome == "Checked"` and `value.kernel.checked == true`. A five-application right endpoint returns `MathematicalRejection` and `kernel.checked: false` despite successful Forth execution and completed tool transport. Kernel checking uses the provider's reported depth/tick limits; no kernel semantics changed.

## 4. Smallest implementation and actual Script

`examples/ce4-composition/src/main.rs` contains an example-local routing Host, adapter, report, fixed workflow and deterministic replay. There is no production framework addition. Actual syntax:

```scope
let produced = scope.invoke("forth", "evaluate_candidate", {});
let adapted = scope.invoke("adapter", "church", {evidence: produced});
let checked = scope.invoke("path", "check_source", {source: adapted});
checked
```

These aliases are explicitly supplied by this Host, not claimed as automatically discovered registered provider names. Full public semantic names and actual forwarded arguments are retained. The local discovery method returns `null`: **automatic discovery of this cross-provider compatibility is not demonstrated**.

The normal agent arm exposes `host.ce4.compose` plus individually registered read-only binding wrappers. The wrappers bind dynamic source text, then invoke exactly one semantic operation. Each inner call goes through `AgentWorkflowContext::invoke`; session policy sees the generated Path source **before** its provider is constructed. ReportHost grants and underlying source-aware semantic authorization are additional checks, not replacements for session authority. Workflow possession does not grant consumer authority.

The agent receives a deterministic compact projection containing requested check, actual transferred integer, step references/statuses, consumer outcome, kernel judgment and limitations. Full intermediate envelopes remain host-local. No nested inference or generated prose is needed. The two replay decisions are workflow invocation and completion, not evidence that a model can discover or reason about the workflow.

## 5. Failure and authorization controls

Focused tests cover:

- Real Forth `[4]` → explicit adapter → independently Checked Path theorem.
- Actual multi-item producer stack rejected before Path; direct wrong representation/range rejection and forged-evidence rejection.
- Producer permitted / consumer denied; no consumer source or dispatch. Agent-session denials are retained as failed **predispatch attempts**, not mislabeled skipped calls.
- Real division-by-zero Forth candidate failure, with adapter rejection and Path skipped.
- Real producer `[5]`, valid adaptation, completed Path call, mathematical rejection and unsuccessful workflow.
- Shared local call allowance exhausted before the next step; argument and return byte delivery bounds.
- Explicitly labeled simulated incomplete producer evidence and simulated semantic authorization denial. These test host handling, not claimed naturally observed incomplete provider behavior.
- Actual stale `expected_version` through the public **nonprojected** semantic tool: `revision_conflict`, authorization never reached, native result absent, source unchanged.
- Immutable source bindings, predecessor/evidence links, compact projection, direct/Script equivalence and normal agent compatibility.

Local Host calls count dispatches (including the adapter); denied/pre-budget attempts are retained but not charged as dispatched. The agent additionally charges the outer workflow and inner attempts using the existing global budget. Return-byte enforcement occurs after provider return and preserves full evidence: this is not a memory-retention cap or sandbox. The example performs no Cell capability escalation, writes, proposal application or arbitrary execution grants.

## 6. Evidence and revision guarantees

Each arm retains exact Script where used, grants/limits, source snapshots, actual versions, forwarded semantic arguments, full returned tool envelopes/errors, evaluation classifications and predecessor links. Attempt IDs (`attempt:N`), source references (`source:N`) and indexes are **local to one arm's report**. Fresh scratch owners may allocate the same raw scratch ID; report-qualified source references prevent treating those as a shared global identity.

A step's `transport_status`, delivered `state`, semantic classification and whole-workflow success are separate. A semantic rejection may be transport-completed. Return-budget failure can preserve a completed transport response while failing delivery. Later never-attempted steps are marked skipped. Missing outcomes never imply success.

The Forth hypothetical candidate remains an argument attached to its empty base snapshot. The generated Path document is an independently bound hypothetical scratch, not a saved revision of the original source. No provider version or nonexistent contract version is invented. Numeric Path goal IDs are not used as durable identifiers.

Snapshots are immutable for each call. The stale-precondition regression exercises real revision rejection, **not concurrent mutable-source validation**. Historical reports do not validate edited current sources. Complete retained evidence means the captured tool envelopes and bindings, not universal native diagnostic provenance, durable persistence or a global evidence resolver. Evidence is ordinary report-owned memory until explicitly written by the reproduction command.

## 7. Measured results

Checked-in `examples/ce4-composition/deterministic.json` contains a direct sequential baseline, Script composition and normal agent replay. Direct invokes the **same** two independent providers and adapter without the interpreter. Direct and Script step evidence, sources and actual outputs compare equal. No extra arithmetic provider is required for this baseline.

| Metric | Direct sequential | Script composed | Agent workflow replay |
|---|---:|---:|---:|
| Real semantic operations | 2 | 2 | 2 |
| Adapter dispatches | 1 | 1 | 1 |
| Model decisions | N/A | N/A | 2 prescribed |
| Cumulative model message-content UTF-8 bytes | N/A | N/A | 4,944 |
| Tokens | unavailable | unavailable | unavailable |
| Explicit adapter validations | 1 | 1 | 1 |
| Independent consumer checks accepted | 1 | 1 | 1 |
| Local grant boundaries | 3 | 3 | 3 plus session policy |
| Agent authorization calls | N/A | N/A | 4 including wrapper |
| Argument/return JSON bytes, local accounting | 5,805 | 5,805 | 5,805 |
| Complete semantic envelopes retained | 2/2 | 2/2 | 2/2 |
| Serialized full report bytes | 8,657 | 8,885 | 11,479 |
| Serialized host projection bytes | 1,902 | 1,902 | 1,899 |
| Elapsed microseconds, one recorded run | 2,026 | 4,249 | 2,949 |

Serialized report/projection bytes are **not** model traffic. Agent message-content bytes sum actual messages presented at both decisions, excluding protocol framing; they are not token estimates. Host projection sizes differ from the workflow-specific model projection. Elapsed times and thus exact serialized metrics can vary on reproduction. No independent-agent baseline, speedup, live-model benefit or statistical performance inference is claimed.

## 8. Reproduction and validation

From the Scope root, with sibling `../scope-forth`, `../path`, the pinned Rust toolchain and cached dependencies:

```sh
cargo test --offline --locked --manifest-path examples/ce4-composition/Cargo.toml
cargo run --offline --locked --quiet --manifest-path examples/ce4-composition/Cargo.toml > examples/ce4-composition/deterministic.json
cargo test -p scope-script --all-targets --locked --offline --quiet
cargo test -p scope-ide --test structural_interoperability --locked --offline
cargo test -p scope-ide --features agent --lib --locked --offline --quiet
cargo test --offline --locked --manifest-path examples/ce1-forth/Cargo.toml
cargo test --offline --locked --manifest-path examples/ce2-forth/Cargo.toml
cargo test --offline --locked --manifest-path examples/ce3-path/Cargo.toml
git --no-pager diff --check
```

CE4: **18 tests passed**, and runnable output passes. Script all-target tests pass; SI structural interoperability: 8 passed; CE1: 4 passed; CE2: 7 passed; CE3: 6 passed. The agent library run had **587 passed / 1 failed**: `neutral_proposal_rejects_without_changing_source` encountered temporary Cell workspace mount `NotFound` in test setup. That test passed in isolation with default features; this does not establish that the full-suite transient is fixed. Existing dependency/dead-code warnings remain. No live-model/network experiment was run.

## 9. Concrete downstream requests and limits

No sibling semantic change is required for this bounded integration. For broader downstream use:

1. Providers should publish owner-authored, versioned payload contracts if automatic compatibility discovery is wanted. Current semantic operation descriptions do not supply recursive output contracts or nominal payload identities. Do not promote this host adapter identifier into a provider attestation.
2. Cross should explicitly register any domain adapters and their assumptions. A general Forth-to-Path correctness translation is absent; this eight-bounded Church encoding is not its substitute.
3. Reusable durable evidence handles, retention/resolution policies and live multi-source freshness remain separate missing contracts. Local reports suffice here, not for distributed or changed-source citation.
4. Real incomplete/resource-limited outcomes and broader schema compatibility deserve provider-specific integrations; simulated incompleteness is not such a demonstration.
5. A live small-model trial and independently invoked agent baseline are required before model-efficiency claims.

## 10. Cross readiness assessment

This table concerns only the exercised slice, not a general readiness score.

| Capability | Classification | Evidence / limit |
|---|---|---|
| Discover compatible capabilities | **Supported but untested** | Nominal registry discovery exists; CE4 uses explicit fixed bindings. Automatic Forth/Path payload compatibility discovery is **missing** without published bridge contracts. |
| Compose typed operations | **Demonstrated** | Real transport types plus explicit runtime checked singleton integer and bounded Church adapter; no static semantic assignability claim. |
| Execute under explicit authority | **Demonstrated** | Per-step grants, source-aware semantic policy, normal agent session checks and denial controls. |
| Validate actual results | **Demonstrated** | Adapter consumes actual Forth stack; independent Path accepts four and rejects five against fixed addition. |
| Handle partial failure | **Demonstrated** | Real producer/consumer failures, budget stops, predispatch denial and skipped steps. Naturally occurring incomplete-provider execution remains untested. |
| Inspect provenance | **Demonstrated** | Full report-local arguments, snapshots, attempts/results and predecessor links. Durable cross-run/global resolution remains missing. |
| Expose compact agent results | **Demonstrated** | Normal workflow protocol returns deterministic projection; complete intermediate evidence retained locally. Live-model interpretation is untested. |

**Conclusion:** an independent downstream project can rely on existing explicit host composition, per-boundary authority, typed transport, runtime validation and normal workflow dispatch for a narrow real integration. It cannot infer semantic interoperability from matching JSON, inherit execution authority from evidence, treat local references as durable identities, or generalize this one kernel-checked equality to arbitrary cross-domain correctness.
