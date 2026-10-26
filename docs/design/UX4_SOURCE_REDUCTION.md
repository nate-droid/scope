# UX4 — Source reduction feasibility

## Finding

**Yes, within a deliberately small whole-program boundary.** The experiment in
`crates/scope-script/src/reduction.rs` folds consecutive decimal-integer
assignments followed by a final binding read. It does not implement `:compact`,
a proposal adapter, an optimizer framework, or a language extension.

Example (supported Script syntax):

```scope
let mut n = 6;
n = 8;
n = 12;
n
```

Candidate (blank statements intentionally preserve original separators/trivia):

```scope
let mut n = 12;
;
;
n
```

This is source reduction, not pretty-printing. Keeping `mut` avoids unnecessary
byte changes and preserves the binding's mutability for inspection. The prompt's
`n * 7` syntax is valid but outside the executable rule. Do not interpret refusal
as a claim that a particular program cannot be simplified.

## Semantic inventory

Grounding: `scope-script/src/parser.rs`, `lib.rs` (`statements`,
`expression_inner`, `user_call`, `check_bindings`, `Budget`), `admission.rs`,
`workbench.rs`, `runner.rs`, and `semantic.rs`.

- Parsing precedes execution. Semicolons and newlines separate statements;
  `#` introduces line comments. AST expressions have UTF-8 byte start offsets,
  not complete end ranges or attached trivia. The AST is not a lossless syntax
  tree. Source is limited to 65,536 bytes and nesting is bounded.
- `let` evaluates and binds a value; its statement result is that value, even
  with a semicolon. `let mut` additionally registers the name as mutable.
  Assignment requires an existing mutable name **before evaluating its RHS**,
  then requires the same outer runtime `Value` variant. Heterogeneous arrays
  remain possible. Assignment's statement result is null, not its RHS.
- Every statement replaces the current result; the last statement determines
  the Script result. Empty programs, assignments, functions, conditionals and
  loops yield null. A semicolon does not discard a final expression's value.
- Binding preflight reserves declarations for each lexical scope and rejects
  duplicate names and shadowing of outer names, including outer declarations
  textually later in the program. Sibling branches may independently declare
  identical local names; those locals are cleaned up on success or failure.
  Loops cannot shadow an outer name; loop locals are fresh each iteration.
  Outer mutable assignments in branches/loops survive cleanup.
- Functions are top-level only, snapshot all current bindings at declaration,
  and are removed after each eval. Captures and parameters are immutable;
  calls isolate locals and do not write captures back. Parameters cannot
  shadow reserved outer declarations. Recursion and excessive call depth fail.
- Expression evaluation is ordered: binary left before right, base before
  index, array/object entries in source order, call arguments left to right.
  Logical operators short-circuit. Function arity/recursion checks can fail
  before arguments run. Built-in names/signatures are parser-checked; unknown
  user calls fail at runtime before argument evaluation.
- Arithmetic is checked; overflow, division by zero, missing names/fields,
  indexing, wrong types and helper conversion errors can fail. Built-in text,
  collection and numeric helpers are not host effects but are not universally
  total. All expression evaluations debit steps/work; bindings, captures and
  printed values consume retained-value budget. Limits are observable errors.
- `print` appends a typed value to ordered retained output and yields null.
  Runtime failure leaves earlier prints/host effects and successful bindings;
  parsing and binding-preflight failures execute no statements.
- `scope.operations` calls the host; `scope.invoke` evaluates provider, action
  and argument object then invokes the host. Neither is classified safe here.
  `SemanticHost` enforces installed read-operation bindings and authorization,
  captured source/version preconditions and a call limit. Native hosts remain
  trusted synchronous code. Reductions must not bypass authorization.
- Scratch workbench runs use fresh interpreters and owned revision-bearing
  snapshots. The standalone `repl.rs` still has persistent interpreter state:
  **this reducer's contract is not for that API**. Assignment fragments can
  accumulate in scratch if their complete concatenation parses, passes binding
  checks and executes from its declarations. They are not independent actions:
  order, types, initial state and failures remain significant. Admission's
  pure/effectful classification is not a proof of totality or budget safety.

