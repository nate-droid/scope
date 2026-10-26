# CE2 — Agent-native bounded semantic investigation

## Result

A real Scope Forth investigation now runs as one host-defined Script tool through the normal Scope decision protocol. It performs four registered reads, returns compact observations with report-local evidence references, and retains the underlying responses. The generic runner contains no Forth knowledge and has no Script dependency.

**Feasibility demonstrated; practical advantage for a small local model remains unverified.** Three paired deterministic replays reduce prescribed decisions from five to two and cumulative model message-content bytes from 85,483 to 24,113. No live model was run, no token savings were measured, and answer prose was not independently graded. Replay cannot establish that an LLM discovers, selects, or interprets the workflow correctly.

## 1. CE1 reconciliation

Inspected `docs/CE1_CONTEXT_ECONOMY.md`, the actual `examples/ce1-forth` harness and Script composition, `scope-script` Host/Interpreter/runner, `scope-ide` semantic adapter, session dispatcher, agent loop and evidence contracts. CE1 did implement compact catalog/detail discovery and a separate host-driven Script composition. It did **not** expose Script in the agent execution path. Its measured 45.76% reduction concerned cumulative message-content UTF-8 bytes, not tokens, round-trips or local-model correctness.

| Question | Before CE2 |
|---|---|
| Can an agent invoke Script through normal execution? | Not through the supplied normal investigation interface. Hosts could compose Script separately. |
| Can Script call several registered semantic operations? | Yes: `SemanticHost`, `ScriptRunner::with_operations`, existing multi-semantic tests and the real CE1 fixture do so. |
| Can results be projected without returning all intermediate values? | Yes: existing object/field expressions and control flow suffice. No interpreter change is needed. |
| Are individual results retained? | `RunRecord.results` retains full successful envelopes independently of the final value. Failed attempts are not a complete audit ledger. Investigation lifecycle `InvestigationEvidence` separately owns call/source/response. |
| Are revisions and nested budgets enforced end-to-end? | Semantic adapters enforce host-bound source preconditions and freshness. Script's invocation-local call limit was **not** shared with agent tool limits or argument-sensitive outer authorization. Snapshot Script execution does not promise freshness of a separate mutable live source. |

Existing `ProtocolTrace` is opt-in and sensitive; writer failure does not enforce required persistence. `ToolExecutionEvent` carries execution metadata, not complete arguments/results. Neither is a durable citation resolver.

## 2. Choice of mechanism

| Concern | A: agent-authored Script | B: fixed host-authored Script (chosen) |
|---|---|---|
| Architecture | Existing interpreter, but source validation/exposure needed | Existing interpreter and normal decision loop |
| Authorization | Must intersect policy at every inner call | Same requirement, with reviewed fixed call set |
| Revision | Requires host-owned bindings, never model tokens | Existing projected semantic adapter binds immutable target |
| Expressiveness | More adaptable; greater runtime/error surface | Sufficient for this finite four-read slice |
| Output | Arbitrary projection could omit critical uncertainty | Reviewed projection and explicit partial status |
| Complexity | Requires a public arbitrary execution contract | Small opt-in restricted dispatch hook |
| Audit | Must retain supplied programs and all outcomes | Fixed Script retained in report; local call/result indexes |

Only B is implemented. There is no new interpreter, language, plugin registry, agent loop, authorization system, automatic wiring or modification capability. The public hook is a narrow trusted compound-tool adapter, not a scheduling or workflow framework. It does not make arbitrary native adapters safe.

### Production change

`crates/scope-ide/src/agent_loop.rs` exports `AgentWorkflow`, `AgentWorkflowContext::invoke`, and `run_session_with_workflows`; `agent.rs` reexports them. The opt-in entry point calls the **same** `run_session_inner` as existing APIs. Workflow definitions enter the normal initial catalog; duplicate names are rejected. Models use the unchanged tool/complete protocol, with no hidden prompt rewriting.

Each outer workflow attempt and each inner attempted call consumes `AgentLimits.max_tool_calls`. Every inner call re-enters `AgentSession`'s actual call-aware host policy and additionally requires `ToolEffect::Read`. The context exposes neither raw session nor App. Other workflows are not inner dispatch targets. Repeated workflow invocations cannot reset run budgets; budget failure latches even if an adapter ignores the error.

