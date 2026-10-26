# CE1 — Context economy and semantic projection

## Result and scope

CE1 adds **one opt-in, read-only investigation entry point** with two-level discovery, derived from the existing session catalog. It does not add a registry, scripting language, agent runner, semantic index, parser, evidence store, or write capability.

A real Scope Forth comparison demonstrates a **45.76% reduction in cumulative model-facing message-content UTF-8 bytes**, with identical semantic evidence and unchanged authorization. It does **not** demonstrate fewer model interactions or better local-model task completion: both deterministic arms use four decisions and three semantic calls. Live comparison is **unverified**. Smaller outputs alone are not a demonstrated model-performance improvement.

A separate real Forth demonstration uses the **existing** Scope Script runner to perform three reads and project their results in one invocation. This is host-driven composition, not a newly exposed agent tool or an equivalent agent benchmark.

## 1. Architecture audit

### Canonical contracts

| Concern | Actual implementation / public API | Finding |
|---|---|---|
| Semantic registry | `crates/scope-language/src/semantic.rs`: `SemanticOperationDescription`; `LanguageService::semantic_operations` | Language-local stable identity, description, argument kinds/requiredness, effect, target and opaque constraints. No new registry needed. |
| Effects and evaluation | `SemanticOperationEffect::{Read, ProposeChanges}`, `SemanticOperationPurpose::{Exploration, CandidateEvaluation}`, `SemanticTargetRequirement::{Document, Selection}` | Purpose is independent of effect and evaluation output support. A read can run disposable candidate computation. `Read` is not a native-code sandbox. |
| Installed tool discovery | `crates/scope-ide/src/tool.rs`: `ToolProvider`, `ToolCatalogEntry`, `SessionCatalog::{lookup, to_value}`, `AgentSession::catalog` | Catalog is an owned snapshot of installed providers, including full metadata/output descriptions; discovery never grants authority. |
| Semantic adapter | `crates/scope-ide/src/tool/semantic.rs`: `SemanticToolProvider::{new, with_projected_arguments}`, `SemanticAuthorization` | Typed dispatch, permitted scope, source policy, pre/post freshness. Projected **arguments** already keep source/version host-owned; this is not semantic-output summarization. |
| Generic decisions | `crates/scope-ide/src/agent_loop.rs`: `DecisionModel`, `run_session`, `run_session_traced`, `AgentLimits`, `read_only_policy` | One JSON tool or completion decision per model invocation. Counts attempts, bounds message/conversation bytes, no hard synchronous-call cancellation. |
| Investigations | `crates/scope-ide/src/edit_lifecycle.rs`: `run_semantic_investigation_with_discovery`, `SemanticInvestigationOptions`, `SemanticDiscoveryOptions` | Reuses edit lifecycle in read-only Inspect mode with no proposal capabilities; contextual paged discovery, host assessment, source snapshots, retained evidence. |
| Script | `crates/scope-script/src/lib.rs`: `Interpreter`, `Host`; `src/semantic.rs`: `SemanticHost`; `src/runner.rs`: `ScriptRunner::{with_operations, invoke_script_with_target}` | Existing structured values, field projections, conditionals and several host calls in one evaluation. No interpreter extension needed. This is not a general Python-like programming environment. |
| Registry Script adapter | `crates/scope-script/src/host.rs`: `RegistryHost`; `scope-ide/src/tool/operations.rs`: `OperationRegistry`, `OperationLimits` | Alternative document-session path with opaque revision checks, typed operation schemas and explicit policy; not a reason to build another registry. |
| Source and revisions | `crates/scope-ide/src/source_edit.rs`: `SourceTarget`, `SourceSnapshot`, `DocumentSource`, `PermittedEditScope` | Target identity, language, version and exact text are host inputs. Language paths do not grant filesystem authority. Semantic dispatch checks freshness before and after execution. |
| Evidence | `InvestigationEvidence { call, source, response }`, `EditRun::{source_snapshot, evidence, discovered_operations, conclusion}` | Owned full arguments, source tokens and returned envelopes survive later source edits. Host assesses observations, not just model prose. |
| Script evidence | `runner.rs`: `Snapshot`, `RunRecord::{catalog, results, printed, outcome}` | Full returned envelopes are retained independently of final projection. Interpreter success is not semantic success. Failed calls may appear only in the error chain, not as a complete per-attempt audit record. |
| Trace | `crates/scope-ide/src/agent_trace.rs`: `ProtocolTrace`; lifecycle `EditEvent` | Explicit host-selected capture, potentially sensitive. Protocol writer failures latch `failed` but do not change execution outcome; it is not by itself a required-evidence persistence guarantee. |
| Repetition | `edit_lifecycle.rs`, `docs/AI3_REPEATED_CALL_RECOVERY.md` | Exact repeated operations with unchanged relevant-state tokens are suppressed before provider redispatch. Default is NoProgress; opt-in bounded recovery uses existing budgets. No speculative semantic equivalence cache. |

