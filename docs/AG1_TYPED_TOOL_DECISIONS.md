# AG1 — Typed tool decisions and feedback recovery

## Pre-implementation audit

The supplied Cross observation was `experiment.evaluate` with `arguments:{"input":0}` despite a declared required integer `k`. The provider returned an ordinary successful Scope envelope whose value was `{"status":"rejected","failure":"expected only integer k in 0..=10","remaining":4}`. The model then completed. Two decisions, one call, zero scientific evaluations and four unused evaluations were reported. Copying the prompt example is a plausible hypothesis, not established causation.

`crates/scope-ide/src/agent_loop.rs::PROTOCOL` generates the generic `{"input":"value"}` example. `run_session_inner` sends it as the system message and serializes `session.catalog()` into the initial request. `tool.rs::SessionCatalog::to_value` already exports exact names, kinds, required flags, descriptions, effects and provider metadata. These are adequate authoritative data, but the competing concrete argument example is avoidable ambiguity, especially for a small model. No live model readability claim is established.

`agent.rs::CellAgent` implements `DecisionModel` through the configured chat adapter; its `run_session` delegates to the ordinary bounded loop. `DecisionModel` receives the accumulated messages and token cap. The parser enforces one typed JSON decision, integer-only numbers, and unique object keys. Protocol validity is not argument-contract validity.

`tool.rs::AgentSession::invoke_observed_with_call_policy` resolves registered names, checks host policy against the original call, and only then invokes `ToolProvider::invoke` (or `invoke_with_app`). It does **not** centrally validate `ToolDefinition.inputs`. Providers own validation, including unknown-key policy and domain constraints. Built-in helpers return `MissingArgument`/`InvalidArgument`; those variants do not prove that a generic schema validator ran. Arbitrary provider values, including domain rejection inside `Ok(ToolResult)`, remain opaque data. Cross owned the observed rejection. Moving its range validation into Scope is not justified.

The loop charges every attempted tool decision against the shared call budget before dispatch, including denied, unknown and invalid calls. Errors currently reach the model as a user `tool_result` envelope with `ok:false` and the displayed error. Successful provider values reach it unchanged under `ok:true,value`. Protocol errors instead have `kind:protocol_error`, category and correction instructions and consume the malformed allowance. Message/conversation bounds also apply to feedback.

Completion is a valid independent decision and returns immediately, even after rejection. Existing guidance says “Correct errors using feedback” but also “Complete when done or explain what is missing.” Clarifying optional correction is justified; forbidding completion would override model autonomy without evidence that another operation is appropriate. No automatic retries or argument rewriting are warranted.

Duplicate registration is rejected by the session builder; workflow-name collisions are rejected before model invocation. The protocol rejects duplicate JSON keys. The ordinary runner does not suppress repeated valid tool decisions; edit/investigation lifecycles have their own evidence/state constraints. Malformed recovery does not replay a previous write. Traced execution retains requests, raw responses, parse outcomes and feedback with retention status (`run_session_traced`, `agent/trace.rs`); normal conversations retain tool results within existing byte limits.

Existing `agent_loop_tests.rs` tests cover malformed framing/decision recovery, duplicate keys, budgets, authorization and no write replay (`schema_errors_are_invalid_decisions_and_recover_without_dispatch`, `decision_framing_failures_never_dispatch`, `malformed_recovery_and_exhaustion_never_dispatch`, `recovery_after_successful_write_never_replays_the_write`). These are not provider argument correction tests; AG1 adds that missing case.

## Chosen minimal change

Use prose for tool-decision structure rather than a fabricated argument object. A generic empty object risks being copied for required-input tools; schema-derived values would require inventing valid domain values. Keep the completion JSON example, but describe the three tool fields and direct argument names/types solely to the selected registered definition. Explicitly allow `{}` only when the contract permits it. Clarify that feedback can support another model-proposed call within remaining budgets, while stopping remains legitimate.

For provider-reported typed argument errors only, enrich the existing error envelope with the error category, supplied keys, the selected authoritative catalog definition, and remaining decision/call allowances. Preserve the exact error string. These are budget counts, not a promise of authorization, byte capacity or successful execution. Do not classify `Ok` domain rejections as schema failures.

### Before / after

Before: `{"type":"tool","name":"registered.tool.name","arguments":{"input":"value"}}`.

