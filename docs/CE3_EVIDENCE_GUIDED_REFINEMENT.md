# CE3 — Evidence-guided agent refinement

## Result and experimental status

**The ordinary agent execution path can deliver authoritative Path rejection evidence and independently evaluate a revised candidate. A practical live-model advantage remains unverified.** No new runner, proof search, workflow, provider semantics, automatic source write, or generic feedback taxonomy was needed.

The narrowly scoped implementation is `examples/ce3-path`: a real Path task, a host-local selective result adapter, full local evidence retention, and baseline/compact/feedback-withheld comparison. Production Scope APIs and downstream repositories are unchanged. This is an opt-in experiment, not a silently changed default tool response.

Three deterministic trials per arm prescribe `x`, then `f x`, then completion. They prove pipeline behavior, not feedback-dependent reasoning. All three arms—including the control that cannot see rejection details—use those prescribed choices in replay. No learning or causal correctness advantage follows from these results.

## 1. Architecture audit

Inspected the CE1/CE2 documents and implementation, `scope-language` semantic metadata, `scope-ide` semantic adapter, CLI semantic opt-in, agent loop, edit/investigation lifecycle, tracing, and Script integration. Relevant entry points:

| Contract | Actual implementation and finding |
|---|---|
| Operation metadata | `crates/scope-language/src/semantic.rs`: purpose `CandidateEvaluation` is independent of `Read`/`ProposeChanges`, target requirements, and optional generic evaluation output. Metadata is not authority or proof of purity. |
| Normal decisions | `crates/scope-ide/src/agent_loop.rs`: `run_session`, `run_session_traced`, `DecisionModel`, `AgentLimits`. One visible tool decision at a time; successful results and tool errors become ordinary feedback. Protocol explicitly says feedback is data, not instructions. |
| Semantic dispatch | `crates/scope-ide/src/tool/semantic.rs`: `SemanticToolProvider`, `SemanticAuthorization`, `with_projected_arguments`. Host binds identity, language, version and exact source; schema checks, policy, permitted scope and pre/post freshness remain authoritative. |
| Terminal editing/investigation | `crates/scope-ide/src/cli/agent_semantics.rs` explicitly allowlists Read/Document operations and separately authorizes candidate evaluation. The open document is borrowed throughout. Projected arguments do not grant edits. |
| Edit refinement | `crates/scope-ide/src/edit_lifecycle.rs`: existing evaluation/refinement counters, phase transitions, retained continuation, and `candidate_evaluation_feedback`. The generic branch interprets the optional typed evaluation envelope (`Satisfied`, `Violated`, `Inconclusive`), **not arbitrary Path payload classifications**. Path's current response in this experiment has no top-level generic `evaluation` field. Do not assume Path `Completed` automatically drives this branch. |
| Proposal acceptance | Source editing/controller and proposal lifecycle are separate from candidate evaluation. Evaluation cannot apply, accept, or persist a proposal. |
| Evidence | `InvestigationEvidence { call, source, response }` and `EditRun` retain full successful returned envelopes and source associations. `EditContinuation::refresh` removes stale model-facing evidence when host relevant-state tokens change. Retained historical evidence is not current validation. |
| Structured traces | `EditEvent` provides lifecycle/counter information; `ToolExecutionEvent` is not a full payload ledger. Opt-in `ProtocolTrace` captures requests/responses/feedback; sink failure latches but does not enforce persistence. Trace data may be sensitive. |
| CE1 | Compact overview/detail discovery and existing Script projections reduce context; discovery never expands grants. It did not add agent Script execution. |
| CE2 | `AgentWorkflow`, `AgentWorkflowContext::invoke`, `run_session_with_workflows` register trusted host tools into the same loop. Inner calls re-enter call-aware authorization, require Read, and share attempt/byte budgets. Generic runner has no Script/Forth dependency. CE3 does not need this compound-tool hook. |

CE2's five-versus-two prescribed decisions, four-versus-one outer dispatches and 85,483-versus-24,113 message-content bytes remain replay findings. Four semantic operations ran in both arms. They do not establish model reasoning or token savings. Script completion is not semantic correctness; successful envelope retention is not a universal failed-attempt ledger; CE2's dependency selection remains fixture-specific.

### Distinct success layers

1. JSON decision parses: protocol success, not provider execution.
2. Tool/schema/source checks and host authorization pass: permission, not correctness.
3. Provider returns a read-only envelope: execution returned, not candidate acceptance.
4. Path classification and underlying checked outcome establish candidate status.
5. Agent `complete` stops the loop: self-reported task completion, not proof.
6. A separately authorized proposal may be accepted/applied/persisted: **never performed here**.

## 2. Actual downstream evaluator

