# Scope Script: A Weekend of Little Programs

Welcome! This is a workshop for a programmer with a free afternoon, a little
curiosity, and perhaps a cup of something warm. We'll start with five, build a
calculator, and finish by asking questions about source code.

Scope Script is small enough to explore rather than memorize. Some familiar
conveniences aren't here yet. Treat those moments as opportunities to make a note,
not as puzzles with secret syntax.

## Pack your bag

Run every shell command below from the **repository root**. You need Git, Cargo,
and the Rust toolchain selected by [`rust-toolchain.toml`](../../rust-toolchain.toml).
The first build needs network access to fetch dependencies, including Cell at the
repository's pinned version:

```sh
cargo build --locked -p scope-workbench -p scope-script
```

After dependencies are cached, the exercises work offline. No model server, API
key, Wasm target, or remote service is used. Commands below use `--offline` to make
that explicit. If Cargo reports a missing dependency, complete the initial build
online; that is setup, not a Script capability.

The [six reference programs](../../crates/scope-script/examples/weekend/) are
checked in. Use your editor to make a personal copy before changing one. Script
has **no comment syntax**: don't paste the surrounding explanations into source.

Suggested pace: chapters 1–4 on Saturday, 5–7 on Sunday, and a few challenges from
chapter 8 whenever curiosity strikes. Keep the [field notebook](SCOPE_SCRIPT_FIELD_NOTES.md)
nearby. You do not need to finish everything.

## 1. Hello, Scope Script!

Let's separate three satisfying actions: writing source, running it, and saving
it. They are not the same action.

Start the workbench:

```sh
cargo run --locked --offline
```

At its prompt, enter this transcript. `:done` is an editor command, not Script:

```text
:scope scratch new script
:scope edit --commands
print("Hello, Scope Script!")
2 + 3
:done
:scope script run
:scope script run
```

Both runs include:

```text
print: "Hello, Scope Script!"
final: 5
status: interpreter success
```

The run also identifies a source and revision. Strings are displayed with quotes;
those quotes are presentation, not extra characters in your greeting.
`print` records explicit output; the final expression supplies the final value.
Nothing was saved. The scratch source survives between runs, but its variables
would not: **each workbench Script run starts a fresh interpreter**.

Now change the calculation without leaving Scope:

```text
:scope line 2 "2 + 8"
:scope script run
:scope scratch show
:scope quit
```

The final value is now `10`. `line` replaces a whole source line using a JSON
string. Quitting loses unsaved scratch buffers.

Prefer a file? The same program is already saved for you:

```sh
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/hello.scope
cargo run -p scope-script --locked --offline -- --output summary crates/scope-script/examples/weekend/hello.scope
```

The first command prints only `"Hello, Scope Script!"`. The second also reports
`Final value: 5` and `Interpreter status: success`. File runs read saved text;
editing a scratch copy does not change that file. To keep your own program for
now, save it with Zed or another ordinary editor. We won't enable workspace writes
for this workshop.

**Try it:**

- Change the greeting and calculate `6 * 7`.
- Put `let n = 6` before `n * 7` and run twice. Notice there is no duplicate-binding error.
- Remove `print` and run the file in both output modes.

**Wonder:** What should a tool show when a program has a useful final value but
never explicitly prints it?

## 2. Learning the language: a tiny statistics desk

Instead of a list of grammar rules, let's examine three measurements:

```text
let samples = [7, 11, 18]
let total = samples[0] + samples[1] + samples[2]
let mean = total / 3
let report = {samples: samples, total: total, mean: mean, remainder: total % 3, exact: total % 3 == 0}
print(report)
report
```

Run the saved version:

```sh
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/numbers.scope
```

The displayed record has `total: 36`, `mean: 12`, `remainder: 0`, and
`exact: true`, plus the original array. Object keys display in sorted order, not
necessarily source order. Arrays start at index zero. `report.mean` and
`report["mean"]` select the same field.

