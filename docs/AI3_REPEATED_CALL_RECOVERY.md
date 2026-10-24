# AI3 — bounded repeated-call recovery

## Decision

Reuse the existing public `SemanticInvestigationOptions.budgets.repeated_calls`.
No API, planner, cache, provider retry, or new recovery budget is needed.
Default `0` retains immediate `NoProgress`; `1` tolerates one suppressed attempt
and supplies feedback before the next decision, if existing budgets permit it.
This is a cumulative run-wide allowance, not one allowance per distinct call.

AI3 fixes two investigation presentation conflicts in the shared loop: duplicate
feedback and continuation completion guidance previously referred to `Propose`,
despite the investigation system instruction correctly requiring a conclusion.
Investigation guidance now permits a model-authored conclusion or explicit
uncertainty, points to retained structured evidence, and makes no sufficiency
claim. Editing guidance, default budgets, and dispatch behavior are unchanged.

## Reconciled implementation

Sources: `crates/scope-ide/src/{agent_loop.rs,edit_lifecycle.rs,tool.rs}` and
`crates/scope-ide/src/tool/semantic.rs`.

- Suppression compares the entire `ToolCall` (exact registered name and typed
  arguments) **and** the host's relevant-state token against all prior dispatched
  attempts in the run. No fuzzy semantic-equivalence detection exists.
- Semantic names are `language.<language_id>.<operation.id>`. The semantic runner
  supplies a token containing target `source_id`, version, and exact source text.
  Generic edit-session callers supply their own relevant-state function; their
  state guarantees depend on that function. Provider metadata is not a separate
  call-identity key.
- Objects are recursively represented by `BTreeMap`: JSON key order is irrelevant,
  including nested objects. Array order, string contents, argument values and
  names remain significant. Unsupported numbers and duplicate JSON keys fail
  decision parsing rather than creating ambiguous identity.
- Changed relevant source/version makes the pair distinct, but does **not** refresh
  the immutable semantic baseline or authorize use of a changed source. Existing
  pre/post invocation freshness checks still reject conflicts.
- Calls enter suppression history before session invocation. Returned structured
  semantic failures (for example `invalid_arguments`) are retained and can be
  suppressed just like successes. Session errors terminate: denied →
  `AuthorizationFailure`, unknown → `UnsupportedRequest`, other errors →
  `AgentBackendFailure`; they do not receive an extra duplicate recovery.
- Visibility/phase checks precede duplicate detection. A suppressed attempt emits
  `EditEvent::RepeatedCall`; summary `duplicate_calls_suppressed` counts it.
  It consumes a model decision and the cumulative repetition allowance, but no
  provider invocation, inspection/evaluation/refinement, or `ToolCall` event.
  The transport tool-call limit counts distinct dispatched attempts plus discovery,
  not suppressed attempts. Both model-invocation and transport-decision limits
  still apply to every subsequent decision.
- `repeated > budgets.repeated_calls` records `NoProgressReason` with
  `repeated_suppressed_operation`, preserves the actual last result, and terminates
  `NoProgress`. With default zero the feedback is constructed internally but is
  **not delivered to another model decision**. With one, the next decision gets it.
- Parsing retries use `transport.max_malformed`; lifecycle proposal corrections use
  `corrections`. Neither is reset or consumed merely by duplicate suppression.
  Parsing retries can add decisions during recovery, bounded by their own counter
  and global model/transport limits. Thus one repetition opportunity is not a
  promise of exactly one additional transport call under malformed output.
- Ordinary semantic inspection limits remain independent. Discovery also consumes
  model and transport tool-call resources. Budget/size exits remain
  `OperationBudgetExhausted`; malformed retry exhaustion remains `NoProgress`.

## What the model sees

The semantic lifecycle reconstructs each conversation from system protocol,
immutable objective/source request, host `edit_continuation`, optional feedback,
and phase/catalog/budgets. It does **not** replay prior raw assistant decisions.
Continuation evidence nevertheless supplies the exact previous operation name,
arguments, structured result (including status and provider limitations), and
relevant source identity/version. Exact baseline source is in the immutable
request rather than repeated in each evidence token. No AI3 summary is generated.

Before AI3 the system already described the `complete` decision and investigative
completion, but continuation fields and duplicate feedback contradicted that
instruction by describing edit-phase advancement. Correcting this ambiguity is
not evidence that it caused FW7 repetition. Missing understanding, uncertainty
about sufficiency, or unrelated model behavior remain plausible. FW7's reported
Forth derivation and proof limitations are experiment observations, not verified
by Scope's language-neutral tests or interpreted by Scope.

