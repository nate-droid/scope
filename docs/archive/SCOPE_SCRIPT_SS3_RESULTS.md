# Scope Script SS3 — Conditional structured orchestration

## Implementation and SS2 baseline

SS2 was inspected in `src/runner.rs`, its results document, and shell tests before implementation. Its actual integration is an opt-in `ReadOnlyCommand::invoke_source` callback, not a universal downstream Script environment. `ScriptRunner` installs one explicit semantic read, snapshots separate script/target buffers, constructs a fresh interpreter per full-buffer execution, and preserves catalog, full envelopes, prints, and interpreter outcome in `RunRecord`. The standalone REPL instead keeps its interpreter bindings. None of those ownership or authority boundaries changed.

SS3 adds statement-level conditionals, branch-local immutable bindings, focused interpreter/host/shell regressions, and a runnable declaration-language example. No host privilege, dependency, Cell capability, or Script-specific type system was added.

## Exact syntax and evaluation

```text
if BOOLEAN_EXPRESSION {
    STATEMENTS
} else {
    STATEMENTS
}
```

`else` is optional. Blocks nest; statements use newlines or semicolons. `else` may follow its closing brace on the same line or after newlines, but not after a semicolon. Opening braces follow the condition or `else` on the same logical line. Parenthesized conditions may span lines. There is no `else if` shorthand: put another `if` inside `else { ... }`. Empty blocks are valid. Conditionals are statements, not expressions, and return `null`; a submission ending in a conditional therefore returns `null`.

Conditions evaluate once and must be `ToolValue::Boolean`; integers, null, strings, arrays, and objects are not truthy. A nonboolean condition is a runtime error at the condition's original byte offset. Only the selected branch executes. Unselected branches perform no discovery, invocation, prints, binding initialization, or expression work.

The complete source is lexed and parsed before evaluation, including unselected branches. Invalid syntax in any branch prevents all effects. Duplicate-binding validation also runs over the whole tree before effects. Runtime-only errors such as missing fields or unknown bindings in skipped branches do not execute and do not fail the run.

## Lexical scope

Every selected branch introduces a lexical scope. Outer bindings remain readable. Local immutable `let` bindings disappear on successful exit and on failure. Nested branches behave identically. Sibling branches may use the same local names.

Shadowing is forbidden: a branch cannot declare any name declared in an enclosing scope, including an enclosing declaration later in the submission. Duplicate declarations within a scope, rebinding persistent REPL bindings, and binding builtin/keyword names are errors before effects. This deliberately conservative policy does not introduce forward references: reading a later declaration still fails at runtime. Branch-local names can be reused by a later independent evaluation.

Runtime failure retains earlier successful top-level bindings, ordered prints, and completed host effects; it does not roll back. Branch locals are cleaned up even on error. SS2 continues capturing prior prints and full semantic envelopes independently of the final interpreter result. No reassignment, mutable environment, or exception handling was introduced.

## Parser, attribution, and budgets

`parser.rs` now represents expression/binding statements and conditional statements separately. Recursive block parsing consumes newline/semicolon separators. Lexing preserves newline tokens so block statements are not accidentally joined; expression delimiter groups continue accepting multiline calls, arrays, objects, and parenthesized expressions. Semicolons are not silently treated as expression whitespace.

Original token byte offsets are retained when expression-internal newlines are removed. Errors in selected branches point to their original source expression or declaration. SS2's existing byte-to-line/Unicode-column rendering is unchanged. Source and target snapshots, identities, revisions, selection, operation identity, and upstream metadata remain host-owned.

The existing 65,536-byte source cap and 64-level delimiter/expression limits still apply, with an explicit statement-nesting guard. A single expression-step/work budget is shared by the entire evaluation, including nested selected branches. Conditions consume ordinary expression steps/work; skipped bodies consume none. Branch values count toward retained-memory limits while live; cleanup releases local binding retention without discarding prints. No loop can multiply execution. Tests exercise depth, selected work exhaustion, skipped work, and a directly bounded shared step budget.

## Authorization, types, and outcome layers

No invocation path was changed. Parsing, a true condition, discovery, or a matching field shape grants no authority. Selected invocations still pass through existing host policy, operation contracts, source identity/revision preconditions, and read-effect restrictions. Unselected branches never enter providers. Agent/model calls, editing, processes, and networking are not new builtins.

Values remain the existing `ToolValue`: booleans, integers, strings, null, arrays, and recursive objects are not flattened. Projection and indexing remain strict. Operation validation and nominal type/contract decisions remain upstream; Script adds no unions, structural compatibility engine, automatic adapter, or nominal conversion. Missing type evidence remains unknown. Matching fields are only values to inspect, not compatibility evidence. Contract tests continue preserving operation/adapter/provenance reports without turning them into authorization.

Outcome layers remain distinct:

- `RunRecord.outcome` describes interpreter execution, not semantic acceptance.
- Host invocation can fail with its existing authorization/tool/operation error; this aborts evaluation and remains an error, not a manufactured semantic envelope.
- A successfully dispatched operation may return a rejection, partial, unsupported, unknown, or other operation-specific semantic envelope. It remains an ordinary structured value.
- Scripts inspect only documented fields and may print their own ordinary objects. There is no universal status field or outcome schema. Missing fields fail, rather than silently meaning false or compatible.

The declaration fixture's `read_only` status specifically describes its read-only description result; it is not proof completion or type acceptance. Its payload explicitly retains partial information, unknown inferred type, and unsupported references.

## Runnable demonstration and transcript

The deterministic integration uses the existing registered declaration language, not a fabricated Path API. The example host installs `language.example.declarations.describe_selection` under provider `semantic`, binds `example.decl` and its first declaration selection, and requires explicit `--allow-read`.

```sh
cargo run -p scope-script --example workspace_semantic --locked --offline -- --allow-read crates/scope-script/examples/semantic-workspace crates/scope-script/examples/conditional-declaration.scope
```

`conditional-declaration.scope` discovers that explicitly installed operation, captures its full structured result, checks `result.status == "read_only"`, checks `result.value.kind == "integer_literal_binding"`, and prints a report with the operation descriptor and untouched evaluation envelope. Its final expression returns the original result.

Observed output begins:

```text
String("answer")
String("42")
Object({"evaluation": Object({"operation_id": String("describe_selection"), ...}), "operation": Object({...}), "outcome": String("integer_literal_binding")})
```

The last line above is abbreviated for readability; actual output includes the full semantic envelope and catalog descriptor, including schemas and constraints. The executable shell regression asserts the report's `evaluation` equals the captured original envelope.

For interactive SS2 authoring:

```sh
cargo run -p scope-script --example scratch_runner --locked --offline -- crates/scope-script/examples/semantic-workspace --allow-read --terminal
```

```text
open example.decl
:scope scratch new scope-script
:scope edit
let result = scope.invoke("semantic", "language.example.declarations.describe_selection", {})
if result.status == "read_only" {
    print({outcome: result.value.kind, evaluation: result})
} else {
    print({outcome: "not_read_only", evaluation: result})
}
:done
:scope run language.example.declarations.describe_selection --selection 0:0..0:15
quit
```

The shell shows original semantic envelope, structured print, `final: Null`, and `status: interpreter success (see semantic envelopes for semantic status)`. Selection coordinates refer to the target, not the Script. The PTY regression authors the complete multiline example through this existing editor, runs it, checks all three prints and unchanged envelopes, and verifies unchanged workspace files.

## Validation

- `cargo test -p scope-script -p scope-ide -p scope-terminal --all-targets --locked --offline` — passed after core and integration changes.
- `cargo test -p scope-script --all-targets --locked --offline` — passed after adding structured-report assertions.
- `cargo build -p scope-script --example scratch_runner --offline --locked` — passed.
- Runnable `workspace_semantic` command above — passed, producing the observed structured report.
- `python3 crates/scope-script/tests/pty_scratch_runner.py -v` — all 3 actual-terminal tests passed, including multiline conditional authoring/reruns. An initial stale print-count assertion and an indentation error introduced while updating it were corrected before this passing run.
- `cargo fmt --all -- --check` and `git diff --check` — passed.

Focused coverage includes basic/optional/nested branches, boolean-only conditions, lazy host effects, lexical cleanup, outer reads, duplicate/shadowing policy, complete parse before effects, skipped syntax errors, budgets, offsets, prior print retention, persistent REPL behavior, selected authorization/denial, semantic failure values, strict missing fields, upstream contract/provenance preservation, and multiline SS2 cooked/PTY editing. Existing revision/source and nominal-contract regression suites remain unchanged and pass. Cargo emits existing Cell checkout warnings; the example also has an unused fixture constant warning.

## Limitations and SS4 recommendations

No live downstream Path integration was available or claimed. This fixture demonstrates conditional structured orchestration, not semantic proof acceptance. Bindings are dynamically represented `ToolValue`s; absent fields cannot currently be safely probed without an upstream documented discriminator. Authorization/tool failures cannot be caught inside Script. Richer outcome handling should begin with a concrete downstream contract, not unions, pattern matching, or a second type algebra.

Execution remains synchronous; native host calls have no new cancellation/timeout mechanism. SS2 records are host-owned and unbounded across runs unless drained. Full-buffer runs remain fresh; standalone REPL runs remain persistent. Completion remains metadata/binding based and does not infer branch-local draft bindings. There are no loops, user functions, imports, mutation, rollback, generic types, automatic adapters, or new editor architecture.

For SS4, install an actual Path operation with explicit source binding and authority; document its own acceptance/rejection/unknown contracts and preserve original envelopes. Build a bounded, fixed-size baseline/candidate comparison first. Keep compatibility evidence, adapter correctness, and execution authorization independent. Add bounded record retention and downstream freshness/PTY coverage before asynchronous orchestration or any proposal for iteration.
