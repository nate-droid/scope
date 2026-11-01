# H4 — Language-aware authoring guidance

## Decision and audit

Reuse an ordinary downstream `LanguageService` semantic operation. No new public
interface, language registry, tool provider, purpose enum, or source model is
necessary. The missing piece was an integration recipe and consumer evidence,
not a new dispatch mechanism. The executable fixture is
[`language_guidance.rs`](../crates/scope-ide/tests/language_guidance.rs).

This decision follows inspection of these existing boundaries:

| Mechanism | What already works / limitation |
| --- | --- |
| `LanguageService::semantic_operations` | Optional discovery with stable language-local IDs, descriptions, arguments, effects and targets. Default empty catalog preserves unsupported languages. |
| `SemanticOperationRequest` | Existing document text, path, revision and optional selection support contextual answers without another document representation. |
| `SemanticToolProvider` | Fully qualified catalog names, source preconditions, language matching, scope checks, per-invocation semantic authorization, result validation and retained native observations. Discovery grants no authority. |
| `AgentSession` / ordinary model loop | Installed catalog and requested tool results reach consumers; existing message/conversation and tool budgets apply. No automatic guidance fetch is needed. |
| Configured editing | Existing inspection eligibility/discovery, exploration budgets and source-state-bound continuation evidence can carry a guide. Hosts must include it in the inspection allowlist. A guidance read is Exploration, not CandidateEvaluation. |
| A3 `semantic_feedback` | Optional continuation presentation, capped at 8192 serialized JSON bytes with an inconclusive fallback. Configured editing projects feedback without replacing retained full results or lifecycle evidence. Ordinary `run_session` retains full results **and** feedback; it does not compact them. |
| Source identity | Host-owned `SourceTarget`/snapshot and inspection bindings retain identity, language, revision and exact text. Path plus revision alone is not a global identity. Stale observations never authorize another revision. |
| Human inspection | Existing `languages`, `:scope inspect` and retained result surfaces discover and invoke argument-free document reads, with explicit confirmation and lossless native inspection. No new help UI is needed. |
| Language actions / interactive session help | Action labels alone do not provide a body or dispatch authority; evaluator sessions are separately selected state owners. Neither is a better replacement for the existing semantic read. |

Implementation references: [language contract](../crates/scope-language/src/lib.rs),
[semantic contract](../crates/scope-language/src/semantic.rs),
[provider](../crates/scope-ide/src/tool/semantic.rs),
[inspection](../crates/scope-ide/src/semantic_inspection.rs),
[configured editing](../crates/scope-ide/src/edit_lifecycle.rs),
[retained agent](../crates/scope-ide/src/cli/retained_agent.rs).
See also [architecture](architecture.md), [agent lifecycle](agent-lifecycle.md)
and [agent limits and authority](agents.md).

## Shared responsibility boundary

Scope owns catalog discovery, delivery, generic result envelopes/presentation,
message limits, source binding, invocation policy, lifecycle and evidence retention.
Downstream languages own all syntax, examples, local replacement conventions,
operation references and domain interpretation. Native adapters remain trusted
host code, not sandboxed extensions. Guidance is untrusted descriptive data, never
validation, execution permission, proposal acceptance or saving authority.

A conventional local operation ID is `authoring_guidance`; it is **not** a reserved
Scope ID or machine-enforced classification. Discover its ordinary description,
not an assumed tool that every language must implement. References are existing
language-local operation IDs; resolve them against the actual catalog and host
allowlist. A reference grants nothing and need not be invocable in the same phase.

Minimal downstream recipe:

1. Advertise one argument-free `Read` / `Document` operation with a short description
   identifying authoring guidance. Leave purpose as Exploration.
2. Return `SemanticOperationResult::ReadOnly` containing a compact JSON object with
   orientation, expression examples, local editing semantics and operation IDs.
