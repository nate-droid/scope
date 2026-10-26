# Scope Script

Scope Script is an embeddable orchestration language. Use it in three distinct
ways:

| Entry point | Source and state | Host access |
| --- | --- | --- |
| Default workbench, `:scope script run` | Selected scratch; fresh interpreter per run | `NoHost` |
| Standalone CLI, `cargo run --locked -p scope-script -- FILE` | Saved UTF-8 file; fresh interpreter per run | `NoHost` |
| Standalone REPL, `cargo run --locked -p scope-script` | Bindings retained within the REPL | `NoHost` |

Embedded hosts can explicitly install adapters and authorize operations; the
stock entry points do not do that automatically. Root `cargo run --locked`
selects package `scope-workbench`, binary `scope`, composing `scope-ide` and
`scope-script`. For a guided editing loop, see the [Scope tutorial](../TUTORIAL.md).

## First useful programs

From this checkout, run a complete first program:

```sh
cargo run --locked -p scope-script -- crates/scope-script/examples/hello.scope
cargo run --locked -p scope-script -- crates/scope-script/examples/checklist.scope
```

The first prints `{"total": 42}` (human formatting may expand records). The second
turns comma-separated task names into a checklist summary:

```text
# Edit the input and run again.
let tasks = split("build,test,review", ",")
let report = {count: len(tasks), tasks: join(tasks, " → ")}
if contains(tasks, "test") {
    print(report)
} else {
    print({warning: "Add a test task", report: report})
}
```

Save this text as `tasks.scope` anywhere; no project manifest is needed. For an
installed executable, run `cargo install --path crates/scope-script --locked`
from the checkout once, then use `scope-script tasks.scope` from any directory.
Installation uses Cargo's normal installation directory; it grants scripts no
filesystem access. Use `scope-script --help` for output options.

To run the **same source** in the workbench, start `cargo run --locked`, enter
`:scope scratch new script`, then `:scope edit --commands`, paste the source,
enter `:done`, and run `:scope script run`. Edit and rerun: each run starts fresh.
The REPL instead retains bindings and evaluates one submitted line at a time;
do not pipe multiline files into it. REPL evaluation errors are displayed but do
not make the REPL process exit nonzero; file failures do.

### Small pure standard library

| Function | Contract |
| --- | --- |
| `len(value)` | Array item count, record field count, or string Unicode scalar count (not bytes or graphemes). |
| `contains(value, item)` | Array structural membership, record string-key membership, or string substring; no coercion. |
| `keys(record)` | String keys in deterministic sorted order. |
| `join(strings, separator)` | Join an array of strings; separator must be a string. |
| `split(text, separator)` | Split strings, retaining trailing empty parts; an empty separator yields leading/trailing empty strings and individual Unicode scalars. |
| `parse_int(text)` | Parse a signed 64-bit decimal integer into an `ok`-tagged result; invalid text is recoverable data. See S12 below. |
| `starts_with(text, prefix)`, `ends_with(text, suffix)` | Case-sensitive literal string boundary tests, including empty patterns. |
| `trim(text)` | Remove Unicode whitespace at both ends. |
| `lower(text)`, `upper(text)` | Full-string Unicode casing, not locale-specific casing; may expand characters. |
| `replace(text, from, to)` | Replace all non-overlapping literal matches; empty `from` inserts at UTF-8 character boundaries, including both ends. No regex. |
| `append(array, value)` | Return a new array with one value added; does not flatten or mutate. |
| `reverse(array)` | Return a new array in reverse order; not a string operation. |
| `slice(array, start, end)` | Half-open indices: `{ok: true, value: array}` when `0 <= start <= end <= len(array)`; otherwise `{ok: false, error: {code: "invalid_index", message: "expected 0 <= start <= end <= array length"}}`. No clipping or negative indices. |
| `get(array, index)`, `get(record, key)` | Integer array index or string record key. Found: `{ok: true, value: value}` (including null); absent/negative/out-of-range: `{ok: false, error: {code: "missing_value", message: "requested value is missing"}}`. Wrong collection/key types are fatal. |
| `range(start, end)` | Integer array from inclusive start to exclusive end, step one; empty when end <= start. Large allocations hit shared limits. |
| `abs(integer)` | Checked absolute value; minimum signed 64-bit integer overflows fatally. |
| `min(a, b)`, `max(a, b)` | Exactly two signed 64-bit integers; not array reductions. |
| `format(value)` | String from the bounded human value formatter (default depth 8, with truncation possible), not a template engine, lossless serializer, or JSON guarantee. |