Use sibling `../path`, not `../pathlang`. Pathlang exposes different operations and is not a substitute for this candidate contract. The current Path working tree contains downstream work; local path dependencies are checkout-dependent, not pinned by this experiment's lockfile.

Verified public API: `../path/crates/path-syntax/src/candidate.rs`, re-exported by `path-syntax/src/lib.rs`:

```rust
pub fn evaluate_goal_candidate<'a>(
    source: &'a str, goal_id: usize, replacement: &'a str,
) -> CandidateEvaluation<'a>
```

It freshly checks the original source, requires an incomplete original, selects the current goal, verifies its UTF-8 span is `?`, substitutes verbatim, and checks the **entire hypothetical document**. It does not install candidate definitions. Current code supports document-local definitions; historical claims that no definitions exist are not a reliable description of this working tree.

| Actual classification | Authoritative meaning |
|---|---|
| `Completed` | Candidate outcome `Checked`, with closed kernel evidence. |
| `StillIncomplete` | Candidate `Incomplete`, with remaining obligations. Not complete acceptance. |
| `RejectedCandidate` | Mathematical rejection **or** ordinary frontend failure. Inspect outcome/diagnostics; do not call every rejection mathematical. |
| `Inconclusive` | Unsupported elaboration, resource exhaustion or internal failure. Neither acceptance nor rejection established. |
| `InvalidRequest` | No candidate outcome: original not incomplete, unknown goal, invalid hole span, or candidate-size/allocation request failure. |

Request reasons are `OriginalNotIncomplete`, `UnknownGoalId`, `InvalidHoleSpan`, `CandidateTooLarge`. An original resource failure can yield `InvalidRequest`; a candidate resource failure can yield `Inconclusive`. Invalid replacement syntax need not be an invalid tool request.

The public Scope adapter is `../path/crates/path-scope/src/candidate.rs`, registered through `path-scope/src/lib.rs`. Tool name: `language.language.path.evaluate_goal_candidate`; arguments: integer `goal_id`, string `replacement`. Metadata declares Read/Document/CandidateEvaluation. It returns `read_only` for completed, rejected, incomplete and invalid-goal evaluation results. Projection failures can return `blocked`; the adapter bounds serialized reports to 65,536 bytes/depth 64. These are not guaranteed native allocation bounds.

Actual payload retains original/candidate source, outcome, selected original goal, candidate diagnostics/stages, classification, invalid-request reason, rendered `feedback`, structured `explanation`, and `application_authorized: false`. Diagnostics include provider-established stage/category/severity/message/span and error detail. Original stages are unavailable through this API and projected as empty. Diagnostic coordinates refer to the **candidate**; selected-goal coordinates refer to the **original**. Missing remaining-goal information on failure is not zero obligations.

`propose_goal_candidate` and exact-candidate validation independently evaluate again (`path-scope/src/proposal.rs`); completion is not a reuse or application token. Path candidate/host tests cover stale host preconditions, closed evidence recheck, and goal renumbering.

## 3. Smallest justified CE3 implementation

`examples/ce3-path/src/main.rs` wraps the existing semantic provider with a local `ToolProvider`. All arms use ordinary `run_session`, the same catalog, immutable snapshot, host policy, task, backend configuration and budgets. No nested inference occurs.

- **Baseline:** deliver the original complete provider envelope.
- **Compact:** copy actual status/operation/rationale and provider classification, replacement, goal, invalid-request reason, feedback, explanation, application authorization, plus original/candidate input/outcome/diagnostics/goals/kernel/limitations. Omit duplicate source strings, stage records and redundant report detail. Non-read-only or malformed/partial envelopes fall back unchanged.
- **Withheld control:** explicitly mark feedback withheld; do not imply rejection or success. Keep host-local raw evidence, but do not show it to the control model. Tool errors still remain errors. This control withholds whole returned envelopes, not only rejection prose; it is an intentionally coarse ablation, not a perfectly isolated diagnostic-only intervention.

No diagnostic categories or explanations are generated by Scope. The original authoritative envelope is retained unchanged even when its model-facing view is selective. Exact source identity/text/revision and goal are supplied in the common task and retained separately. Full call arguments bind each result to its candidate. There is no durable citation resolver, evidence database, or cache. Report-local array positions can reconstruct evidence only within their enclosing report; no model-facing evidence reference API is introduced.

The model already receives all raw Path diagnostics in baseline; CE3 improves compactness, not previously absent evaluator knowledge. Only omitted raw detail remains exclusively host-local in the compact arm. Future unknown fields are not automatically projected: this adapter is fixture/provider-specific and opt-in, not a general lossless projection guarantee.