Values are `null`, booleans, signed 64-bit integers, strings, arrays, and
string-keyed objects. There are no floating-point literals. Division is integer
division, truncating toward zero. Change the last sample to `19`: the mean remains
`12`, but the remainder becomes `1`. That is not a floating-point average!

Bindings declared with `let` are immutable. You cannot assign a new value or
redeclare/shadow a name within its enclosing scope. Separate statements with a
newline or semicolon. Strings use double quotes and accept `\n`, `\r`, `\t`,
`\"`, and `\\` escapes; literal Unicode is welcome.

### A guessing calculation, not a guessing game

Script has no input prompt, so make the guess in source:

```text
let secret = 17
let guess = 12
if guess == secret {
    print("Found it!")
} else {
    if guess < secret {
        let distance = secret - guess
        print({hint: "higher", distance: distance})
    } else {
        print({hint: "lower", distance: guess - secret})
    }
}
print(false && missing)
```

```sh
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/control-flow.scope
```

Output is `{"distance": 5, "hint": "higher"}` followed by `false`. The
unselected branch doesn't run, and `&&` short-circuits before looking up
`missing`. Conditions must actually be booleans; integers aren't truthy.
`distance` exists only inside its branch. Write nested `if` inside `else { ... }`;
there is no `else if` form.

Your small toolbox:

- Arithmetic: `+ - * / %`, unary `-`, and parentheses.
- Comparison: `== != < <= > >=`; ordering works on integers or strings.
- Logic: `! && ||`, with boolean operands.
- Equality compares structured values without coercion: `1 == "1"` is false.
- Addition is integer arithmetic, **not string concatenation**.