## Equivalence contract

For a complete snapshot evaluated in a **fresh** interpreter, preserve final typed
value (or error kind/message), ordered typed prints, termination/failure behavior,
and the authorized host-operation trace including order and arguments. Preserve
binding/scope interpretation and evaluation ordering relevant to those observations.
The supported rule additionally preserves final binding value and mutability.
This is not contextual equivalence under arbitrary code appended later, arbitrary
preloaded interpreter state, or resource-budget instrumentation.

Source identity of a run, source revision, raw diagnostic offsets/line numbers,
formatting, timing and internal budget counters are provenance, not unchanged
runtime observations. Diagnostics must retain failure meaning; a future rule that
can preserve failures needs source attribution rather than equal raw offsets.
The implemented rule permits **no failing executions**, so diagnostic equivalence
is vacuous, not empirically established for a failure-producing rewrite.

Removing pure work can move a later operation from budget failure to success.
Consequently purity alone is insufficient. Do not generalize the local rule to
an arbitrary suffix or repeat it within loops.

## Reduction matrix

Each row considers the complete program, not just matching names in text.

| Case / example | Proposed output | Conditions and counterexample | Established now? |
|---|---|---|---|
| A: `let mut n=6; n=8; n=12; n` | `let mut n=12; n` (trivia retained) | All RHS decimal integers, adjacent writes, sole binding, final read, bounded whole program. `n=true; n=12` instead fails a kind check. | Yes, executable rule. |
| A: same chain ending `n*7` | Mathematically `let n=12; n*7` | Must prove suffix/budget/failure behavior and no later mutation before dropping `mut`. | Conditional; refused. |
| B: `let mut n=6; let previous=n; n=8; previous+n` | None | Replacing initial 6 by 8 changes 14 to 16. | Intermediate statements refused. |
| C: `let mut n=6; print(n); n=8; print(n)` | None | Must preserve ordered `[6,8]`, not `[8,8]` or `[8]`. | Calls refused. |
| D: `let mut n=6; n=1/0; n=8; n` | None | Overwritten RHS still fails. Overflow, index and type errors likewise matter. | Nonliteral RHS refused. |
| E: `let mut n=6; if false { n=8 }; n` | None | Unconditional substitution changes 6 to 8; even constant-branch removal changes budget use. | Branches refused, no control-flow analysis. |
| F: `let mut n=6; if true { let n=8 }; n` | None | Shadowing is a preflight error, not a second local binding. Sibling branch locals can share names without sharing identity. | Nested scopes refused. |
| G: `let mut n=6; fn old() { n }; n=8; old()` | None | Captured value is 6; changing initializer changes capture. Loop reads/writes repeat and consume shared budgets. | Functions and loops refused. |
| H: `1; 42` | Potentially `42` | First value is discarded, but whole-program budget must be bounded. `1/0; 42` fails; a final `1;` yields 1 and is not discarded. | All redundant-expression elimination deferred. |

No calls are eliminated, including seemingly harmless built-ins. No reordering,
control-flow, interprocedural analysis, constant evaluation or general effect
inference is introduced.

## Exact executable boundary and justification

`reduction::reduce(&runner::Snapshot)` returns `Result<Candidate, Refusal>`.
The candidate owns the baseline identity, revision and exact text, replacement
byte ranges/text, resulting source, rule name and checked preconditions. Refusal
is explicit (`InvalidSource` or `Unsupported`); there is no execution, host,
filesystem or mutable scratch argument.

Accepted programs have 3–64 top-level statements and at most 4096 UTF-8 bytes:
exactly one mutable integer declaration, one or more same-name integer literal
assignments, and an unbound final name expression reading that binding. Decimal
source ranges are recognized only after AST literal classification; the candidate
is reparsed to guard against missing AST end spans (e.g. parenthesized assignment
RHS). Negative literals (unary AST), arbitrary expressions and all other statements
are refused. Existing parser bounds and literal overflow checking apply.

