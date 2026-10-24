# Read-only semantic investigations

## Choosing the runner

`run_semantic_edit_with_discovery` requires nonempty proposal capabilities. Removing
that guard is insufficient: inspection completion and exhausted inspection budgets
advance to `Propose`; completion there means `UnsupportedRequest`.

The generic `run_session` already permits reads followed by `complete`, but accepts
model completion without host evidence assessment and does not provide the semantic
edit runner's snapshot binding, contextual discovery, retained evidence, and
inspection/refinement lifecycle. It is not an equivalent investigation contract.
Script's `SemanticHost` / `ScriptRunner` already support explicitly selected reads
and retained run records, but are script execution APIs, not model investigations.

The additive entry point uses the same internal decision loop, semantic provider,
`AgentSession`, correction/repeated-call handling, and budgets. `CellAgent` remains
an ordinary `DecisionModel`; no Cell changes or new model protocol are needed.
Existing edit entry points keep their proposal behavior.

## Public API

Exported by `scope_ide::agent` (with the existing `agent` feature):

```rust,ignore
pub struct SemanticInvestigationOptions {
    pub operations: Vec<String>,
    pub budgets: EditBudgets,
}

pub enum InvestigationCompletion { Completed, Inconclusive, Unsupported }

pub struct InvestigationEvidence {
    pub call: ToolCall,
    pub source: ToolValue,
    pub response: ToolValue,
}

pub fn run_semantic_investigation_with_discovery<T: SourceTarget>(
    model: &mut (impl DecisionModel + ?Sized),
    controller: &mut SourceEditController,
    target: &T,
    service: &dyn scope_language::LanguageService,
    request: AgentEditRequest<'_>,
    options: &SemanticInvestigationOptions,
    discovery: &SemanticDiscoveryOptions,
    policy: impl for<'a> FnMut(
        scope_ide::tool::semantic::SemanticAuthorization<'a>
    ) -> bool,
    completion: impl FnMut(&str, &[InvestigationEvidence]) -> InvestigationCompletion,
) -> EditRun;
```

The existing request and controller types are reused for provider compatibility;
their edit-oriented names do not grant proposal authority. Internally proposal
capabilities are always empty. Only registered reads in `operations`, satisfying
selection prerequisites, are eligible. Discovery exposes those names without
authorizing them. The independent semantic policy is checked on invocation.
No general Scope write provider, apply/save operation, filesystem access, or
implicit reload is installed. Native language services remain trusted code:
`Read` describes persistent effects, not computational inactivity or a sandbox.

The model uses the unchanged `tool` / `complete` protocol. Investigation remains
in `Inspect` until termination, never `Propose` or `Validate`. At the inspection
limit the model may still conclude, but another inspection is denied by the
existing budget. Candidate evaluations retain their independent bounded evaluation
counter. Unlike editing, negative/inconclusive evaluation evidence does not force
repair: it may legitimately support an investigative conclusion.

`complete` retains its content and calls the host completion policy once:

- `Completed` maps to `EditOutcome::InvestigationCompleted` only if at least one
  retained response has successful `read_only` status.
- Missing successful evidence or `Inconclusive` maps to
  `EditOutcome::InvestigationInconclusive`.
- `Unsupported` maps to `UnsupportedRequest`.

Both investigation conclusion outcomes finish normally. Other existing terminal
classifications distinguish authorization, missing/stale prerequisites, semantic
blockers, provider/backend failures, budgets, and no progress. Malformed decision
retry exhaustion remains `NoProgress` with categorized protocol trace events.
Host assessment does not run on abnormal termination.

## Ownership and limitations

`EditRun` additionally owns `source_snapshot`, `discovered_operations`, `evidence`,
and `conclusion`. Snapshots contain identity, revision and exact text, including
available target provenance on pre-dispatch failure. Each invocation record owns
its full arguments, source identity/version/text and structured response (including
failures); later edits cannot replace these values. Existing pre/post invocation
freshness checks remain authoritative. Discovery retains revealed identities;
full metadata remains available in model context through the existing catalog.

`Completed` means host acceptance of an answer, not mathematical verification,
source modification, or universal semantic correctness. The host must inspect
actual evidence and explicit evaluation limitations; a callback that blindly
accepts prose is not a correctness check. Language-specific correctness remains
with the provider/evaluator. Existing protocol representation limits remain:
semantic JSON not representable as `ToolValue` produces a structured provider
failure, not successful evidence. Native provider observations are not added as a
second model protocol. Synchronous model/provider calls have no new preemptive
timeout guarantee. Authorized source/results may reach the configured model.

## Downstream migration

1. Replace `run_semantic_edit_with_discovery` with
   `run_semantic_investigation_with_discovery`.
2. Replace `SemanticEditOptions` with `SemanticInvestigationOptions`:
   move the old `capabilities.inspect` into `operations`, preserve `budgets`, and
   remove `propose` / `inspect_first`. There is no proposal-authority parameter.
3. Preserve the same bound target, request, controller, service, discovery options,
   `CellAgent`, and explicit semantic authorization closure.
4. Add the final host completion callback. Return `Completed` only when the answer
   satisfies host criteria using retained evidence; otherwise `Inconclusive` or
   `Unsupported`. Do not translate model confidence into evaluator correctness.
5. Read `conclusion`, `evidence`, `source_snapshot`, and `outcome`; do not expect
   `ProposalReady` or invoke proposal acceptance/save APIs.

No downstream semantic dispatcher, operation changes, planner, Forth dependency,
or live-model experiment is required.

For opt-in duplicate feedback using the existing `budgets.repeated_calls = 1`,
see [AI3 bounded repeated-call recovery](AI3_REPEATED_CALL_RECOVERY.md). The default
remains immediate `NoProgress`; suppressed calls are never redispatched.

## Deterministic demonstration

`crates/scope-ide/src/investigation_tests.rs` contains a scripted model and
language-neutral fixture: discover `inspect` and `evaluate_example`, inspect the
bound definition, evaluate disposable arithmetic, and conclude. Its host callback
checks structured observations rather than merely trusting `complete`. Tests
assert actual model/provider invocation, zero proposals, unchanged target,
independent authorization, hidden/write denial, invalid-argument recovery,
malformed/repeated decisions, budgets, missing evidence, host uncertainty,
pre-dispatch provenance, and retained evidence after a later source mutation.