After: tool decisions have exactly `type` (literal `tool`), `name` (registered name), and `arguments` (object matching that tool's contract). There is no universal argument key or invented argument value.

## Validation and follow-up

### Deterministic regression results

`agent_loop_tests.rs` adds:

- `ag1_registered_contract_feedback_and_recovery`: deliberately different required integer `k`, required string `path`, and empty contracts. Checks exact exported names/kinds/required flags, unknown-key/missing/wrong-type errors, useful feedback, original supplied arguments, successful subsequent correction and empty-object dispatch.
- `ag1_rejections_preserve_authority_and_budgets`: denied calls never reach the provider, original arguments reach policy, failed calls consume call/decision budgets, and completion remains possible after failure.
- `ag1_structured_rejection_is_ok_and_traces_correction_without_enrichment`: Scope-local Cross-equivalent `experiment.evaluate` fixture uses required integer `k` in `0..=10`, first receives `input:0`, and returns the reported rejection with four remaining evaluations. Both the original reject-then-complete sequence and a scripted reject-then-`k:0` correction are possible. Traced and untraced correction runs agree; rejection remains `ok:true` and is not relabeled as a schema error. This fixture models protocol messages, not Hilb computation or scientific scoring.

The scripted correction takes three decisions and two tool calls, with no malformed decisions. This establishes execution capability, **not improved model reasoning**. The original two-decision stop remains legal.

Existing specialized lifecycle tests already cover argument correction: `edit_lifecycle_tests.rs::invalid_arguments_changed_calls_recover_despite_identical_errors`, `one_correction_then_valid_proposal`, and `investigation_tests.rs::invalid_read_arguments_can_be_corrected_without_losing_rejection_evidence`. Duplicate suppression remains lifecycle-specific: identical invalid/failed candidates are suppressed, changed arguments may restore eligibility, and suppressed investigation reads may reuse retained evidence within retry bounds. AG1 changes none of these paths.

### Validation

Passed:

```sh
cargo fmt --check
cargo test -p scope-ide --features agent --lib --offline
cargo test -p scope-ide --features agent --offline
cargo check -p scope-ide --features agent --example ag1_typed_tools --offline
git diff --check
cargo test --manifest-path examples/ce3-path/Cargo.toml --locked --offline
```

The normal agent library suite passed **591 tests**. The CE3 fixture suite passed **6 tests**. Library coverage includes ordinary protocol parsing, host policy, shared workflow budgets, edit/investigation lifecycles, compact discovery, semantic evidence and traces. The full package test command also passed. Existing duplicate Cell-package warnings and an unrelated `set_agent_config` dead-code warning remain.

Additional CE1 and CE2 fixture suites were attempted:

```sh
cargo test --manifest-path examples/ce1-forth/Cargo.toml --locked --offline
cargo test --manifest-path examples/ce2-forth/Cargo.toml --locked --offline
```

CE1 failed linking with `No space left on device`; CE2 failed creating its compiler-output fingerprint during the same parallel build. They did not reach test execution. These failures do not establish a compatibility regression. No downstream source was changed and no existing build caches were removed to make room.

### Controlled live comparison

No working, explicitly authorized LM Studio backend was established for this execution environment. **No inference requests or endpoint probes were made.** The supplied model name and historical localhost configuration are not evidence of current availability or explicit endpoint authorization. There are no live-model results, elapsed inference measurements or token counts to report.

A reproducible opt-in comparison uses the normal runner, not another execution loop:

```sh
cargo run --offline -p scope-ide --features agent --example ag1_typed_tools -- \
  --live qwen3-30b-a3b http://127.0.0.1:1234/v1 > ag1-live.jsonl
```

Run only after approving that endpoint and loading that model. It sends synthetic prompts/catalog/results to the endpoint and can incur backend costs; traces contain that data. The fixture is a fixed four-integer task, **not a reproduction of Cross's scientific task**. Both arms hold the task, catalog, model adapter/sampling defaults, read-only authorization, four accepted-evaluation allowance, eight tool calls, twelve decisions, two malformed decisions, 700-token decision cap and byte limits constant. The baseline adapter substitutes the exact old system guidance; AG1 uses production guidance. Because this provider returns ordinary `Ok` domain rejections, neither arm receives typed argument-error enrichment: only protocol guidance differs.

Each JSONL record retains protocol events and measures first provider-call arguments, valid-dispatch count/rate, attempts to first valid provider dispatch, correction after rejection, completion, decisions, invalid calls, elapsed milliseconds and actual model-facing request/response UTF-8 bytes. For baseline request events, the example records the substituted system text actually delivered by the adapter. Actual tokens are `null`: the existing adapter does not expose usage. Bytes are not a token estimate. Unknown-tool calls count against the ordinary call budget but do not reach the fixture; provider-attempt metrics distinguish that boundary. One baseline-then-AG1 pair is only an initial observation; repeated/counterbalanced trials and content review are needed for a robust practical claim.

### Remaining limitations and Cross follow-up

- Provider validation remains authoritative; registration is descriptive, not automatic schema enforcement. Unknown keys follow each provider's existing contract. AG1 does not make permissive providers reject unknown fields.
- `MissingArgument`/`InvalidArgument` identify provider-reported typed argument errors, not proof of a generic schema-validation pass. Domain constraints may also use these variants. Feedback provides the registered contract and original error without claiming to know the internal cause.
- Remaining counts describe the ordinary loop's decision/call budgets only. They do not promise host authorization, downstream evaluation allowance, remaining conversation bytes or useful results. Enriched feedback itself consumes the same message/conversation budgets and can hit their limits.
- Successful provider envelopes remain opaque even when their values report rejection. Scope cannot infer scientific success from `ok:true`, nor should it require further action after rejection.
- Ask Cross to retain its exact provider contract, argument-validation policy and remaining-evaluation feedback; clarify separately whether missing/unknown keys should return typed `ToolError` or intentionally remain domain-level `Ok` results. That is a downstream API choice, not an AG1 migration requirement.
- Ask Cross to rerun the controlled eleven-circuit/four-evaluation experiment with identical backend settings and task, retain exact prompts/results and real usage if available, and review the model's incorrect range explanation. AG1 does not fix downstream validation, invent scientific evidence, or establish why the original model stopped.

No `DecisionModel` API, registry, authority, effect, dispatch, candidate semantics or trace-retention behavior changed. No Cell, Cross, Path, Hilb or Forth source was modified.