Inner argument/result bytes are charged cumulatively against the run conversation allowance and bounded individually by its message allowance. These checks occur after provider return for results: **not allocator limits, hard timeouts or guaranteed bounded native-provider retention**. Trusted synchronous providers/adapters must terminate themselves. An oversized provider result may already have been allocated and retained before rejection. No sandbox or cancellation claim is made.

## 3. Real downstream investigation

`examples/ce2-forth` is a standalone workspace using the existing sibling `scope-forth` provider; no sibling files or Cell files were changed. External dependencies are locked, but local path dependencies remain checkout-dependent.

```forth
: square dup * ;
: fourth square square ;
: misleading-fourth square dup ;
```

Task wording is identical in both arms (see `TASK` in the harness): list local words and investigate `fourth`, related `square`, and `misleading-fourth` with one symbolic input each; report observations, not arithmetic proof.

The actual operations are `language.example.forth.words(document_only: true)` followed by three `describe(name, symbolic_inputs: 1)` calls. The fixed Script uses existing object projections. Expected conditional final stack **node indexes**, not numeric values, are `fourth: [2]`, `square: [1]`, and `misleading-fourth: [1,1]`. The latter leaves two stack items rather than one. Provider obligations and limitations remain in full evidence; this is not a universal arithmetic correctness, safety, termination or equivalence proof.

**Dependency gap:** this experiment exposes no public dependency-graph operation. The host explicitly selects `square` from the visible fixture, rather than implementing a Forth parser or claiming automatic relationship discovery. The workflow is intentionally specialized to this fixture. Its typed parameter contract is the empty argument object: extra arguments, including attempted revision overrides, are rejected. Arbitrary word investigation and inferred dependency traversal are not implemented.

## 4. Findings versus evidence

The model receives the real snapshot identity, language, revision and text, investigated objects, inventory and symbolic stack/status projections, local evidence indexes, partial/error status, and explicit coverage limitations. The fixed source is small and public; no implicit filesystem reads occur. The immutable target borrow plus semantic source checks prevents silent revision changes.

The report retains:

- Exact Script, task, base catalog, grants and limits.
- Host source binding and unchanged-source checks.
- Full projected agent calls, complete returned provider envelopes and errors.
- Inner pre-dispatch failures and context rejection where encountered.
- Each workflow's final projection, including partial runs.
- Decision-request messages, model responses, counts and elapsed time.

This uses invocation-local host capture, as in CE1, **not a new evidence store**. References index only the enclosing run's `evidence` array; there is no global or durable resolver. Projected arguments omit adapter-injected preconditions: the separate binding and provider source metadata establish their provenance. The harness does not claim a complete low-level dispatch event ledger or per-operation budget/timing trace. Decision-request capture can omit terminal feedback when no next model request occurs; baseline pre-dispatch failures are visible in subsequent requests only when one occurs. Workflow inner failures are explicitly captured by the forwarding Host. A budget stop may retain a projection without delivering it to the model. Interpreter success is not semantic success.

The host assesses inventory and all three revision-bound conditional stack observations independently of completion prose. Thus “correct findings” below means expected observation coverage, not verified natural-language reasoning. Evidence remains public fixture data; live transcripts may be sensitive and must be reviewed before sharing. No automatic acceptance, write authority, secret capture, or additional inference occurs inside the workflow.

## 5. Controlled comparison

The baseline uses ordinary `run_session`; CE2 uses its opt-in workflow variant. Both retain the same direct semantic operations, source/revision, task wording, base catalog, semantic grants and host policy. CE2 adds the workflow definition and permission to call that read-only wrapper, not extra semantic authority. Each arm uses ten decisions, eight total charged calls, 100,000 bytes per message, 1,000,000 aggregate bytes and 2,000 requested maximum tokens per decision. CE2 additionally charges inner traffic, so the allowance is conservative rather than an accounting bypass.