3. Use only the supplied document and optional selection for context. A general
   document guide works through human inspection and retained scratch agents.
   Selection-required contextual guides can use the same operation model, but
   are not eligible for the current argument-free document inspection surface.
4. Optionally project the essential fields using `semantic_feedback` for configured
   editing. Do not drop syntax examples from that projection. Return no success or
   proof claim merely because a guide was delivered.
5. Test examples against the downstream parser/checker; Scope cannot establish
   their linguistic or mathematical accuracy.

### Size, request boundary and retention

Request the guide explicitly before first authoring in an unfamiliar language or
when a changed target makes contextual guidance stale. Do not insert a language
manual into every system prompt, catalog entry or continuation. Hosts can instruct
an experiment to fetch the discoverable guide once; this does not grant permission
to invoke it. Missing guidance means the previous catalog/workflow remains intact,
not a fabricated generic syntax answer.

Keep the full guide small, preferably a few KiB including JSON escaping, and its
catalog description short. This is a downstream content recommendation, not a new
Scope hard limit. The existing 8192-byte feedback cap applies only to the optional
projection, **not** full native evidence. Existing `AgentLimits` enforce message
and total conversation budgets (defaults 64 KiB/message and 256 KiB total).
Oversized messages stop rather than truncate; smaller configured budgets can reject
otherwise compact guidance. The regression fixture deliberately includes a larger
reference body to test that full evidence survives and limits are not bypassed.
Production guides should omit such padding.

Ordinary sessions retain the result in their bounded conversation. Configured edit
continuations retain source-state-bound observations, deduplicate identical calls
at the same state and invalidate evidence when relevant state changes. Their
compact feedback may therefore recur in subsequent continuations; this reuse does
not add a once-only eviction guarantee. Keep it concise. There is no new cache,
background refresh, transcript persistence, retention store or universal source-free
guidance API. Full host evidence remains separate from continuation presentation.

## Actual model-facing example

The Calc test service supplies this essential content through the ordinary result
(and its optional feedback projection):

```json
{
  "orientation": "Calc uses let bindings and integer arithmetic; no evaluation is needed to author source.",
  "examples": [
    {"source": "let answer = 6 * 7", "explanation": "Bind an arithmetic expression."}
  ],
  "local_replacement": {
    "operation_id": "replace_expression",
    "instruction": "Replace only the selected expression, preserving the binding."
  }
}
```

The deterministic model discovers `language.example.calc.authoring_guidance`,
requests it and asserts receipt of orientation, source example and editing reference
before completing. This establishes delivery, not live-model improvement. The Calc
fixture defines the example convention; it is not a production parser or evaluator.
No editing operation is invoked, proposal accepted or source changed.

The same fixture uses `semantic_inspection::discover` and
`InspectionBinding::capture` / `invoke` against a real `Document`. A downstream
launcher that registers the service can use the existing shell sequence:

```text
:scope languages
:scope inspect
:scope inspect operation authoring_guidance
```

Then confirm the token printed by the capture, and use `:scope inspect result
--full` for native details. This targets the App document opened by bare `open`,
not a separately selected shell buffer. Capturing/discovering does not execute;
confirmation grants only that captured human invocation, not agent permission.

## Path compatibility and next experiment

Read-only inspection of the sibling Path checkout found A4 evidence at
`../path/live-evidence/path-a4-20261102/REPORT.md`. Its live candidate used `@`,
received `Parsing: UnsupportedConstruct`, repeated the candidate and stopped
`NoProgress`. That run did not demonstrate a completed proof, a revised candidate
or live receipt of A4's improved TypeMismatch projection. Deterministic diagnostic
replay and live evidence must remain distinct.

Path can add this one operation in its own Scope adapter, using its current
`docs/language.md` and downstream parser/checker tests as authority. The sibling
checkout also contains an untracked draft `docs/agent-syntax.md` and untracked
`crates/path-syntax/tests/agent_syntax.rs`; these were inspected, not changed,
executed or assumed to be committed deliverables here.