There are no loops, user-defined functions, recursion, mutation, imports, or
collection methods such as `map` or `length`. Pure built-in functions now include
`len`, `contains`, `keys`, `join`, and `split`, alongside `print`.
`scope.operations` and `scope.invoke` ask a host for help. See the
[small standard library](../scope-script.md#small-pure-standard-library) for contracts.
Use `#` for line comments.
Reusable computations currently mean bindings within a run or repeated source,
not function definitions. A Fibonacci exercise must be a short, manually unrolled
sequence of bindings, not a recursive function disguised as supported syntax.

**Try it:**

- Try `guess = 17` as an edit to the declaration, then a guess above the secret.
- Add `print(report.samples[0] < report.mean)` to the statistics program.
- Express the first six Fibonacci numbers using a new binding for each number.
  Compare the experience with Python, Rust, or Forth.

**Wonder:** Which missing primitive would make your next little program easier:
iteration, functions, or a collection operation? Record a concrete example.

## 3. Playing in the interactive workbench

Let's revise a program repeatedly without restarting Scope. Start a **new** shell
so the IDs below match:

```sh
cargo run --locked --offline
```

```text
:scope scratch new script
:scope edit --commands
let n = 6;
n * 7
:done
:scope script run
:scope line 1 "let n = 8;"
:scope script run
:scope scratch show
```

The final values are `42`, then `56`. Source stays in the buffer; interpreter
bindings don't stay between runs.

On a real terminal, `:scope edit` without `--commands` opens a multiline keyboard
editor. Arrows, Home/End, Backspace/Delete, Enter, Tab, and bracketed paste edit
the draft. **Ctrl-S accepts to memory**, not disk. Ctrl-C or Escape cancels.
Unicode and tabs may display as safe ASCII escapes; the underlying text is kept.

In explicit command mode you can revise the `8` precisely:

```text
:scope edit --commands
:show
:at 1 9
:delete 1
:insert 9
:done
:scope script run
```

Now the result is `63`. Command mode starts at the beginning and **inserts** text:
retyping the whole program here would put it in front of the old program.
`:at` uses one-based Unicode character positions; `:delete` deletes forward,
including newlines. `:cancel` discards the draft. `:complete` lists metadata-only
completions; `:complete INDEX` applies one. Completion doesn't run your program.

For a lower-level replacement exercise, create another buffer:

```text
:scope scratch new script
:scope scratch replace 1 1 1 1 let n = 6;\nn * 7
:scope script run
:scope scratch replace 1 11 2 6 
:scope scratch show
:scope scratch list
:scope scratch select 0
```

The first replacement inserts two lines and evaluates to `42`. The second deletes
the newline and `n * 7`, leaving `let n = 6;`. The trailing space after the final
coordinate means empty replacement text. Unlike editor `:at`, **scratch replace
coordinates use one-based UTF-8 byte columns** and a half-open range. These
examples use ASCII so the distinction doesn't bite us.

Open a saved file as another buffer:

```text
:scope scratch open crates/scope-script/examples/weekend/hello.scope
:scope script run
:scope line 2 "2 +"
:scope script run
:scope line 2 "2 + 3"
:scope script run
:scope quit
```

The middle run reports a parse error; fixing the line restores `final: 5`.
The disk file is unchanged. `scratch list` shows selected IDs and dirty state.
`new` selects the new buffer; `select` returns to an old one. Dirty `close` is
refused; `scratch discard` explicitly throws away the selected buffer. Do not use
it on work you want to keep.

### History is not source storage

At the outer TTY prompt, Up/Down navigate lines and recall history at buffer
boundaries; Ctrl-P/Ctrl-N recall entries directly. Ctrl-A/Ctrl-E move to the
beginning/end of the pending input. Ctrl-C cancels pending input. Editor-internal
commands don't become individual shell history entries. Piped input is
line-oriented and doesn't persist history. Scratch buffers aren't restored after
exit, even if a command appears in history.

Saving is a separate, authorized workflow: `scratch destination`, `scratch save`,
and `scratch promote` are not evaluation commands. The default shell is read-only;
this workshop does not require changing that. See the
[workspace editing guide](../../docs/integration.md) if you
later choose to explore explicit checked saves.

**Try it:**

- Make a wrong edit, cancel it, and compare the source and revision.
- Deliberately insert a duplicate program, inspect with `:show`, and remove it.
- Create two scratches, switch between them, and provoke one syntax error in each.

**Wonder:** When you see an error, can you tell which text was evaluated without
consulting your terminal scrollback?

## 4. Writing real `.scope` files

Let's keep a useful numerical playground. Open
[`little-calculator.scope`](../../crates/scope-script/examples/weekend/little-calculator.scope)
in your editor. Its input is an ordinary record:

```text
let input = {left: 29, right: 6, operation: "divide"}
if input.operation == "add" {
    print({answer: input.left + input.right})
} else {
    if input.operation == "divide" {
        if input.right == 0 {
            print({error: "Choose a nonzero divisor"})
        } else {
            print({quotient: input.left / input.right, remainder: input.left % input.right})
        }
    } else {
        print({error: "Try add or divide"})
    }
}
```

```sh
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/little-calculator.scope
```

It prints `{"quotient": 4, "remainder": 5}`. Change the record, save, and rerun.
The zero-divisor branch is our own error **data**, not a caught exception. There
is no `try`/`catch`; unguarded runtime errors stop the evaluation.

The file runner takes one source file, not Script arguments. There is no argv,
stdin-reading builtin, environment access, import, or multi-file composition.
To reuse a definition, keep it in this file; copying source is possible, loading a
module isn't. Input records are a convenient current substitute for CLI options.

Compare presentation modes:

```sh
cargo run -p scope-script --locked --offline -- --output summary crates/scope-script/examples/weekend/little-calculator.scope
cargo run -p scope-script --locked --offline -- --output json crates/scope-script/examples/weekend/little-calculator.scope
```

Summary adds a final value of `null` because an `if` statement finishes with
`null`. JSON emits one versioned execution record with the full source, printed
values, outcome, and `no-host` metadata. It is a record of this interpreter run,
not proof of a successful semantic operation. It can expose your full source if
you share it. Human previews are bounded and can truncate large values; JSON
retains full values and is the machine-readable option.

### A different kind of interactive session

The standalone runner without a filename is a **stateful REPL**, not the scratch
workbench:

```sh
cargo run -p scope-script --locked --offline
```

Enter each line separately:

```text
let x = 4
x + 1
1 / 0
print(x)
let x = 9
:quit
```

You see `=> 4`, `=> 5`, a division error, then `4`; rebinding `x` fails. A runtime
failure doesn't reset earlier bindings. Each redirected line is also evaluated
separately; don't pipe a multiline file into this REPL as a replacement for the
file runner.

| Where you run | What survives the next evaluation? |
| --- | --- |
| Workbench `:scope script run` | Scratch source/revision, not interpreter bindings |
| Standalone file runner | Saved file; no interpreter session |
| Standalone no-argument REPL | Top-level bindings until process exit |
| Explicit host examples below | Host-specific reads; not automatically installed elsewhere |

**Try it:**

- From a blank personal file, write a rectangle area/perimeter report without copying.
- Add a multiply branch to your calculator; try an unknown operation name.
- Change `right` to zero, then remove the guard and compare error behavior.

**Wonder:** Would this program be more useful with command-line input, or with a
better way to reuse calculations? Capture the smallest example that shows why.

## 5. Scope Script meets Scope

Now we give Script a very particular pair of eyes. This is **not** a general
filesystem API: an existing example host can summarize Markdown in a bounded,
read-only workspace. The host does the listing, reading, and line processing;
Script orchestrates the calls and shapes a report.

Run:

```sh
cargo run -p scope-script --example workspace --locked --offline -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/weekend/workspace-report.scope
```

The script is:

```text
let operations = scope.operations()
let discovered = scope.invoke(operations[0].provider, operations[0].action, {})
let batch = scope.invoke(operations[1].provider, operations[1].action, {})
print({documents: batch.document_count, pending: batch.todo_count, needs_attention: batch.todo_count > 0})
print(batch.documents[1].path)
print(batch.documents[1].pending)
```

You see a Rust-debug-style object containing `documents: Integer(2)`,
`pending: Integer(2)`, and `needs_attention: Boolean(true)`. Next comes
`String("release.md")` and an array describing pending lines **9 and 10**.
This example host uses Rust debug formatting, not the standalone CLI formatter.

Here's what happened:

1. `scope.operations()` asked this host for its catalog. It advertises provider
   `downstream_workspace` with actions `discover_markdown`, then `inspect_discovered`.
2. Discovery returns sorted path/revision records and remembers their revisions.
3. Inspection rereads those documents, checks revisions, and returns
   `document_count`, `todo_count`, and `documents`.
4. Each document has `path`, `revision`, full `text`, `headings`, and `pending`.
   A pending record has one-based `line` and `text` fields.

This Markdown processing is deliberately simple: headings start with `#`; pending
lines contain `TODO` or `- [ ]`. It isn't a Markdown parser. The two actions are
**example-host batch projections, not registered language semantic operations**.
Their known order makes indexing convenient here; don't assume every host has
this catalog or order.

To inspect the source information, append `print(batch.documents[1].text)` or
`print(batch.documents[1].headings)` to a personal copy and run it through the
same host. No loop is needed to print a whole structured collection.

The explicit `--allow-read ROOT` belongs to this Rust example host. The normal
file runner doesn't recognize it. Try the *unchanged* report in the ordinary
runner:

```sh
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/workspace-report.scope
```

It fails on `operations[0]`: that runner's catalog is empty. Merely opening a
workspace or a `.scope` file does not install a provider or grant access.

Keep the supplied small workspace for now. The host rejects more than 128 entries,
16 Markdown documents, 32 KiB of source per batch, or 8 charged host calls. It
materializes files/listings before these checks: these are logical limits, not a
streaming memory budget. Do not point it at your whole checkout just to stress it.
Discovery/rechecking also isn't an atomic multi-file snapshot.

**Try it:**

- Print the first document's headings instead of the second document's pending work.
- Add a conditional message when `todo_count > 0`.
- In a small personal copy of the fixture, add one TODO line and rerun with that
  directory as the explicit root. No wider grant is necessary.

**Wonder:** What question could you answer with the existing batch fields alone,
without asking the host for another capability?

## 6. Scope Script and other languages

Let's inspect a declaration written in another language. This repository includes
a tiny **declaration fixture**, not a full compiler or Scope Forth runtime.

```sh
cargo run -p scope-script --example workspace_semantic --locked --offline -- --allow-read crates/scope-script/examples/semantic-workspace crates/scope-script/examples/weekend/semantic-investigator.scope
```

The host reads fixed `example.decl`, selects its first declaration, and exposes one
read-only semantic operation. The optional final path is the **Script program**,
not the target declaration file. The script discovers, invokes, and projects:

```text
let operation = scope.operations()[0]
print({provider: operation.provider, action: operation.action})
let result = scope.invoke(operation.provider, operation.action, {})
print({status: result.status, name: result.value.name, kind: result.value.kind, partial: result.value.partial})
print({type_analysis: result.value.inferred_type.status, references: result.value.references.status})
result
```

The debug-formatted output contains:

- Provider `semantic`, action `language.example.declarations.describe_selection`.
- `status: "read_only"`, `name: "answer"`, `kind: "integer_literal_binding"`,
  and `partial: true`.
- Type analysis `"unknown"` and references `"unsupported"`.

Those last fields matter: this language service describes a selected declaration;
it does not infer types or find references. `result` is an envelope, not simply
the name. It includes `status`, `operation_id`, `rationale`, and structured
`value`. The host prints explicit outputs but doesn't display the final envelope;
append `print(result)` to see it. Catalog entries also carry `description` and
`metadata`; `print(operation)` lets you explore them.

Script knows how to compose records. The language service knows what a declaration
means. The host chooses the target/selection, captures source/version, and grants
this exact read. Discovery is not authorization, and interpreter success is not
semantic acceptance: inspect the envelope too. This host does not offer arbitrary
path arguments, edits, or runtime execution.

### What about Scope Forth?

There is no verified, stock Script-to-Forth workflow in this checkout. These hosts
do **not** install Forth, and the declaration fixture is not a Forth substitute.
Downstream hosts can integrate language sessions and expose operations, but that
requires explicit integration outside this workshop. Writing `scope.invoke` with
a Forth-looking name cannot load or authorize one.

A useful boundary experiment, requiring no downstream installation:

```sh
cargo run -p scope-script --locked --offline
```

```text
scope.operations()
scope.invoke("forth", "inspect", {})
:quit
```

The catalog is `[]`; invocation reports an unknown-operation error. The action is
intentionally nonexistent, **not a proposed Forth API**. This distinguishes a
missing host integration from invalid Script syntax. We haven't verified behavior
in any separate Scope Forth repository.

**Try it:**

- Print `result.value.span` and find the zero-based line and byte columns.
- Print the entire catalog instead of indexing it, then compare with chapter 5.
- Run the semantic script through the no-host CLI and explain where it fails.

**Wonder:** If a Forth host exposed a read-only word description, what information
would you want in a Script report? Keep that as a question, not a supported promise.

## 7. Let's build something!

Time to stop following recipes. Choose a project, sketch its output first, and
write your version before opening the reference. Each reference is a normal file,
so the solutions can stay out of your way.

### Project A — The little calculator

**Goal:** Turn two integer inputs and an operation name into a useful result.

**Building blocks:** records, immutable bindings, integer arithmetic, nested
conditionals, and `print`. No runtime input is available; edit an input record.

**Hints:** Start with addition. Guard division by zero before evaluating `/`.
Return a record with a helpful error field for an unknown operation.

**Reference (open when ready):**
[`little-calculator.scope`](../../crates/scope-script/examples/weekend/little-calculator.scope).

```sh
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/little-calculator.scope
```

The reference prints quotient `4` and remainder `5` for `29 / 6`.

**Extensions:** add subtraction; compare negative division with your usual
language; return the original inputs beside the answer.

### Project B — The workspace explorer

**Goal:** Tell a teammate whether the small documentation workspace needs attention.

**Building blocks:** the chapter 5 host's discovery/inspection actions, batch totals,
document source/heading/pending fields, and structured output.

**Hints:** Discover before inspecting. Print the whole batch while exploring its
shape. Don't index a document unless you know it exists; the reference deliberately
assumes the supplied two-file fixture.

**Reference:** [`workspace-report.scope`](../../crates/scope-script/examples/weekend/workspace-report.scope).

```sh
cargo run -p scope-script --example workspace --locked --offline -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/weekend/workspace-report.scope
```

Expect two documents, two pending lines, and the `release.md` pending records.

**Extensions:** show headings; show source next to pending lines; make a tiny
personal fixture representing your own weekend project.

### Project C — The semantic investigator

**Goal:** Answer “What declaration is selected, and how much does the language
actually know about it?” Don't turn `unknown` into a confident answer.

**Building blocks:** semantic discovery metadata, an empty argument object,
`scope.invoke`, envelope status, and partial analysis fields.

**Hints:** Inspect the catalog first. Keep the returned envelope in a binding.
Separate the provider's status from the facts in `value`.

**Reference:** [`semantic-investigator.scope`](../../crates/scope-script/examples/weekend/semantic-investigator.scope).

```sh
cargo run -p scope-script --example workspace_semantic --locked --offline -- --allow-read crates/scope-script/examples/semantic-workspace crates/scope-script/examples/weekend/semantic-investigator.scope
```

Expect the `answer` declaration and explicit unknown/unsupported analysis, as in
chapter 6. This is a semantic read, not execution of `example.decl`.

**Extensions:** include the literal and selected span; show rationale; make the
report explicitly warn when analysis is partial.

**Try it:** Pick two extensions, implement one without looking at its reference,
and record one design decision you made independently.

**Wonder:** Which project would you actually keep? What is the smallest missing
facility that would make it substantially more useful?

## 8. Breaking things on purpose

A good language workshop includes safe ways to be surprised. Use a disposable
scratch or personal file, not source you care about. These are bounded probes,
not stress tests. Record the exact runner as well as the source.

### A five-minute failure laboratory

Start the standalone REPL:

```sh
cargo run -p scope-script --locked --offline
```

Submit these **one line at a time**, then `:quit`:

```text
print("before"); 1 / 0
print("never"); let broken =
"hello" + "world"
[][0]
{}.missing
if 1 { print("yes") }
let kept = 7; 1 / 0
kept
if true { let local = 3; print(local) }
local
scope.invoke("missing", "missing", {})
print("recovered")
```

Expected observations, rather than exact diagnostic wording:

- The first print survives the division error. Runtime errors aren't rollback.
- `"never"` does not print: a parse error prevents the entire submitted program
  from executing.
- String addition, empty-array indexing, missing fields, and integer conditions fail.
- `kept` survives the later runtime error; branch-local `local` does not escape.
- Unknown host invocation fails, but the following submission still prints.

File runs instead exit nonzero on evaluation failure; human diagnostics include
path, one-based line/column, and a UTF-8 byte offset. Wrong builtin arity, such as
`print()`, is rejected during parsing. Some errors may feel like they belong to a
different stage: that's a useful observation to write down.

### Choose a few more probes

| Probe | Safe procedure and what to watch |
| --- | --- |
| Repeated evaluation | Run a scratch with `let n = 1` twice; compare with submitting it twice in the stateful REPL. |
| Revision recovery | In chapter 3, replace `2 + 3` with `2 +`, run, repair, and run again. Does the report clearly identify the revised text? |
| Duplicate bindings | Submit `print("before"); let same = 1; let same = 2`. Duplicate-name validation rejects before printing. |
| Unexpected arguments | Submit `scope.invoke("x", "y", [])`; arguments must be an object even before a useful host can handle them. |
| Empty workspace | Point the chapter 5 host at a personal empty directory. Discovery/inspection can succeed with zero documents, but the reference's `[1]` then fails after printing totals. |
| Host ordering | In a copy of the workspace script, invoke `inspect_discovered` before discovery: the host rejects the sequence. |
| Incorrect host arguments | Replace the discovery `{}` with `{path: "release.md"}` in a personal copy. The workspace host requires empty arguments; paths/revisions are host-owned. |
| Workspace boundary | Omit `--allow-read` from a host command: startup refuses access. Do not broaden grants to make a failing probe pass. |
| Missing semantic action | In a semantic-script copy, change the discovered action to `"missing"`. Expect an unknown-operation failure, not an automatically loaded service. |
| Terminal cancellation | Open a draft, change it, cancel, and rerun. Compare source and revision. |
| Display | Print `[1, [2, [3]]]` and an object with reordered keys. Then try a single 250-character string in a file, comparing summary with JSON. Human strings preview only 200 characters. |
| Depth | Manually nest a small array ten levels deep. Human output previews depth 8; full JSON distinguishes display truncation from data loss. Stop there rather than constructing huge inputs. |

The unusual-input probes above have no need for network, writes, or wider
capabilities. Try at most a few deliberate host calls per run. Script has a 64 KiB
source limit, nesting/expression depth limit of 64, and logical evaluation/value
budgets. These are not a hard execution deadline or a process sandbox for trusted
native providers. No unbounded stress test is needed to evaluate the experience.

**Try it:** Reproduce one error in both file and REPL modes; fix a scratch error
without restarting; reduce one confusing case to its smallest source.

**Wonder:** Was the surprising part the language rule, the host boundary, the
message, or the way the terminal presented it? There can be more than one answer.

## 9. Where could Scope Script go next?

End with a small success, not a feature wishlist. Run the statistics program again:

```sh
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/numbers.scope
```

It still gives total `36` and mean `12`. You've now seen the same building blocks
in a numerical report, a workspace report, and a semantic investigation.

For one final program, replace its measurements and field names with something
from your own day: three build timings, three puzzle scores, or three estimates.
Keep the integer-division remainder visible. Run through the same file command,
using your personal path. The expected result is your three-input sum and an
honest integer mean—not an implied floating-point statistic.

**Try it:**

- Make a developer utility with a fixed configuration record and a useful report.
- Explore a read-only workspace report using only fields the existing host supplies.
- Sketch a language-introspection report that preserves unknown/partial results.

Composition with other language runtimes, lightweight automation, and safe use of
Cell-provided capabilities are interesting future experiments, **not promises
that these runners already support them**. Ask what existing host can supply a
needed operation before assuming that Script can perform it.

**Wonder:** What made you want to keep playing—and what nearly made you stop?
Write one concrete example of each in your [field notes](SCOPE_SCRIPT_FIELD_NOTES.md).
That is a much better starting point for improving a language than a speculative
list of features.

## Verification notes and further trails

The six checked-in examples were executed through the commands above. The
command-mode create/edit/run/revise/open/error-recovery workflows and the REPL
failure laboratory were exercised with redirected input. The workspace and
semantic hosts ran offline with cached dependencies, against their supplied local
fixtures. Related CLI, workspace-semantic, and scratch-runner tests also passed.
TTY keybindings are described from the implementation and existing documentation;
the physical keyboard/viewport experience was **not manually verified** in this
workshop preparation. Forth interoperation in a separate downstream repository
was not tested. No language or host implementation was changed to make an
exercise work.

The practical constraints shaping this workshop were the lack of functions,
iteration, imports, runtime input, and a general standard library; the
absence of providers in pure runners; host-specific formatting and catalogs; and
the declaration fixture's intentionally partial analysis. Keep those separate
from new observations you make yourself. Since the original workshop, S10 added
line comments and the small pure helper library described above.

For more detail, see the [Script guide](../scope-script.md),
[scratch editing](../../TUTORIAL.md), and [example catalog](../../examples/README.md).