Lifecycle evidence is invalidated when relevant host state changes. Editing
refinement may selectively retain candidate evidence rather than all earlier
reads. The investigation path retains its structured invocation evidence.
Message/conversation byte limits reject oversized context; they do not silently
truncate results to fit. Provider-specific presentation/representation limits can
still apply upstream; semantic JSON must be representable as `ToolValue`.

## Strategy comparison

| Strategy | Finding |
| --- | --- |
| A: suppress and stop | Existing default, unchanged control. |
| B: explicit correction | Already supported by `repeated_calls = 1`; use lifecycle-appropriate feedback. |
| C: existing evidence reminder | Existing continuation already re-presents actual call/result/state. B now points to it; no duplicate result message or generated summary needed. |

No operation is selected by Scope. Investigation proposal capabilities remain
empty; completion callback and successful read-evidence requirement remain
host-authoritative. No source refresh, write authority, retry, or altered evidence
meaning is introduced. Equivalent-but-unequal calls may still dispatch within
existing limits; Scope cannot infer domain-equivalent arguments.

## Deterministic validation

`investigation_tests.rs` covers discovery → inspect → duplicate → conclusion,
exact evidence/feedback at recovery, all host completion assessments, changed
arguments selected by the script, repeated exhaustion, malformed recovery, global
decision limits, conversation limits, and zero proposals. Existing shared tests
cover recursively reordered JSON, changed state, failed previous invocations,
authorization, evidence invalidation, source freshness, AI2 projected arguments,
editing refinement and proposal review.

Actual offline results:

- Original tree: `cargo check -p scope-ide --features agent --offline` passed.
- Original tree: `cargo test -p scope-ide --features agent --lib --offline investigation_tests`
  is blocked by the unrelated missing `host_command` argument in
  `cli/source_proposals/configured_tests.rs:407`. That fixture was not repaired.
- Disposable copy in `target/ai3-validation`, with only that unrelated test module
  excluded: `cargo test --manifest-path target/ai3-validation/Cargo.toml -p scope-ide --features agent --lib --offline agent::decision`
  passed **133** tests, including all **20** investigation tests.
- Same copy without the filter passed **525** library tests. This is qualified
  validation, not a claim that the unmodified repository test command passes.
- Original tree: `cargo test -p scope-language -p scope-types -p scope-meta --offline`
  passed, including doctests; `git diff --check` passed.
- Existing Cell duplicate-package dependency warnings were left untouched.

No Forth, Path, or Cell files were changed; no live model was run.

## Exact downstream handoff

Keep the existing investigation runner, model configuration, source, operations,
discovery, authorization, completion callback, and every other budget fixed:

```rust,ignore
// scope_ide::agent::{SemanticInvestigationOptions, EditBudgets, ...}
let mut options: SemanticInvestigationOptions = existing_options;
options.budgets.repeated_calls = 0; // control (also the default)
// In a separate otherwise identical run:
options.budgets.repeated_calls = 1; // treatment: one tolerated suppressed attempt
// Pass &options to run_semantic_investigation_with_discovery as before.
```

Do not raise decision/transport/inspection budgets to force treatment completion.
A run with no remaining budget may never receive recovery feedback; record that.
Allowing one suppressed attempt is the sole configured treatment difference.
Use the same AI3 Scope build for both arms so corrected investigation guidance is
not a confound between arms. Historical FW7 runs are contextual, not the new control.

Retain `EditRun.trace` (especially `RepeatedCall` and `NoProgressReason`),
`summary()`, `evidence`, `source_snapshot`, `conclusion`, and `outcome`. Wrap the
existing `DecisionModel::decide` to record exact input messages and returned raw
responses for **every** call, forwarding `max_tokens` unchanged. This captures
feedback actually delivered, retained evidence at that moment, malformed outputs,
and subsequent model choices. The lifecycle trace alone records response sizes,
not full raw decisions or feedback. Count actual downstream provider executions
separately; authorization attempts and suppressed calls are not provider executions.
Treat retained conversations as potentially sensitive source/evidence artifacts.

Smallest next experiment: paired control/treatment runs on the frozen `mystery`
case, with the same model/settings and fixed global budgets, then replicate on the
same frozen trial matrix if warranted. Compare delivery rate, next decision,
provider execution counts, host-supported conclusions and truthful terminal
classifications. Scripted success establishes the host contract only; it predicts
neither a live-model reasoning improvement nor correctness of a domain conclusion.