Its concise guide should explain, without theorem-specific examples:

- Supported expression grammar, left-associated juxtaposition application,
  parentheses for compound atomic operands, annotated lambda/binder syntax and
  unsupported arrow/multi-name binder shorthand. Explicitly distinguish unsupported
  `@` notation from application.
- Identity types and the actual identity eliminator form, including motive scope,
  universe and base/endpoint obligations, using generic checked examples.
- Primitive constructs versus document-defined names: operations such as `inv`,
  `ap`, `concat` and `transport` are not assumed built-ins. Existing source
  inspection supplies exact signatures and explicit argument order.
- Hole-local replacement: use the captured local context; do not repeat an enclosing
  binder or shadow an active local. Introduce a fresh lambda only when appropriate
  for the expected function type.
- References to its existing semantic inspection and disposable candidate evaluator,
  copied from its actual operation catalog. Candidate evidence installs no definitions
  and does not accept an edit.

No Path kernel, language semantics, proof search, Cell code or Scope domain logic
needs changing. Path's small adapter and example-validation tests should be a
separate downstream change. This milestone does not implement or verify that change.

Recommended live follow-up: keep the A4 model, target, objective and finite budgets;
add only the discoverable syntax guide to the host's read allowlist and ask the
model to read it before authoring. Retain exact source binding and full candidate
results. Measure guide discovery/receipt, unsupported syntax attempts, repeated
unchanged candidates, candidate evaluation outcomes and stop reasons against A4.
Do not supply pointwise-uniqueness proof steps or theorem-specific hints. Run only
with explicit provider/privacy approval; no paid/live calls are needed for the
regression tests here.

## Verification and publication assurance

This change demonstrates existing interfaces with tests and documentation only;
it changes no public dispatch contract. Direct checks:

- `cargo test --locked -p scope-ide --test language_guidance`: 4 passed.
- `cargo test --locked -p scope-ide --features agent --test language_guidance`: 6 passed.
- `cargo fmt --all -- --check`: passed.
- `cargo test --locked -p scope-ide --features agent --lib tool::semantic::tests`:
  22 adjacent provider tests passed.
- `cargo test --locked -p scope-ide --features agent --lib continuation_tests`:
  5 adjacent continuation tests passed.
- `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`:
  5 passed.
- `python3 -B .github/scripts/check_markdown_links.py`: failed with 35 existing
  missing-target occurrences outside the new guide/link. These unrelated missing
  documents were not recreated or links removed to make this check pass.

Coverage includes agent and human discovery/delivery, absent guidance, language
mismatch, stale preconditions/capture, scope and authorization rejection, argument
validation, unchanged source/no proposals, native evidence retention, feedback
fallback, tool budgets and oversized-message rejection before the next model call.
No live model or downstream Path parser/checker was run.

### Separate outstanding A3 Full obligations

The public A3 feedback hook still requires successful Full assurance under the
[verification policy](verification.md). Historical evidence in sibling Path's
`live-evidence/path-a4-20261102/scope-verification/SUMMARY.md` records 18 attempted
commands: 9 passed and 9 failed/blocked. This is evidence for the then-tested Scope
revision plus dirty A3 changes, **not** current-HEAD Full validation.

Outstanding categories: all-feature tests (launcher permission failure and unreached
later targets), TUI launcher prompt failure, all-feature Clippy, no-default-feature
Clippy, standalone downstream/harness tests and harness run (resolution/network),
Markdown links (35 missing-target occurrences), and scaffold/downstream closure
(nested lockfile-write restriction). Retain the exact historical commands/logs;
fix or explain each on the current revision, then complete the policy's Full matrix,
standalone/scaffold checks and platform CI before claiming publication readiness.
Focused H4 passes do not close those failures, replace PTYs with mocks, or waive
public-contract assurance. No broad workspace/PTY repetition is needed to establish
this test-only reuse demonstration; publication assurance remains unresolved.