These are pure functions over existing values, not semantic operations. They
share interpreter work/value budgets and require no capabilities. Calls have
fixed arities; wrong types fail at runtime with source positions. Helper names
can still be bindings for compatibility, but calls such as `len(...)` always
refer to the builtin, not a user-defined function. `#` starts a line comment
outside strings. `+` remains checked integer addition; use `join` for strings.

**Baseline and gap rationale.** The S10-S12 baseline already had structured
values, field access, branches, functions/loops, `print`, the six helpers above
through `parse_int`, and explicit host calls. It could inspect and reduce values,
but normalizing labels, building collections, safe dynamic lookup, and numeric
summaries required awkward manual work or were unavailable. These flat builtins
fill those pure-value gaps without adding namespaces, methods, coercions, or a
second semantic layer. S12 is specifically the `parse_int` result contract, not
an exception system; the S10-S13 experiment below composes it with safe `get`.

Executable pure examples (CLI or unchanged source in a Script scratch):

| Example | Task |
| --- | --- |
| [Text processing](../crates/scope-script/examples/stdlib-text.scope) | Normalize incoming document labels and build a publication checklist. |
| [Collection analysis](../crates/scope-script/examples/stdlib-collections.scope) | Functions and loops reduce bounded deviations, select recent samples, and enumerate days. |
| [Experiment summary](../crates/scope-script/examples/stdlib-experiment.scope) | Summarize S10-S13 measurements, branching separately on missing fields and invalid integers. |

Run `cargo run --locked -p scope-script -- crates/scope-script/examples/stdlib-text.scope`
(or either other path). `cargo test --locked -p scope-script --test stdlib`
compares repeated CLI and public scratch runs, including recoverable results and
fatal failures. Pure results do not discover or authorize operations, validate a
downstream artifact, or establish semantic acceptance. `scope.invoke` envelopes
and the outer CLI execution record remain distinct from `ok`-tagged helper values.
All calls retain shared budgets; there is no regex, sorting, generic map/filter,
string slicing, floating-point math, filesystem/process access, or error catching.

Try the intentionally failing example:

```sh
cargo run --locked -p scope-script -- crates/scope-script/examples/expected-failure.scope
```

It prints a progress message, then exits nonzero because `join` requires string
items. The diagnostic names the source path, line, column, and byte offset.
Syntax errors execute nothing; runtime errors preserve prior output. To inspect
record fields safely, test `contains(record, "field")` before field access.
This is not exception handling: authorization errors still stop evaluation.

## Local computation: loops and functions

```text
fn double(x) {
    x * 2
}
let mut total = 0
for item in [2, 4, 6] {
    total = total + double(item)
}
print(total)
```

This prints `24`. These constructs use the existing evaluator and runtime value
checks, not a separate interpreter or static inference system.

- `for name in list { ... }` evaluates its list once and visits items in order.
  Each iteration has fresh locals; nested loops work. Loop variables are immutable.
- `let` stays immutable; `let mut` permits direct assignment to that binding.
  Assignment must preserve its runtime kind (integer, string, boolean, null,
  array, or object). Lists retain existing heterogeneous element semantics;
  assignment does not freeze array element kinds or record shapes. There is no
  indexed/field assignment.