Every attempted call uses the normal shared tool budget. Limits per run: 10 decisions, 8 calls, 200,000 bytes/message, 2,000,000 conversation bytes, 2,000 requested tokens/decision. Provider returns precede result byte checks; no cancellation or allocator cap is claimed. No remaining-budget packet was added because ordinary runner counters suffice for this experiment.

## 4. Evidence-aware repetition and revisions

Existing suppression is in the **edit/investigation lifecycle**, not generic `run_session` or every `AgentSession` invocation. `edit_lifecycle.rs` compares exact `ToolCall` equality and host `relevant_state` tokens before redispatch. BTreeMap argument representation avoids key-order artifacts. Default duplicate tolerance ends in NoProgress; bounded recovery can reference retained continuation evidence. This is a no-progress policy, not a claim that every Read provider is pure. Failed identical operations can also be suppressed.

Existing tests demonstrate `identical_failed_candidate_is_suppressed`, `reordered_nested_candidate_objects_are_the_same_call`, `candidate_source_expectations_and_host_state_each_restore_dispatch_eligibility`, and investigation retained-evidence recovery. Different arguments or host relevant state restore eligibility; continuation refresh qualifies/removes stale context. Low-level hosts supplying a null/insufficient relevant-state token cannot infer changing evaluator configuration automatically.

Path's evaluator currently freshly checks supplied source and candidate without host mutations, filesystem/network or installed proof state. Nevertheless CE3 adds **no caching or suppression extension**. Ordinary harness repeats deliberately dispatch twice; the focused test records two real evaluations and one exact repeat. This documents the runner distinction instead of promising suppression where none exists.

Changed candidate text is counted as a candidate-text revision, **not semantic equivalence or demonstrated meaningful reasoning**. Goal ID, source revision and evaluation conditions must remain part of any future reuse key. Path's numeric goal IDs can renumber after edits and are not durable identifiers. The immutable borrow and host binding establish this experiment's consistent revision; real concurrent revision changes are not experimentally simulated here. Existing semantic-host freshness and lifecycle state-change tests remain the coverage for that boundary. No evidence is reused across revisions.

## 5. Real task and protocol

Real source fixture: `examples/ce3-path/application.path`:

```text
theorem application (A : Type 0) (B : Type 0) (f : Pi (a : A). B) (x : A) : B := ?
```

The fixture is loaded into a real Scope scratch source. `x` is a plausible in-scope term, but has type A where B is required. Path rejects it with mathematical/type-mismatch evidence. `f x` is independently kernel-checked as `Completed`. `?` remains incomplete; unknown goal 999 is an invalid evaluator request. No checker/kernel changes or weakened criteria are involved.

The common model task asks for its own kernel-checked replacement, using the source and registered evaluator, and never gives `f x` as a hint. The replay backend alone prescribes the sequence for contract testing. Live mode prescribes **no candidates and no seeded failure**; therefore live runs may solve on the first try or never encounter rejection. Such runs must not be advertised as feedback-dependent refinement. Only the evaluator is installed; the source provides the goal/context, and arbitrary additional inspection tools are outside this deliberately small experiment.

Every trial retains source binding, catalog, task, budgets, model request messages and outputs/errors, full provider envelopes/errors, delivered projections, classifications, repetition/change counts and elapsed times. Model errors preserve partial evidence and are serialized; setup failures are retained as failed trials rather than aborting the comparison. Source text/version/dirty state and absence of proposals are checked afterward. The disk fixture is only read, with no filesystem write operation available to the agent.

## 6. Results

Checked-in `examples/ce3-path/deterministic.json` contains three trials per arm, rotating baseline/compact/control order. Per run:

| Metric | Baseline | Compact CE3 | Withheld |
|---|---:|---:|---:|
| Prescribed model invocations, including completion | 3 | 3 | 3 |
| Cumulative model message-content UTF-8 bytes | 41,303 | 26,867 | 9,716 |
| Actual token counts | unavailable | unavailable | unavailable |
| Charged tool calls / provider evaluations | 2 / 2 | 2 / 2 | 2 / 2 |
| Exact repeated argument sets | 0 | 0 | 0 |
| Candidate-text changes | 1 | 1 | 1 |
| Completed / rejected candidates | 1 / 1 | 1 / 1 | 1 / 1 |
| Inconclusive / incomplete evaluations | 0 / 0 | 0 / 0 | 0 / 0 |
| Full returned evaluation envelopes retained | 2/2 | 2/2 | 2/2 |
| Source edits applied or persisted | 0 | 0 | 0 |

All nine runs preserve source and revision. Compact reduces cumulative message-content bytes by **34.95%**; it does not reduce decisions or evaluations. Control is smaller because it removes substantive evidence, not because it is equivalent. Bytes exclude roles/framing and are not tokens or cost. Timings are retained per trial and per evaluation in the report; this small replay measures local overhead, not a causal latency advantage or model performance. Candidate acceptance is separate from model stop/completion and from persistence.