The old internal function named `compact_catalog` already removes some transport redundancy, but still includes argument descriptions, invocation schemas, preconditions and explanatory strings on each phase. Existing contextual discovery reveals full descriptions by page, rather than an overview plus explicit canonical detail.

### Answers to the audit questions

1. **How much is possible in Script?** Most of the deterministic composition: selected reads, branching on returned fields, explicitly assembling requests, and returning only chosen fields. Providers keep domain semantics. Existing `multi_semantic` examples/tests already exercise several registered operations. CE1 now demonstrates this against the real Forth provider too.
2. **Can existing descriptions support compact discovery?** Yes. `SessionCatalog` already joins actual installed tool definitions with language metadata. Level A and Level B can be derived without a parallel registry.
3. **Can several authorized reads happen in one Script invocation?** Yes, through `SemanticHost` / `ScriptRunner::with_operations`. Exposure and per-binding permission are independent; unknown, hidden or denied operations do not gain authority through Script.
4. **Intermediate representation/retention?** Script structured values and full semantic envelopes; `RunRecord.results` retains returned values before projection. Investigation evidence separately owns calls, source state and responses. Neither is a new durable global evidence-reference service.
5. **What prevents efficient agent use?** The model protocol makes one dispatch per decision; the normal investigation runner does not offer Script execution. Full catalogs are repeatedly included in phase context. Continuation evidence is also rendered repeatedly, sometimes with substantial provider output. Script composition requires explicit downstream knowledge of result shapes. Existing duplicate detection prevents redispatch but does not eliminate the model invocation that proposed the duplicate. There is no safe inferred argument wiring from prose descriptions alone.

### Budget and authority boundary: why no arbitrary agent Script tool

The interpreter has finite source/depth, expression-step and value-work/output limits. The semantic host has a 64-read invocation-local limit; `OperationRegistry` also has per-call input/output limits. These are not a shared transitive budget with `AgentLimits.max_tool_calls` or investigation inspection/evaluation counters. Wrapping Script as one tool would hide several semantic operations behind one outer call and reset local limits on another invocation.

Furthermore, `SemanticHost` binding permission is per-action, whereas an outer host may enforce argument-sensitive policy. An adapter must intersect that policy on **every** inner invocation, not assume permission to run Script authorizes its contents. Native providers are synchronous trusted code; result-size checks cannot prevent provider allocations or cancel execution.

`ScriptRunner.records` has no aggregate history retention cap. Its document entry point works on an invocation-local clone: its evidence is about that snapshot, not continuing freshness of the original live document. Script's snapshot identity is not a substitute for the host's binding identity: the Forth demonstration retains separate scratch IDs because both existing record snapshots report `SourceId(0)`.

**Decision:** do not expose arbitrary agent-authored Script in CE1. Doing so safely requires shared attempt/byte accounting, transitive call-aware authorization and attributable failure evidence. The narrower host-selected read composition is demonstrated using existing APIs, without claiming that it closes those gaps.

## 2. Implemented two-level discovery

New public API, exported from `scope_ide::agent` with the existing `agent` feature:

```rust,ignore
run_semantic_investigation_with_compact_discovery(
    model, controller, target, service, request, options, discovery,
    semantic_authorization_policy, completion_policy,
)
```

Arguments match `run_semantic_investigation_with_discovery`. Existing runners and `SemanticDiscoveryOptions { initial_operations, page_size }` struct literals remain compatible. The change is confined to `crates/scope-ide/src/edit_lifecycle.rs`, its exports in `agent.rs`, and tests in `investigation_tests.rs`.

### Level A

For visible, phase-eligible operations, the model receives sorted entries with:

- Fully qualified `name` and actual tool `effect`.
- Canonical `target`, `purpose`, `produces_evaluation` metadata (explicit null if unavailable).
- Description capped at 160 Unicode characters, plus ellipsis and `description_truncated`.

No full argument schema or opaque constraints are included. Null/omission is **unknown**, never an authority grant. The catalog remains bounded by the existing message/conversation limits; the entire catalog is rejected on budget overflow rather than silently cut. Description truncation is explicitly marked. Canonical identity strings are not shortened.

### Level B

The existing lifecycle discovery action supports two forms **only in the opt-in runner**:

```json
{"type":"tool","name":"edit.discover_operations","arguments":{}}
```

Reveals the next sorted page of eligible Level A entries, preserving existing page behavior.

```json
{"type":"tool","name":"edit.discover_operations","arguments":{"name":"language.example.forth.describe"}}
```