- Names cannot shadow visible bindings, including parameters and loop variables.
  Duplicate declarations are rejected before effects, even in unselected branches.
- Functions are top-level named declarations, available after declaration in the
  current evaluation. They are not first-class values. Functions snapshot visible
  values at declaration; captures and parameters are immutable. Only function-local
  mutable bindings can be assigned inside a function; caller state is not mutated.
  Calls to other named functions resolve declarations already executed in this run.
- A function must end with an unbound expression (with or without a semicolon)
  and returns that expression's value. Empty bodies and bodies ending in a
  declaration, assignment, `if`, or `for` are syntax errors. Use final `null`
  for a procedure. There is no explicit `return`. To choose a result
  conditionally, assign a function-local mutable binding in branches, then use
  that binding as the final expression.
- Parameters/results have the same runtime checking as ordinary expressions:
  no annotations, coercions, inferred signatures, or call-site specialization.
  `double("text")` fails at `*` in the body. Structured values pass unchanged;
  field access and builtins enforce their existing contracts. Arity errors name
  the function and expected/actual counts.
- Direct and indirect recursion are rejected. Nonrecursive call depth is capped
  at 64. Expressions, calls, and each loop iteration share the existing 100,000
  step and 1,000,000 work budgets. Retained values/output are bounded too,
  including function captures and active loop lists. Errors retain body source
  byte offsets; prior prints and host effects are not rolled back.
- Functions last for one evaluation, including in the REPL; submit declaration
  and use together. Value bindings retain the existing REPL persistence behavior.
  File/workbench runs each start fresh. No function or loop installs a host,
  grants authority, or bypasses independent operation authorization.

Executable no-host examples:

| Example | Task |
| --- | --- |
| [Collection processing](../crates/scope-script/examples/collection-processing.scope) | Flag long checklist items and count them. |
| [Reusable computations](../crates/scope-script/examples/reusable-computations.scope) | Calculate an invoice in integer cents using two functions. |
| [Release readiness](../crates/scope-script/examples/release-readiness.scope) | Summarize component checks, owners, blockers, and review effort. |

Run any with `cargo run --locked -p scope-script -- PATH`, or paste its source
into a workbench Script scratch. Mutation is included because ordered reductions
(counts, totals, and conditional results) otherwise require manual indexing or
additional collection APIs. No global mutable state is shared by stock runs.

Remaining limits: no imports, exceptions, `while`, `break`, `continue`, closures,
explicit returns, or filesystem/process APIs. A useful next milestone is small,
higher-order collection operations, rather than more control-flow machinery.
External downstream integrations remain untested by these pure examples.

## Unsaved source: the default workbench

Start `cargo run`, then enter:

```text
:scope scratch new script
:scope edit --commands
let experiment = {name: "daily", samples: [2, 4, 6]}
let total = experiment.samples[0] + experiment.samples[1] + experiment.samples[2]
if total == 12 {
    print({name: experiment.name, total: total, status: "ok"})
} else {
    print({name: experiment.name, total: total, status: "unexpected"})
}
:done
:scope script run
```