### Live model status

Historical Path launcher configuration names `qwen3-30b-a3b` at `http://127.0.0.1:1234/v1`. A bounded read-only `/v1/models` probe from this environment failed to connect. No working backend or loaded model was established; sandbox/network access can also limit visibility. **No live inference was run.** No model-dependent benefit, token economy, successful autonomous recovery, or diagnostic causality is verified.

Live harness uses the same explicit model/backend in all three arms, fixed task, same budgets, three rotated trials, no trial-specific prompt tuning, and retains failures. It sends public fixture/source/catalog/results to the configured endpoint; review privacy/cost implications. Reports contain raw model content; review before sharing. `OPENAI_API_KEY`, if present, is used but not written as a report field. Backend token telemetry is not exposed and remains null.

## 7. Reproduction and validation

From Scope root, with sibling Path and cached dependencies:

```sh
cargo test -p scope-ide --features agent --lib --offline --quiet
cargo test -p scope-script --all-targets --locked --offline --quiet
cargo test --manifest-path examples/ce3-path/Cargo.toml --locked --offline
cargo run --manifest-path examples/ce3-path/Cargo.toml --locked --offline --quiet > examples/ce3-path/deterministic.json
# Requires an accessible OpenAI-compatible server and a loaded model:
cargo run --manifest-path examples/ce3-path/Cargo.toml --locked --offline --quiet -- --live qwen3-30b-a3b http://127.0.0.1:1234/v1 > examples/ce3-path/live.json
git --no-pager diff --check
```

Validation performed: **588 scope-ide agent library tests passed**, existing Script all-target suites passed, and **6 CE3 tests passed**. Standalone build and report generation passed. Existing `set_agent_config` dead-code warning remains unrelated. The downstream audit additionally compiled and ran nine real Path candidate integration tests with temporary outputs; all passed.

CE3 tests cover real Completed/RejectedCandidate/StillIncomplete/InvalidRequest, preserved diagnostics/identity fields and kernel checked completion, paired catalogs, raw/control delivery, duplicate redispatch accounting, invalid/unknown arguments, authorization, shared tool-budget stop, partial evidence, model failure retention, and projection passthrough on failure. Inconclusive projection preservation is **synthetic**, explicitly not a real resource-failure experiment. Existing core tests cover revision/freshness, lifecycle exact suppression/revised eligibility, policies, and Script compatibility. No universal audit completeness or all-provider failure coverage is claimed.

## 8. Limitations and concrete downstream requests

No missing Path rejection diagnostic blocks this task. Do not request a new taxonomy or modify the kernel to improve the experiment.

Narrow follow-ups if this adapter is promoted:

1. Path: document/version the public projection shape and coordinate provenance explicitly, including original versus hypothetical candidate revision. The same supplied document version on hypothetical candidate input is **not a committed new revision**.
2. Path/Scope integration: if generic edit-lifecycle assessment is desired, explicitly map Path classifications/outcomes into the existing typed evaluation contract, retaining original classifications and partial/inconclusive distinctions. Current language-specific payload alone is not that contract; no such mapping is guessed here.
3. Expose actual backend usage telemetry if token comparisons are required; message bytes cannot substitute.
4. Establish an accessible intended local model and run the paired live/control protocol. Independently grade feedback-dependent changed decisions, not merely final completion strings or repeated attempts.

## 9. CE4 recommendations grounded in CE3

- **Typed outputs:** ToolValue/JSON and operation metadata do not establish cross-provider semantic compatibility. Preserve schema/version and distinguish Path payload status from generic evaluation outcomes; do not wire by prose field names.
- **Provenance and execution identity:** include provider identity/version, operation/arguments, exact source binding, original/hypothetical coordinate roles, and invocation identity. Current numeric goal IDs and local evidence positions are insufficient as cross-provider handles.
- **Partial failure:** transport failure, invalid request, frontend rejection, mathematical rejection, incomplete and inconclusive must survive composition distinctly. A full returned-result list is not a complete attempted-operation ledger.
- **Authorization propagation:** downstream consumption/evaluation is a new authorized operation; possession of another provider's evidence grants no write or evaluation authority. Preserve shared attempt/byte budgets.
- **Independent verification:** Path kernel evidence can be rechecked by Path; generic JSON or completion prose is not itself an independent proof. Another provider needs an explicit trusted verifier/translation contract before interpreting acceptance.
- **Revision validity:** historical evidence must remain historical after source/configuration changes. Provider-neutral identity must include evaluation preconditions, not only a human path or integer revision.

Do not solve these composition issues by adding a database or orchestration framework during CE3. The next empirical step is a controlled live run; the next architectural step is explicit interoperable evidence contracts, not automatic proof repair.