Three fixed pairs run in baseline/workflow, workflow/baseline, baseline/workflow order, without prompt tuning. Each starts fresh state. Default backend is the same deterministic `DecisionModel` replay, **not an LLM**. Individual baseline calls and the single workflow invocation are deliberately prescribed. Direct calls remain available in CE2; a live model is not forced to select the workflow.

Checked-in `examples/ce2-forth/deterministic.json`:

| Metric (per trial unless noted) | Baseline | CE2 |
|---|---:|---:|
| Decision-model invocations (includes completion) | 5 | 2 |
| Model tool dispatches | 4 | 1 |
| Total charged tool attempts (includes inner reads) | 4 | 5 |
| Model-facing tokens | unavailable | unavailable |
| Cumulative model message-content UTF-8 bytes | 85,483 | 24,113 |
| Semantic operations executed | 4 | 4 |
| Duplicate inspections | 0 | 0 |
| Elapsed microseconds, three recorded trials | 4,682 / 3,345 / 3,357 | 5,010 / 4,822 / 4,677 |
| Expected observation coverage | 4/4 | 4/4 |
| Full successful semantic envelopes retained | 4/4 | 4/4 |
| Agent completion / unchanged source | 3/3 | 3/3 |

Fewer prescribed decisions and ~71.8% fewer message-content bytes are measured replay properties. Bytes exclude roles/framing and are not tokens. The local interpreter adds work: these timings do not show latency improvement. Repetitions establish replay consistency, not statistical inference performance.

No authorized local model/backend identifier was supplied or established, so live inference was not attempted. The optional `--live MODEL BASE_URL` path uses the same `CellAgent` configuration in both arms and the same three-pair order. It sends source/catalog/results to that endpoint and may incur costs; optional `OPENAI_API_KEY` comes from the environment, not report fields. Actual token usage is not exposed by this harness and remains null even in live mode. Live conclusions require separate answer-quality assessment, backend/token telemetry, more trials and stronger counterbalancing. A successful process alone proves none of those.

## 6. Reproduction and validation

From the Scope root, with sibling `../scope-forth` and cached dependencies:

```sh
cargo test -p scope-ide --features agent --lib --offline --quiet
cargo test -p scope-script --all-targets --locked --offline --quiet
cargo test --offline --locked --manifest-path examples/ce2-forth/Cargo.toml
cargo run --offline --locked --quiet --manifest-path examples/ce2-forth/Cargo.toml > examples/ce2-forth/deterministic.json
git --no-pager diff --check
```

Optional live comparison (not run):

```sh
cargo run --offline --locked --quiet --manifest-path examples/ce2-forth/Cargo.toml -- --live MODEL BASE_URL > examples/ce2-forth/live.json
```

Validation performed: agent library **588 tests passed**, existing Script all-target suites passed, downstream CE2 **7 tests passed**. New generic tests cover discovery, duplicate registration, shared/repeated budgets, read-only and argument-sensitive authorization, unknown/recursive attempts, byte bounds and latched failures. Downstream tests cover empty typed arguments, source override rejection, partial failure, projection/reference correctness, mixed/direct/repeated calls, equal evidence and immutable revision binding. Existing runner and Script compatibility remains tested. The pre-existing `set_agent_config` dead-code warning remains unrelated.

## 7. Concrete follow-up requests / limits

1. Scope Forth: if general investigation is desired, expose a bounded read-only word-reference/dependency operation with explicit unresolved/builtin references and source attribution. Do not infer one from prose metadata. No provider change is required for this fixed slice.
2. Run the optional comparison on the intended small local model, independently grade its answers, and record actual token usage before claiming practical advantage.
3. A future mutable-source workflow needs tested shared source binding and freshness between all reads; this immutable experiment is not a live-edit concurrency demonstration.
4. General-purpose evidence persistence, allocator bounds and cancellation remain outside this change. Do not promote report-local indexes or successful result retention into claims of a complete provenance service.

**Conclusion:** programmatic semantic investigation is now feasible through Scope's normal agent protocol with inherited read authorization and budgets. Whether it gives a small local model a meaningful practical advantage remains an open empirical question, with a working comparison harness rather than an assumed result.