This exact source is runnable as
`cargo run -p scope-script -- examples/workbench.scope`. On a TTY, plain
`:scope edit` opens a multiline keyboard editor: arrows/Home/End, Enter,
Backspace/Delete, Tab, and bracketed paste edit the draft; Ctrl-S accepts and
Ctrl-C/Escape cancel. The transcript uses `:scope edit --commands` explicitly;
non-TTY input also defaults to this cooked-line editor, which
inserts ordinary lines with a newline. `:done` commits to memory only;
`:cancel` or EOF discards the draft. Inside the editor, use `:at LINE COLUMN`
(one-based Unicode scalars), `:delete COUNT` (forward, including newlines), and
`:insert TEXT` (escapes `\n`, `\t`, `\\`) to revise source.
`:complete` lists completions and `:complete INDEX` applies a current item.
Completion uses fresh Script metadata, not live interpreter bindings, and does
not evaluate source or authorize operations. These colon commands belong to
command mode, not the raw editor. Acceptance creates one revision only if text
changed; cancellation creates none. See [editing and rerunning](../TUTORIAL.md#editing-and-rerunning).

Each `:scope script run` snapshots the selected scratch source and evaluates it
with a fresh interpreter and `NoHost`. Output includes source identity/revision,
ordered prints, final value and interpreter status, or source-located errors.
No bindings persist between runs. Neither editing nor running implicitly saves
source, writes results, or creates a report. Opening `.scope` source grants no
authority and installs no providers.

`:scope run` remains a distinct, configured downstream semantic runner. Scope
Simple (`scope.simple`) supplies analysis; neither its analysis nor pure Script
success is downstream semantic acceptance. Interactive language sessions and
host-backed Script remain explicit embedding choices.

## Composing downstream semantic reads

`ScriptRunner` is the reusable `ReadOnlyCommand` adapter for
`scope_ide::cli::run_with_registry_and_command`. A launcher can explicitly expose
several existing operations from one `LanguageService` against one bound target:

```rust,ignore
let mut runner = ScriptRunner::with_operations(
    service,
    [
        ("inspect".to_owned(), Some(true)),
        ("check".to_owned(), None),
    ],
)?;
run_with_registry_and_command(args, registry, &mut runner)?;
```

Use actual language-local operation IDs, not these illustrative names. Exact API:

```rust,ignore
pub fn with_operations(
    service: &'a dyn LanguageService,
    operations: impl IntoIterator<Item = (String, Option<bool>)>,
) -> Result<Self, Box<dyn std::error::Error>>;
```

The existing `ScriptRunner::new(service, operation_id, authorization)` remains
compatible. The collection constructor rejects empty collections, duplicate IDs,
unknown IDs, and non-`Read` operations. It does not replace earlier bindings.
The normal semantic provider also rejects duplicate full identities, including
collisions between providers in an `AgentSession`. Short names are never used for
Script dispatch: identities remain `language.<language_id>.<operation_id>`.
There is no alias or language-wide permission mechanism.

For hosts already constructing a semantic `AgentSession`, the lower-level API is:

```rust,ignore
pub fn with_bindings(
    session: AgentSession<'a>,
    bindings: impl IntoIterator<Item = (String, Option<bool>)>,
    expected_source: impl Into<String>,
    expected_version: u64,
) -> Result<Self, Box<dyn std::error::Error>>;
```

This is `SemanticHost::with_bindings`; it accepts full operation identities and
rejects duplicate bindings. As with `SemanticHost::new`, the embedding must install
trusted semantic providers and capture source/version tokens from their target.
Unknown bindings are not discoverable and invocation fails explicitly. Non-read
bindings are excluded from discovery and explicitly rejected on invocation.
Discovery preserves the underlying session catalog order and original semantic
metadata, including typed arguments, effect, target, purpose, and constraints.

### Exposure is not authorization

There are four distinct boundaries: a language advertises operations; the host
selects exact identities for Script exposure; each selected identity separately
receives `Some(true)` authorization (both `None` and `Some(false)` deny); and the
semantic provider checks its host-bound target, scope, revision, typed arguments,
and source policy. Discovery does not grant authority. The runner installs the
same per-operation read-only restriction at both the Script/session and semantic
provider boundaries. Registered but unexposed operations remain unavailable.
Argument-validation or source-policy failures can be structured semantic envelopes;
a successful interpreter result alone is not semantic success.

The runner uses independent Script and semantic scratch sources. Each run captures
fresh host-owned snapshots; retained `RunRecord` evidence keeps the original text,
identity, and revision even after later edits. No source is implicitly saved and
scripts supply neither filesystem paths nor revision tokens. A scratch language
mismatch fails provider construction. Without a bound target, the existing pure
Script callback remains available but has no semantic host. Document-based `run`
continues to use the embedding's configured language for its cloned document;
embeddings must select the appropriate service rather than infer authority from a
filename.

The adapter supports one service and one target per runner, not multiple target
routing. All operations share the existing 64-call host budget. For compatibility,
`RunRecord.operation` remains the entry operation label (the lexically first full
binding for scratch callbacks), not a list of calls; `results` retains all returned
semantic envelopes. Provider descriptions are expected to remain stable during a
session, as in the existing semantic provider contract.

Only declared `Read` effects are supported. Candidate evaluation is callable only
when its existing operation is genuinely declared `Read` and independently
authorized; purpose metadata grants nothing. `ProposeChanges`, edit/review flows,
runtime authority, and filesystem capabilities are not added. Native language
providers remain trusted code; this adapter does not sandbox their computation.

### Runnable demonstration

From the repository root:

```sh
cargo run -p scope-script --example multi_semantic --locked --offline
```

The [demo](../crates/scope-script/examples/multi_semantic.rs) installs a small
fixture language, discovers `echo` and `measure`, invokes both against an independent
unsaved target, and combines their envelopes into a report. It then prints explicit
rejections for a registered-but-hidden operation and an exposed-but-denied one;
only the two authorized reads reach the provider. No downstream project or input files are needed. `--offline` requires the Rust
dependencies to have been fetched already.

SI7 extends the two Script adapters, not the underlying semantic system:
`SemanticToolProvider` and `AgentSession` already represented multiple operations,
typed dispatch, source policies, and duplicate rejection. Their existing facilities
were reusable, but the single-action fields in both Script adapters prevented
collection exposure. Downstream adoption only requires replacing the single
constructor with the collection constructor and choosing authorization separately
for each existing read operation; registry and launcher composition stay unchanged.

## Saved files: edit, run, repeat

From the repository root:

```sh
cargo run -p scope-script -- --help
cargo run -p scope-script -- crates/scope-script/examples/daily.scope
```

The general form is `cargo run -p scope-script -- FILE`. Files must be UTF-8 and
at most 64 KiB. Each run reads the saved source into a fresh interpreter with
`NoHost`: no providers, semantic services, workspace grants, or language runtime.
The CLI reads the supplied script file; the script itself receives no filesystem
authority. The `.scope` suffix is only a convention.

Edit and save in Zed or another editor, then repeat the command. The daily example
prints:

```text
{"name": "daily", "status": "ok", "total": 12}
```

No arguments (`cargo run -p scope-script`) starts the stateful REPL instead.
File runs do not preserve REPL bindings or implicitly save source/results.

## Structured values and conditionals

The ordinary, no-host `daily.scope` example uses records, arrays, field/index
access, arithmetic, and a boolean conditional:

```text
let experiment = {name: "daily", samples: [2, 4, 6]}
let total = experiment.samples[0] + experiment.samples[1] + experiment.samples[2]
if total == 12 {
    print({name: experiment.name, total: total, status: "ok"})
} else {
    print({name: experiment.name, total: total, status: "unexpected"})
}
```

Change a sample in your own saved copy and rerun to explore the other branch.
Conditions must be booleans; only the selected branch executes. Loops and
user-defined functions are described above. Host operations use the same structured values,
but their installation and authorization remain the embedding host's decision.

## Output and failure

```sh
cargo run -p scope-script -- --output summary crates/scope-script/examples/daily.scope
cargo run -p scope-script -- --output json crates/scope-script/examples/daily.scope
```

- `prints` (default): explicit `print` output only, with bounded human previews.
- `summary`: prints plus labeled final value and **interpreter** status.
- `json`: one full `scope-script.execution` version 1 record on stdout, including
  source path/text, printed values, tagged outcome, and no-host metadata.

Evaluation failure exits nonzero. Human errors include source path, one-based
line/column, and byte offset. Earlier prints survive runtime failure; parse
failure executes nothing. JSON evaluation errors are on stdout, with error
location and source chain; setup errors use stderr. JSON contains full source
and values, may be large, and is not a semantic proof or host replay record.
Interpreter success does not imply acceptance by a downstream semantic service.

## Developer-experience audit and priorities (S10)

The baseline was exercised through file execution, the line REPL, the default
scratch workbench, and explicitly authorized example hosts. Records, arrays,
checked integer arithmetic, Boolean short-circuiting, branch-local immutable
bindings, field/index access, and structured output already worked. Values are
null, Boolean, integer, string, array, and record; there are no floats or implicit
conversions. Source-located errors and fresh file/scratch interpreters already
worked too.

Priorities, with concrete baseline reproductions:

1. **Pure string/collection helpers and comments — implemented.** Previously
   `len([1, 2])` failed with `only builtin calls are supported`,
   `"a" + "b"` failed with `invalid string operator`, and `# note` was rejected.
   The small library above now supports counting, membership, splitting, joining,
   and key inspection without adding syntax or public types.
2. **Feedback — improved.** Unknown-binding and missing-field errors now include
   the requested name; completion includes `if`, `else`, and the helpers, and
   avoids comments. Workbench completion still does not evaluate draft bindings.
3. **Variable-sized processing and reuse — implemented in S11.**
   The S10 baseline lacked loops, mutation, and user functions; S11 adds `for`,
   `let mut`, assignment, and named functions as described above. Imports and
   try/catch remain unsupported.
4. **Operation access — host boundary, not missing call syntax.**
   `scope.operations()` is `[]` in stock CLI/workbench. Merely opening a workspace
   does not install providers. No new host loader or automatic grants were added.

### From local values to permitted operations

`scope.operations()` returns the host's existing catalog as structured values.
Inspect its argument/result contracts, effects, and authorization metadata; use
`scope.invoke(provider, action, arguments)` with those exact identities. Store
its returned value in a binding, inspect fields, and pass the appropriate fields
into another operation's argument record. Discovery never authorizes a call.
Returned semantic failure envelopes can be inspected with existing conditionals;
transport/policy failures remain typed interpreter errors, not catchable values.
Required capability/authorization detail depends on the existing host descriptor;
Script does not invent or infer a separate capability schema.

For a confined workspace task, reuse the existing Markdown inventory example:

```sh
cargo run --locked -p scope-script --example workspace -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/markdown-inventory.scope
```

See the [example catalog](../examples/README.md) for its explicit read grant and
workspace arguments. `markdown-inventory.scope` returns documents and revision
tokens; its existing integration tests cover empty/nonempty workspaces and no
writes. It is not runnable with workspace authority in the stock no-host CLI.
For downstream composition, run the `multi_semantic` demonstration above: it
uses a fixture language through the public semantic interface, not an installed
Path/Forth/Hilb runtime. It discovers and composes two reads and demonstrates
both hidden-operation and authorization denials. Existing single-operation
constructors remain compatible.

The smallest blocker to using such operations in the stock workbench is an
explicit embedding that installs a provider, binds a target snapshot, and chooses
per-operation authorization. That is deliberately not solved by a Script alias,
a second registry, native filesystem builtin, or automatic permission elevation.

### Validation

```sh
cargo test -p scope-script -p scope-workbench --all-targets --locked --offline
```

This passed for S10, including downstream adapter contracts, confined workspace
reads and denials, real-terminal workflows, helper type/resource tests, and the
new `everyday` tests comparing file JSON results with two fresh scratch runs of
each beginner example (including the expected runtime failure). The hello,
checklist, workspace inventory, and multi-operation demo commands above were
also executed successfully. `--offline` requires cached dependencies. External
Path, Scope Forth, and Hilb repositories were not executed; their existing Scope
integration interfaces were not changed.

## S11 assessment and S12: handling conversion results

**Baseline assessment.** S11 already supports the required composition: records,
Boolean field tests, branch-local bindings, object-valued mutable locals, loops,
and functions returning a final expression. There is no `return`, `try`, catch,
or result-propagation operator. A function must end in an expression, not an
`if`; use a mutable object result and end with that binding. Changing an object's
shape is allowed; assigning a different runtime kind is not. Reading an absent
field is fatal, so select the tag before accessing success/failure fields.

**S12 contract.** `parse_int(text)` is
one pure builtin taking exactly one string. It returns an ordinary tagged
`ToolValue` object, not a new language type:

| Case | Result |
| --- | --- |
| Valid integer text | `{ok: true, value: integer}` |
| Invalid integer text or overflow | `{ok: false, error: {code: "invalid_integer", message: "expected a signed 64-bit decimal integer"}}` |
| Wrong argument type or arity | Fatal interpreter error, not an `ok: false` value |

Parsing uses Rust's `i64` decimal parser: an optional ASCII `+` or `-` followed
by one or more ASCII digits, within `-9223372036854775808` through
`9223372036854775807`. Leading zeroes are accepted; whitespace, radix prefixes,
separators, Unicode digits, empty input, and a sign alone are rejected. Invalid
syntax and overflow use the same fixed message shown above.

The inactive field is absent: success has no `error`, failure has no `value`.
This requires no provider, capability, host call, or exception syntax. Test
`result.ok` before reading `result.value` or `result.error`. An `ok: false`
object is a successful interpreter value: printing or returning it does not
itself make a file run exit nonzero.

### Handle a conversion locally

The [conversion handling example](../crates/scope-script/examples/conversion-handling.scope)
processes `"42"` and `"not-an-integer"`:

```text
let result = parse_int("42")
if result.ok {
    print(result.value + 1)
} else {
    print({code: result.error.code, message: result.error.message})
}
```

The success branch prints `43`; invalid text takes the error
branch without stopping the next iteration. No implicit string-to-integer
coercion is added to arithmetic.

### Propagate a result through a function

The [function propagation example](../crates/scope-script/examples/conversion-propagation.scope)
returns the original failure object unchanged, or transforms the successful value:

```text
fn increment_text(text) {
    let parsed = parse_int(text)
    let mut result = parsed
    if parsed.ok {
        result = {ok: true, value: parsed.value + 1}
    }
    result
}
```

The caller checks `increment_text("41").ok` through a result binding (as in the
example) and prints `42` on success. This is explicit value propagation, not
catching errors: wrong types, checked arithmetic overflow, resource limits,
and host authorization/transport errors still stop evaluation. In the REPL,
submit a function declaration and its calls on the same line; functions do not
persist to the next submission. Saved files and Script scratches accept the
multiline examples and start fresh on each run.

### Preserve the existing error boundaries

Do not wrap or normalize `scope.invoke` results into this conversion shape.
Existing semantic envelopes retain their own `status` and operation-specific
fields: the tested read envelope uses `read_only`; failures include
`revision_conflict`, `unauthorized`, `ambiguous_target`, and `invalid_arguments`.
Failure envelopes can lack `value`; their `message`, `operation_id`, and optional
`invocation_schema` remain intact. Policy denial/missing authorization and
unavailable operations can instead be fatal interpreter errors. A returned
semantic failure is inspectable data, not permission to catch host errors.
Neither conversion success nor interpreter success establishes semantic acceptance.

Syntax errors, unbound names, invalid calls/types, illegal mutation, recursion,
and missing-field access retain fatal, source-attributed diagnostics. Evaluation
steps, work, retained-value/output budgets and call-depth guards remain shared
through nested functions and loops. Exhaustion terminates that evaluation;
there is no catch or retry mechanism inside the script to reset its budget.
A later independent evaluation has its own budget, as before.
The CLI's outer `scope-script.execution` record is also unchanged and distinct
from both kinds of returned object.

### Compose with an explicitly configured semantic host

Run the [recoverable semantic example](../crates/scope-script/examples/recoverable_semantic.rs)
from the repository root:

```sh
cargo run --locked --offline -p scope-script --example recoverable_semantic
```

This embedding reuses the existing `MultiLanguage` provider and
`ScriptRunner::with_operations` with explicit `echo` and `measure` bindings,
each read-authorized with `Some(true)`. It supplies a separate `example.multi`
target scratch containing `independent target source`; the Script scratch is
not the semantic target. No external runtime or input file is required. Stock
CLI, REPL, and workbench runs still install no semantic providers or grants.

The script parses `"3"`, then passes the integer as `limit` alongside
`enabled: true` to `language.example.multi.measure`. It inspects the returned
`read_only` envelope. A second invocation deliberately supplies a string limit
and inspects the unchanged `invalid_arguments` envelope, including its message
and invocation schema. Assertions check both statuses and that only the valid
`measure` call reached the provider. Conversion errors remain local tagged
values; semantic argument rejection remains a semantic envelope, not a
conversion error or caught host exception.

### Validation

Run the two pure examples from the repository root:

```sh
cargo run --locked --offline -p scope-script -- --output summary crates/scope-script/examples/conversion-handling.scope
cargo run --locked --offline -p scope-script -- --output summary crates/scope-script/examples/conversion-propagation.scope
```

Both commands passed with interpreter success and two prints: one successful
conversion and one `invalid_integer` failure. The successful numeric outputs
were `43` and `42`, respectively. The `recoverable_semantic` command above also
passed, including its envelope and dispatch assertions. Offline commands require
cached dependencies. The semantic example emits an unused-fixture-constant
warning; this does not affect execution.

Concise baseline findings: S11 already supplied the control flow needed here.
REPL runtime errors preserved prior prints and allowed later submissions;
scratch diagnostics retained source/revision, Unicode line/column, and byte
offsets and recovered to a later successful run. Before S12, `parse_int` was an
unknown function. The final checks above exercised the actual builtin in file
mode and the explicit semantic runner; they did not repeat interactive REPL or
manual scratch editing.

A final `cargo test -p scope-script -p scope-workbench --all-targets --locked --offline`
rerun still failed in the workbench test
`syntax_runtime_and_immutable_errors_preserve_ordered_unicode_prints_and_recover`,
which retains the stale immutable-assignment diagnostic expectation identified
in the baseline. This is an existing baseline failure, separate from the conversion contract.
`cargo test -p scope-script --all-targets --locked --offline` passes, including
new conversion, function propagation, CLI, scratch, budget, and host-denial
regressions. The mechanism is deliberately structural rather than nominal:
user records can use the same shape, and no static generic `Result` type or
universal host-error recovery is introduced. Missing-file recovery is possible
only if an explicitly configured host already exposes it as operation data;
setup failures and registry dispatch errors are not converted by Script.

## Host support is explicit

Naming a provider or operation does not install or authorize it. An embedding
host must register services, choose target snapshots, and authorize concrete
calls. A runnable declaration-language example is:

```sh
cargo run -p scope-script --example semantic -- --allow-read crates/scope-script/examples/semantic.scope
```

This is a separate trusted example host, not a flag that enables providers in
the standalone CLI. It installs one declaration read against a fixed host-owned
target and selection. The supplied file is Script source, not the target.
Omitting `--allow-read` denies invocation. It does not install Path, Forth, or
an arbitrary downstream runtime; there is no automatic host loader.

For a downstream, host-backed unsaved-source workflow, the explicit
`scratch_runner` example installs a semantic Script runner in the IDE shell.
Unlike the default pure workbench, it uses a separately configured App target.
Source buffer and App target remain separate; execution neither saves the script
nor edits the target. See the
[example catalog](../examples/README.md) for commands and host classifications.
See also [architecture](architecture.md) and [source evaluation](SOURCE_EVALUATION.md).
