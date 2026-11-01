# H3 — Evidence-guided ordinary agent continuation

## 1. Baseline feedback audit

The existing path already supports continuation:

```text
DecisionModel::decide
  → parsed ToolCall
  → AgentSession::invoke_with_call_policy
  → registered ToolProvider / SemanticToolProvider
  → execute_semantic_operation / LanguageService
  → validated native SemanticOperationResult
  → ToolResult.value
  → JSON user-message envelope (kind = tool_result)
  → next ordinary decision
```

`SessionCatalog` describes installed tools and argument contracts; dispatch still
checks the host's per-call policy. `SemanticToolProvider` separately validates
source preconditions, arguments, permitted document scope and semantic host policy.
It executes the downstream service and checks live source identity/text/revision/
language again. Model feedback normally contains the entire representable semantic
value, rationale and selected existing evaluation fields under a status envelope.
A successful dispatch produces `ok: true` even when the semantic report says the
candidate was rejected, blocked or incomplete.

`ToolValue` supports signed integers but not arbitrary JSON numbers. Consequently,
a native semantic result can be valid while ordinary feedback reports
`internal_failure` because its payload is unrepresentable. The provider's
`native_result()` retains validated read/blocked native observations independently,
including such numbers; it clears that slot before invocation. It does not expose
unvalidated results or proposals. H1 tool output is trusted in-process evidence,
excluded from serialized `AgentTurn` snapshots.

H2 adds revision-bound initial context, evidence references and explicit selection
completeness, not native-result retrieval. `ContextAssembly` describes construction;
`ContextSupplied` describes initial model delivery. Neither classifies tool results.
The runner already appends JSON feedback with ordinary message/conversation byte
accounting and allows another decision under decision/tool/malformed budgets.
Therefore H3 needs **no new continuation loop**. The narrow selection point is
inside the semantic provider after validation/freshness checking, before native
payload conversion becomes ordinary tool feedback.

These remain distinct:

| Event | Establishes | Does not establish |
|---|---|---|
| Tool returns `Ok` | Dispatch returned a result | Domain success |
| Semantic data returned | A downstream report is available | That all claims are verified |
| Explicit Path kernel classification | The selected candidate's downstream outcome | A committed source edit |
| Model interprets feedback | The model chose a next action | New independent verification |
| `AgentState::Completed` | Protocol completion | Verified task success |
| Human accepts a proposal | A mutation decision | Mathematical correctness |

Relevant code: `crates/scope-ide/src/{tool.rs,agent_loop.rs,agent_lifecycle.rs,
semantic_context.rs}`, `tool/semantic.rs` and `cli/retained_agent.rs`.

## 2. Safe projection contract

Agent-enabled hosts may explicitly configure
`SemanticToolProvider::with_verification_projection` with:

- A language-local operation-ID allowlist, independent of execution permissions.
- A trusted `SemanticResultProjector` that selects public fields from validated
  native results, operation description, source snapshot and validated arguments.
- A serialized projection-envelope byte cap, additionally capped at 4,096 bytes.

The types in `tool/verification.rs` express `Verified`, `Rejected`, or
`Inconclusive`; source-snapshot versus hypothetical-candidate subject;
`SelectedFields` versus `Unavailable` completeness; a public host-declared authority
label; and explicit safe `ToolValue` fields. These three classifications are a
transport vocabulary, **not a universal verifier**. The host adapter—not generic
Scope code—decides what authoritative downstream evidence establishes them.

The provider supplies the producing fully qualified tool name and exact source
identity, language and revision. The host selection cannot substitute that
provenance. Model-facing revisions are decimal strings so even `u64::MAX` does not
require a lossy signed-integer conversion. Retained metadata preserves typed `u64`.

For configured read operations with a validated native observation, the provider
**replaces**, rather than supplements, the ordinary native-payload feedback:

```text
kind: tool_result
call: <ordinary tool ordinal>
name: <actual tool name>
ok: true
value:
  status: verification_observation
  observation:
    operation / source / classification / subject / completeness / authority
  fields: <explicitly selected public values>
```

Native payloads are not converted through `ToolValue` first to determine the
classification. For example, a valid native report containing fractional numbers
and protected strings can produce a small boolean/status selection without
exposing either. `native_result()` retains the original trusted report. No new
retention registry or native-result database is introduced.

