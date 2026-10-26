# CE3: ordinary Path candidate evaluation feedback

Standalone workspace; owns only `examples/ce3-path`. Reads the real sibling
`../../../path/crates/path-scope` provider. No production/downstream changes,
new agent loop, workflow, taxonomy, cache, or candidate application.

## Reproduce

From this directory (with the sibling Path checkout and cached dependencies):

```sh
cargo test --offline
cargo build --offline
cargo run --offline --quiet > deterministic.json
cargo run --offline --quiet -- --live MODEL BASE_URL > live.json
```

The checked-in report was generated with the third command. Elapsed microseconds
vary; the semantic results and message-content byte measurements are deterministic
for these source/dependency versions. Live mode was **not run**. It uses `CellAgent`,
reads `OPENAI_API_KEY` when present, and sends source, catalog, and feedback to the
configured endpoint; this may incur costs. Reports retain message contents, so
review before sharing. Credentials/authorization headers are not recorded.

## Design

All arms use production `run_session`, the same projected semantic catalog,
read-only policy, source, task, and budgets. Only `evaluate_goal_candidate` is
installed. Source/revision preconditions are host-bound by
`SemanticToolProvider::with_projected_arguments`, not chosen by the model.

The immutable real Path source fixture is `application.path`, loaded using
`include_str!` as `SOURCE` in `src/main.rs`:

```text
theorem application (A : Type 0) (B : Type 0) (f : Pi (a : A). B) (x : A) : B := ?
```

`x` has type A and is rejected at the B goal; `f x` completes the kernel check.
Evaluation never applies either candidate. Every run checks source bytes,
revision, dirty state, and absence of edit proposals afterward.

- **baseline** returns the complete provider envelope.
- **compact** copies actual envelope fields: status, operation, rationale;
  classification, replacement, goal ID, selected goal, invalid-request reason,
  feedback, explanation, application authorization; and original/candidate input,
  outcome, diagnostics, goals, kernel, and limitations. It omits duplicate source
  strings, stage records, and other detail; it is a selective projection, not a
  substitute for all provider evidence. Source identity/text/revision are also
  supplied identically in each task and retained in `source_binding`. Provider
  `input` carries its actual document version/path. No derived classification.
- **withheld** returns only an explicit feedback-withheld marker for returned
  provider results. Transport errors remain errors. Full envelopes still appear
  in the host-local report, never in control model feedback.

The local wrapper calls the real provider on **every** invocation, retaining
arguments, full envelope/error, delivered projection, and evaluation elapsed time.
No cache or durable evidence-reference system is introduced. Reports include
request messages, decision outputs/errors, message-content UTF-8 byte totals,
provider classifications, evaluation/call/decision counts, exact repeated-argument
counts, consecutive candidate-text revisions, source revisions, and elapsed time.
Candidate revisions count text changes between evaluations, not source edits.
Request snapshots are not a terminal-feedback log: feedback after a final budget
stop might never appear in a subsequent request. Pre-dispatch failures do not
count as provider evaluations. Byte budgets are checked after provider return,
not allocation/retention guarantees. Token usage is unavailable and is null.

## Deterministic results and limitations

Three trials rotate arm order to balance positions. In deterministic mode each
arm has exactly the **prescribed** sequence `x`, `f x`, complete. In particular,
the withheld control's second choice is prescribed, not inferred from feedback.
Live mode instead lets the model choose all candidates and when to stop; there
is no seeded rejected candidate. Models can stop without evidence of completion;
`observed_completed` is a host evidence assessment, separate from final prose.

| Per run (identical in all three trials) | baseline | compact | withheld |
| --- | ---: | ---: | ---: |
| Decisions including completion | 3 | 3 | 3 |
| Provider evaluations / charged calls | 2 | 2 | 2 |
| Cumulative message-content UTF-8 bytes | 41,303 | 26,867 | 9,716 |
| Repeated argument sets | 0 | 0 | 0 |
| Candidate-text revisions | 1 | 1 | 1 |

All nine runs observe `RejectedCandidate`, then `Completed`, and unchanged source.
Compact reduces cumulative message-content bytes by about **35.0%** versus raw.
The control is smaller because evidence is removed, not because it is equivalent.
No turn-count/evaluation improvement is claimed. This replay does not demonstrate
LLM reasoning, quality, feedback utility, or causal latency improvements. Bytes
exclude roles/framing and are not wire bytes, tokens, or cost. Timing includes
local overhead. Answer quality and actual live usage need separate assessment.

## Validation

`cargo test --offline` passes six tests covering real `InvalidRequest`,
`StillIncomplete`, `RejectedCandidate`, and `Completed` classifications; nonempty
rejection diagnostics; actual kernel completion; copied-field projection fidelity;
raw/control delivery; equal catalogs; repeat/revision accounting; and unchanged
source. Focused harness coverage also checks unknown tools, unknown/typed-invalid
arguments, unauthorized operations, shared tool-budget exhaustion with partial
evidence, partial/failure envelope passthrough, and synthetic `Inconclusive`
preservation. The synthetic projection fixtures are **not** real Path-produced
outcomes. Invalid arguments return provider validation envelopes but never reach
semantic authorization/execution; unknown tools and policy denials never reach
the wrapper. Semantic authorization counts are retained separately from evaluations.

A `DecisionModel` error is tested after one real rejection: `session_result`
serializes the error and preserves evidence, failed request snapshots, and source
checks. The test uses local model exhaustion, not a live network error. On session
error, decision count comes from captured requests; unavailable charged tool-call
counts, stop, and conclusion are null rather than invented. Main retains setup
errors as failed trial entries and continues other arms/trials. A successful
session stopped by a budget remains distinguishable from an error via
`session_result` and `stop`; neither implies observed completion.

`cargo build --offline` and report generation also passed, each with a
200-second bound. Existing dependency duplicate-package and `scope-ide`
`set_agent_config` dead-code warnings were not changed.