Returns the complete existing canonical `SessionCatalog` entry: definition, provider, output description and metadata, including invocation schema, target, effect, purpose and opaque constraints where supplied. This preserves the existing description, not a claim that arbitrary constraints become generic validators. Dynamic authorization remains the host policy, not discoverable permission.

Unknown or phase-ineligible names return `unknown_or_ineligible_operation`. Invalid argument shapes retain existing bounded failure handling. Detail requests consume normal decision and transport tool-call budgets, do not invoke a provider, and are not semantic evidence. Detail for an eligible but hidden operation **does not make it invocable**; the model must still reveal the relevant page. This separation is tested.

For a small task-relevant catalog, hosts can populate `initial_operations` as before. This avoids compulsory discovery round-trips. Compact discovery is opt-in: an agent unfamiliar with the argument shapes may need extra detail calls and do worse. Do not make it the default based on byte reduction alone.

## 3. Semantic overview: reuse domain operations

No reusable projection type was justified by the chosen slice.

Actual Forth operations are `language.example.forth.words` and `language.example.forth.describe`, not hypothetical `enumerate_words` / `inspect_word`. `words` accepts `document_only` and `prefix` and already gives a compact vocabulary with stack metadata. `describe` accepts `name`, `symbolic_inputs` and optional interpretation, returning definition/source attribution and bounded symbolic observations. The demonstration requests document-local vocabulary and symbolic descriptions for two words; no Scope parser or invented semantic relationships are involved.

Other existing downstream contracts inspected:

- Path exposes `check_source` and `evaluate_goal_candidate`; disposable evaluation results must distinguish kernel success, incomplete/unsupported elaboration and frontend rejection. Existing `path-dx5-live` binds a retained scratch and two reads.
- Hilb exposes circuit/static and simulation evidence and already adopts host compositions through the normal shell (`:scope hilb check`, `run`, `full`). Numerical/source/execution projections remain downstream knowledge; missing statevectors or unsupported boundaries cannot be inferred from success of a command.

Provider overviews are sufficient for this experiment. Any future generic overview should be justified by a second concrete provider needing shared behavior, not by these examples alone.

## 4. Evidence versus model output

Compact discovery changes only descriptions sent to the model; investigation dispatch and complete retained responses are unchanged. The paired Forth test requires byte-for-byte JSON equality of provider evidence between arms. Source/version, policy, selected operations and budgets are the same.

The host-driven Script demonstration retains `RunRecord` source/target snapshots, complete catalog, all three returned envelopes, printed values and outcome. Its final object selects vocabulary count and symbolic stack/status fields. References such as `results:1` are **record-local indexes into that actual serialized results array**, not durable external IDs. Only retained result slots receive indexes; partial/error runs expose `partial: true` and no nonexistent references. A denial has no final value and no successful results. Required records are obtained before rendering the report; failure to serialize/write the report returns an error.

No automatic summary generation, extra sensitive-source persistence in Scope, or new trace destination is introduced. The example intentionally captures a public static fixture in an opt-in report. Live reports contain raw messages/provider evidence and must be treated as sensitive. `Read` and interpreter success must not be confused with mathematical correctness. Later source revisions do not turn retained revision-0 observations into current evidence; the example is explicitly an immutable snapshot experiment.

The standalone Script demonstration is a success-path composition plus denial test, not a claim of a complete nested audit ledger. The normal investigation path remains the tested evidence-assessed agent interface.

## 5. Real downstream experiment

Files:

- `examples/ce1-forth/Cargo.toml`, `Cargo.lock`: standalone workspace with local Scope and sibling Scope Forth dependencies; does not add Forth to Scope's workspace dependencies.
- `examples/ce1-forth/src/main.rs`: real provider, identical fixed decisions and optional live `CellAgent` comparison.
- `examples/ce1-forth/src/script_composition.rs`: existing Script composition, separate from comparison arms.
- `examples/ce1-forth/deterministic.json`: retained complete paired conversations, evidence and Script record.
- `examples/ce1-forth/README.md`: commands, provenance, interpretation and privacy boundaries.

Source, revision 0 in both arms:

```forth
: square dup * ;
: misleading-square dup ;
```

Task: list document-local words and inspect both with one symbolic input; distinguish stack observations from arithmetic proof. The host checks actual vocabulary and final `constructed-conditional` symbolic stacks: `[0, 0]` for `misleading-square`, `[1]` for `square`. It does not accept free-form prose as proof or assert universal arithmetic safety. Both arms allow only `words` and `describe`, initially visible, with six inspections, ten decisions/model invocations, eight transport calls and 2,000 requested maximum tokens per decision.

The baseline is the **existing** investigation runner. The modified arm uses compact discovery. Default model is the same fixed `DecisionModel`, not a local LLM. The source, task prompt, authority, budgets and decisions are identical. No prompt tuning or candidate/write capability is used.