Unauthorized, malformed, stale and missing-native invocations do not run the
projector or reuse an earlier observation. They retain ordinary error/status
feedback and produce no verification evidence. Proposals are not projected. A
missing projection is not an empty successful verification.

Oversized or invalid selections become an explicit bounded `Inconclusive` /
`Unavailable` observation with null fields and a fixed reason. If even the fallback
cannot fit the configured cap, projection returns a sanitized execution error.
Mandatory provenance is never truncated or rebound to a dummy source. Existing
runner message and conversation limits apply to the **entire** feedback envelope,
not just the projection's inner cap. A budget stop can prevent feedback delivery
while retaining an already obtained observation.

The projector is trusted host code, not a sandbox or secret detector. Public labels
and selected fields must be reviewed by its author. Scope never infers verification
from an operation name, successful dispatch, model prose, arbitrary `success` field
or a string containing `passed`. Text selected from diagnostics remains untrusted
data under the existing tool-feedback protocol and cannot grant capabilities.

## 3. Integration, provenance and lifecycle

`cli::run_retained_scratch_agent_verification_observed` opts into H2 context plus
projection via `RetainedVerificationConfig`. All previous entry points preserve
their behavior. Provider configuration occurs before existing host-bound argument
projection. The immutable retained borrow pins the source across assembly, semantic
execution and continuation. Both ordinary and semantic policies still authorize
every tool call; the projection allowlist does not authorize execution.

`ToolResult` carries an agent-feature-gated optional typed observation. The ordinary
runner validates it against the actual tool name and the turn's current source
binding before sending feedback. Invalid attribution fails the turn with a
sanitized execution error. Hosts using the generic runner must supply the correct
current binding; the retained runner also keeps the private owner identity used by
its existing freshness check. Serialized owner-local IDs are not globally unique.

The runner appends metadata-only:

```text
AgentEvidence::Verification
  request_ordinal
  call_ordinal
  observation
    operation
    source identity / revision / language
    classification
    subject
    completeness
    authority
```

It follows the matching successful Tool evidence, before cancellation checks.
Thus cancellation after obtaining a result does not erase it. Raw selected fields,
native results, prompts and candidate replacement text are not copied into this
public evidence. The invocation ordinal identifies which hypothetical candidate
was evaluated; its baseline revision is not a claim that the candidate was committed.
No mutation, proposal acceptance or source-revision advancement occurs.

Live and retained import validation reject mismatched names/revisions, observations
without a matching success, duplicate/delayed observations, invalid labels and
oversized metadata. The existing `AgentTurnStore`, `scope.agent.get` and typed
import expose these records to non-UI consumers as read-only observations. A turn
with `Completed`, successful dispatch and `Rejected` verification is valid.
A budget-exhausted turn may also retain verified evidence.

Typed observations are intentionally unsupported inside compound workflows in this
milestone: they fail closed, including when a workflow catches an inner error.
This avoids attributing an inner operation's verification to an outer workflow
call. Existing workflow charges remain authoritative. Supporting that nesting
would need explicit inner-call attribution, not silent flattening.

Compatibility: the new evidence variant is additive, but Rust consumers matching
the enum exhaustively must handle it. Direct `ToolResult { value }` literals in
agent-enabled code must use `ToolResult::new(value)` or initialize the optional
field. Tools-only builds do not acquire agent-type dependencies.

## 4. Path public-interface investigation

No downstream changes were required. Path's existing public operations include:

| Operation | Role |
|---|---|
| `inspect_example` | Kernel-checks an internally compiled associativity fixture; not arbitrary source |
| `check_source` | Checks source and exposes snapshot-local goals |
| `evaluate_goal_candidate` | Evaluates a replacement for a validated authored goal in a hypothetical whole document |
| `propose_goal_candidate` | Produces inert edits for host review; excluded from these read-only runs |

The existing retained interface supports argument-bearing read/document operations,
so the separate no-argument interactive-inspection restriction is not a blocker.
The experiment uses `evaluate_goal_candidate`, with `goal_id = 0`, on:

```text
theorem trial (A : Type 0) (x : A) : Id A x x := ?
```

Actual downstream outcomes:

- `refl x`: classification `Completed`, candidate outcome `Checked`, and candidate
  kernel checked = true.
- `x`: `RejectedCandidate`, candidate outcome `MathematicalRejection`.
- `?`: `StillIncomplete`, candidate outcome `Incomplete`; not a closed proof.

The example's Path-specific projector requires all three positive fields for
`Verified`. It maps Path's explicit rejected classification to `Rejected` and all
remaining cases to `Inconclusive`. `RejectedCandidate` can include frontend
rejections too; the chosen `x` case specifically establishes mathematical rejection.
Recognized incomplete reports have selected-fields completeness; missing or
unrecognized native reports are unavailable with null fields. The only selected
fields are classification, candidate outcome, kernel-checked flag and validated
request goal ID. No source, full proof payload or diagnostic prose is copied.

Goal 0 is supplied from the known fixture contract, not discovered by the model;
Path independently revalidates that goal on each evaluation. The generic harness
contains no Path proof rules. The example adds a direct path dependency on the
existing `scope-language` crate solely to name its native semantic contracts.

## 5. Controlled experiment

The existing `examples/h1-path-agent` executable gains `h3-control`, implemented in
`src/continuation.rs`, not a separate runner/workspace. It runs baseline, H2 and H3
for positive, rejection/refinement and incomplete scenarios. Each scenario keeps
its source bytes, task, permissions and budgets equal across variants:
4 decisions, 2 tool calls, 1 malformed response, 320 output tokens per decision,
65,536 message bytes and 196,608 conversation bytes.

The offline decision model reads **actual feedback**. Rejection produces a new
`refl x` evaluation; verified feedback completes as verified; inconclusive feedback
completes as unresolved. Baseline/H2 controls read the existing native-report
fields; H3 reads the projected classification. Counterfactual control assertions
check that changing feedback changes the next decision. These assertions test the
control, not a substitute verifier: every experimental evaluation uses the real
Path service/kernel.

Every run independently reevaluates the final selected candidate through Path's
public service outside the agent loop. This extra validation is explicitly counted
as one outside-budget host check, not an uncharged agent operation. H3 also compares
projector-observed native classifications with tool feedback and retained typed
metadata. A separate tool session retrieves and imports the sanitized terminal turn.

Reproduce with the sibling Path checkout and cached dependencies described in the
harness README:

```sh
cargo build --offline --locked --manifest-path examples/h1-path-agent/Cargo.toml
examples/h1-path-agent/target/debug/scope-h1-experiment h3-control
```

Routine output goes to ignored `target/results/h3-control-summary.json`. The reviewed
snapshot is `examples/h1-path-agent/evidence/h3-control-summary.json`; routine runs
do not overwrite it. Local working-tree dependencies are not a pin of downstream
source revisions. Historical H1/H2 modes/evidence remain available.

### Measurements

| Scenario | Variant | Initial input bytes | Feedback content bytes | Cumulative input bytes | Decisions / calls | Domain outcome |
|---|---|---:|---:|---:|---:|---|
| Positive | Baseline | 3,736 | 12,868 | 20,473 | 2 / 1 | Verified |
| Positive | H2 | 3,454 | 12,868 | 19,909 | 2 / 1 | Verified |
| Positive | H3 | 3,454 | 568 | 7,609 | 2 / 1 | Verified |
| Refinement | Baseline | 3,731 | 26,752 | 52,218 | 3 / 2 | Rejected → verified |
| Refinement | H2 | 3,449 | 26,752 | 51,372 | 3 / 2 | Rejected → verified |
| Refinement | H3 | 3,449 | 1,151 | 12,470 | 3 / 2 | Rejected → verified |
| Incomplete | Baseline | 3,731 | 15,009 | 22,599 | 2 / 1 | Inconclusive/unresolved |
| Incomplete | H2 | 3,449 | 15,009 | 22,035 | 2 / 1 | Inconclusive/unresolved |
| Incomplete | H3 | 3,449 | 580 | 7,606 | 2 / 1 | Inconclusive/unresolved |

All nine runs passed: source fresh and unchanged, protocol Completed, store/get/import
valid, no budget exhaustion. Each refinement has exactly one real rejection and one
successful refinement. Each incomplete run has one successful dispatch, zero verified
candidates and an unresolved conclusion. H3 retains one verification record per
call; baseline/H2 retain none of the new records. Independent final validations
match actual feedback in every case.