Local argument: preflight sees one valid declaration. All literal evaluations
terminate and cost one step/work unit; all RHS kinds are integer. One binding
retains at most source-name length plus one value node. At most 64 expression
steps/work units and well below 1,000,000 retained nodes/bytes are needed, versus
100,000 step and 1,000,000 work limits. No output, captures or hosts consume budget.
The final read sees the last assigned integer in both programs. Thus both succeed
with that integer, no prints and no operation trace. Deleting writes and replacing
the initializer does not change lexical structure. This argument depends on the
current evaluator's scalar accounting and must be revisited if semantics change.

Unaffected source bytes are retained, including comments and separators. This
avoids a replacement parser or formatter. A second reduction refuses because the
candidate has no assignment: termination after one successful proposal, not an
endless sequence of cosmetic rewrites.

## Validation and limits

`cargo test -p scope-script --lib`: **62 passed, 0 failed**, including four new
reduction tests. The subsequent full `cargo test -p scope-script` also passed
(unit, integration and doc-test targets). `rustfmt --edition 2024` was run on the
new module. Table-generated chains compare 16 combinations including zero
and `i64::MAX` using separate fresh interpreters with `NoHost`. Tests assert typed
final values and ordered output, exact Unicode-aware ranges/trivia, baseline text
retention and one-step termination. Misleading comments, parenthesized RHS,
intermediate reads, prints, failures, kind changes, branches, shadowing, loops,
functions, host discovery, final assignments and arbitrary suffixes are refused.
Independent executions demonstrate changed intermediate-read results, ordered
prints, capture-by-value and runtime/preflight errors in counterexamples.

Dynamic execution is supporting evidence, not the soundness argument. Accepted
programs cannot print or fail; no positive printing/failing reduction is claimed.
No host is invoked in reduction tests or analysis; operation preservation follows
from the accepted grammar excluding all calls. No live UI/PTY, filesystem edit,
cross-project change or production compaction workflow was tested.

## Proposal integration assessment

Existing `scope-ide/src/source_edit.rs` provides `SourceSnapshot`, tagged edit
`SourceId`, `SourceTarget`, and `SourceEditController`; `proposal.rs` provides
`SourceEditProposal`, preview/diff, explicit accept/reject/discard and status.
`ScratchBuffer` already implements the target contract. Preview/accept check target
identity, expected memory version and exact original text, plus edit ranges.
Acceptance applies one committed-memory replacement/revision, without save or run.
Stale candidates must be regenerated, not silently rebased or retargeted to the
currently selected scratch. Disk revisions and scratch memory versions differ.

A future adapter should capture the owner-local **edit identity** alongside the
snapshot's **evaluation identity**, register the exact candidate/ranges against
that baseline, retain rule/preconditions as review evidence, display the diff,
and accept only via explicit human dispatch. Even identical-text edits advance
scratch versions; selection and semantic-target binding must not retarget proposals.
Existing draft guards and retained controller owner/lifetime rules still apply.
See `docs/RETAINED_SOURCE_PROPOSALS.md` and `docs/CONTRACTED_WORKSPACE_EDITING.md`.

`ValidationEvidence` binds semantic operations/contracts to exact source/candidate
and observations; a reducer's checked preconditions are **not** that evidence.
Evidence setters are crate-private and configured controllers may require trusted
semantic validation. Do not invent an operation invocation or claim a successful
NoHost run satisfies an equivalence contract. `PureRunRecord`/`RunRecord` retain
execution snapshots/results but do not automatically confer proposal validation.

Minimum missing integration: an adapter binding the returned evaluation snapshot
to its original scratch edit identity/version, with retained local-rule evidence
and an explicit assurance policy (unevaluated proposal versus existing trusted
validation path). Human-viewed status is not tracked by core Pending status.
No new edit engine, scratch owner or cross-project abstraction is justified here;
this milestone stops before the adapter and UI.

## Recommendation

Proceed only with a demand-driven, review-only prototype of this narrow idea.
The motivating arithmetic suffix and readable `let n=12` presentation are not yet
supported by this experiment. Prioritize user evidence that these reductions are
useful before widening the rule. Any expansion must earn its budget, failure,
binding and effect guarantees separately; never substitute final-value spot checks
for observational equivalence. The architecture is suitable for proposals, but
not automatic rewriting or production `:compact` on the current evidence.