| Observed metric | Existing detailed discovery | Compact discovery |
|---|---:|---:|
| Decision-model invocations | 4 | 4 |
| Tool decisions / semantic invocations | 3 / 3 | 3 / 3 |
| Discovery decisions | 0 | 0 |
| Duplicate decisions | 0 | 0 |
| Cumulative message-content UTF-8 bytes | 53,254 | 28,886 |
| Outcome | InvestigationCompleted | InvestigationCompleted |
| Provider evidence | Three retained reads | Identical three reads |
| Measured input/output tokens | Unavailable | Unavailable |

The retained report also records elapsed milliseconds. These tiny deterministic synchronous timings vary by run; they are not a latency comparison. Byte counts sum message content at each decision, including repeated history; they exclude role/framing overhead and are not token counts. Zero discovery calls is deliberate for a two-operation catalog, not evidence that repeated-discovery behavior has been solved.

### Reproduce from Scope root

```sh
cargo test -p scope-ide --features agent --lib --quiet
cargo test --offline --locked --manifest-path examples/ce1-forth/Cargo.toml
cargo run --offline --locked --quiet --manifest-path examples/ce1-forth/Cargo.toml > examples/ce1-forth/deterministic.json
```

The example requires sibling `../scope-forth` and cached dependencies for offline builds. It makes no downstream edits. Its README records actual checkout revisions and pre-existing downstream dirty files; path dependencies are not immutable commit pins.

Optional live comparison (not executed/verified in CE1):

```sh
cargo run --offline --locked --quiet --manifest-path examples/ce1-forth/Cargo.toml -- --live MODEL BASE_URL > examples/ce1-forth/live.json
```

Supply an actually available OpenAI-compatible backend/model; optional `OPENAI_API_KEY` comes from the environment and is not included in reports by the harness. Both arms use the same `CellAgent` configuration and task. Backend stochasticity and sequential arm order remain confounders. Inspect outcome, evidence and raw responses; do not interpret process success as semantic task success. Backend calls can share source/evidence and incur costs. No endpoint/model availability was established, so CE1 does not claim a live-model result.

## 6. Regression coverage and validation

The Scope agent library suite passed **581 tests** after the change. New tests exercise canonical summary/detail equality, Unicode-safe visible truncation, explicit unknown metadata, full schema reaching actual model feedback, unknown/write-ineligible detail lookup, hidden-operation visibility isolation and model/tool budgets. Existing tests retain coverage of source changes, source-bound dispatch, invalid arguments, failed operations, independent authorization, repeated-call recovery and edit/proposal behavior.

The standalone actual-provider suite passed **4 tests**: equal paired evidence and smaller context, denied authority, three retained Script results with revision/projection checks, and Script denial without successful evidence. References on partial failure are additionally checked to be empty. The existing `scope-script` multi-operation integration suite passed **6 tests** in final validation; no interpreter code changed. `git diff --check` also passed. Builds emitted the existing unrelated `set_agent_config` dead-code warning.

Native provider time/allocation cancellation and outer/inner shared budget tests cannot be claimed for a new agent Script tool, because no such tool was added. No generic stale-projection cache, external reference resolver or bounded overview transport contract was added either. Those are not hidden implementation guarantees.

## 7. What next / what not next

1. Run the supplied fixed-prompt live comparison on the intended small local model. Record actual usage when available, correctness, detail calls, repeats, backend failures and elapsed time. Repeat with counterbalanced arm order before general claims.
2. If compact descriptions cause extra schema failures/detail round-trips, retain detailed discovery for small catalogs rather than forcing laziness. Initial task-relevant subsets already solve part of the problem.
3. Measure repeated continuation evidence separately: it remains a source of context growth. A future explicit host projection callback should retain full evidence and test failures/revisions/byte limits before becoming a public contract.
4. Only if live evidence supports it, design a shared bounded execution ledger and inherited call-aware authority for Script-in-agent. Charge all attempts and cumulative retained/result bytes; distinguish semantic failure from interpreter completion; retain attributable failed calls. Do not merely wrap the existing interpreter.
5. Keep Forth/Path/Hilb semantic knowledge downstream. Do not add a universal symbol graph, automatic result wiring, parser, vector database, prompt mutation, hidden summarization, second event store or second runner.

**Answer to the CE1 question:** Scope can already perform provider-specific semantic investigation and host-driven multi-read projection while retaining underlying observations. A minimal normal-agent discovery adapter measurably reduces deterministic context overhead with unchanged evidence and authority. Whether this helps a relatively small local model finish useful tasks with fewer interactions remains an open, reproducibly testable question—not a demonstrated success of this milestone.