Input measurements count UTF-8 roles plus message contents, not HTTP framing or
tokens. Cumulative input counts retransmitted conversation on each decision. Feedback
measurements sum distinct consumed feedback messages; these cases have one or two
unique messages. H1 charged conversation bytes count accepted content once and are
retained separately in JSON. H2 and H3 initial context bytes match; only result
projection changes their feedback. Byte reductions do not establish lower latency,
model token usage or billing costs.

H3 reduces feedback by about 95.6% (positive), 95.7% (refinement) and 96.1%
(incomplete), versus H2's complete raw reports. Cumulative input drops about 61.8%,
75.7% and 65.5%, respectively. This benefit is intentionally selecting less data;
it is not lossless report compression. Detailed diagnostics and full candidate
reports are omitted, potentially limiting more complicated repairs.

Decision counts and task outcomes do not improve relative to the capable domain-
specific deterministic baseline. Rejection demonstrably changes the next action,
but the repair expression is supplied by the control/task, not synthesized by a
language model. Unlike the earlier compiled fixture, these are real authored
candidate checks; they still do not demonstrate general autonomous proof synthesis.

No LM Studio availability probe or local-model runs were performed for H3. Existing
H1/H2 real modes do not constitute H3 evidence. No paid endpoint was used. A future
bounded paired local-model experiment must keep generation settings fixed and
report stochastic variability rather than treating one success as reliability.

## 6. Regression assurance

Focused lasting-contract tests cover positive/rejected/inconclusive metadata,
projection allowlisting and denied authorization, invalid/missing/stale observations,
large/unrepresentable native values, secret exclusion, full-u64 revision transport,
strict projection/fallback caps, source and candidate attribution, incomplete versus
unavailable selection, feedback-dependent continuation, message/conversation caps,
verified evidence surviving a budget stop, cancellation after a result, metadata-only
serialization/import, duplicate/delayed/tampered observations and terminal outcomes.
Adversarial selected feedback telling the model to ignore authorization still meets
a denied subsequent call: the protected provider never executes.

Validation performed:

- `cargo test -p scope-ide --lib --features agent --offline`: **655 passed** in the
  final full agent-enabled library run.
- `cargo test -p scope-ide --lib --all-features --offline`: **746 passed**, including
  the final verification coverage and existing UI/lifecycle tests.
- `cargo test --workspace --lib --offline`: **passed**, across workspace libraries.
- Tools-only library check: passed; agent-gated projection types do not break it.
- Offline locked example build and all nine real Path deterministic controls: passed.

The full workspace integration command was not rerun. H2 previously timed out in
`source_proposals_through_real_launcher`, an existing PTY test; passing workspace
library tests does not imply that full integration suite has passed. No unrelated
launcher or downstream changes were made for H3.

## 7. Final design questions

1. **Independent semantic results as ordinary feedback?** Yes. Explicit trusted
   host adapters project validated native reports into the existing tool feedback
   path, with source and invocation attribution. Generic Scope code verifies that
   attribution and boundedness, not Path's mathematics.
2. **Does rejection/inconclusiveness influence decisions?** Yes in the measured
   deterministic control: rejection adds a distinct candidate check (3 decisions /
   2 calls rather than 2 / 1); inconclusiveness produces unresolved completion and
   no fabricated verification. No claim about language-model behavior is established.
3. **Minimum cross-language structure?** The implemented source/invocation metadata,
   subject distinction, authority label, three outcome categories and selection
   completeness are enough for bounded transport. Another language needs one reviewed
   native-result projector and domain-specific tests—not a universal proof schema.
   More complex repair may need a few additional safe diagnostic codes, selected by
   that domain adapter rather than automatic raw diagnostic export.
4. **Ready for systematic harness evaluation?** Yes for a narrow evaluation of this
   interface: fixed paired tasks, budget/byte/outcome accounting and several local-
   model repetitions. No new orchestration layer is justified. The smallest next
   capability is an H3-aware local-model adapter in this same harness, using the
   existing fixed generation settings and projected reports, to test whether the
   byte savings preserve useful repair behavior. If failures reveal insufficient
   diagnostics, add one safe Path diagnostic code field, not a general framework.
