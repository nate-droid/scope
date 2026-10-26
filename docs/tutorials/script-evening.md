# An evening with continuous Script

A short, local workshop: write a little program, inspect its source, revise it,
then decide explicitly what to run and save. No model, API key, semantic provider,
or remote service is needed. The stock workbench supplies the pure Script runner
and local reducer.

## Start here

From the repository root, with the toolchain in `rust-toolchain.toml`:

```sh
cargo build --locked -p scope-workbench
cargo run --locked --offline -p scope-workbench -- shell
```

The initial build may need network access for dependencies; subsequent commands
can run offline once they are cached. At the shell prompt:

```text
:scope script enter
```

You now have a `script>` prompt and an in-memory Script scratch. Paste commands
without prompt labels. **Each numbered exercise is independent:** start it with
`:scope scratch new scope-script` as shown, while in Script context. This selects
an empty scratch without destroying earlier ones. If a draft is pending, first
use `:cancel`. To start an exercise in a new process, launch the shell and enter
Script context first.

Three different actions matter:

- **Evaluate:** complete pure input commits source and evaluates the entire scratch
  in a fresh interpreter. `:run` explicitly evaluates that same full snapshot.
- **Edit/accept:** editing source or accepting a proposal changes memory only;
  neither executes nor saves it.
- **Save:** a separate command writes committed source to an authorized destination.

There is no persistent runtime environment: earlier declarations work because
source accumulates and is replayed, not because variables survive evaluation.

## 1. Reassign, don't redeclare

```text
:scope scratch new scope-script
let mut n = 6;
n = 8;
n * 7
:show
:run
:script evidence
```

The final expression yields `56`; the explicit run yields `56` again, not a
cumulative update. `:show` inspects current source; `:script evidence` inspects
the last execution snapshot, prints, result and outcome without running again.
Evidence can describe an older revision after an edit.

`let` is immutable; `let mut` permits assignment of the same runtime kind.
`n = 9` is assignment; a second `let n = 9` is a forbidden redeclaration.
There is no indexed or field assignment. Try Tab after `n` or a longer binding
prefix: completion reads source, not live interpreter state.

## 2. A function and a multiline expression

```text
:scope scratch new scope-script
fn double(x) {
  x * 2
}
double(21) + 1
if true {
  double(6)
}
```

The call expression yields `43`; the final conditional yields `12`. Open braces
produce the continuation prompt `...>`; closing the construct commits it.
Functions are named top-level declarations, not first-class function expressions.
Their final unbound expression is the result: there is no `return` keyword.
Parameters and captured values are immutable; captures snapshot values when the
function declaration executes. Recursion is unsupported.

Try an unfinished draft:

```text
if true {
:show
:run
:cancel
```

`:show` labels committed source separately from the pending snapshot. `:run` is
blocked while input is pending; `:cancel` drops only that draft. Nothing here
saves source. Continuous entry inserts statement separators between submissions;
it is not a source formatter.

## 3. Errors and a repair that does not run

```text
:scope scratch new scope-script
1 + )
:show
1 / 0
:show
:edit --commands
:at 1 1
:delete 5
:insert 1 / 1
:done
:show
:script evidence
:run
```

The syntax-invalid first line is rejected without committing it. The division
by zero is syntactically valid, so it **remains in the scratch** after its runtime
failure. The command editor replaces those five characters with `1 / 1`.
`:done` keeps the draft in memory; it does not evaluate or save. Evidence still
shows the failed execution until `:run` produces `1`.

Editor coordinates are one-based lines and Unicode-scalar columns. `:delete`
deletes forward; `:insert` accepts `\n`, `\t`, and `\\` escapes. Inside this editor,
`:cancel` discards the editing draft. Plain `:edit` on a TTY opens the keyboard
editor instead (Ctrl-S keeps changes; Ctrl-C/Escape cancels).

A syntax error inside multiline input discards the whole pending construct;
start that construct again. Runtime errors do not roll back earlier output or
host effects. Use a fresh scratch if you want to abandon a broken program.

## 4. Compact: preview, reject, then accept

Use exactly this small program, with no earlier expressions in its scratch:

```text
:scope scratch new scope-script
let mut n = 6;
n = 8;
n = 12;
n
:compact
```

The result before compaction is `12`. Compaction prints a rule, checked
preconditions, a diff and a **pending source proposal ID**. It proposes changing
the initializer to `12` and removing the assignments, retaining the final `n`.
It neither edits nor evaluates the source yet.

In the following commands, replace `ID` with the actual printed number (do not
literally type `ID`):

```text
:source-proposal ID
:source-diff ID
:source-reject ID
:show
:compact
```

Rejection leaves source unchanged. Read the **new** proposal ID, then:

```text
:source-diff ID
:source-accept ID
:show
:script evidence
:run
```

Acceptance revalidates the target and revision, then changes source in memory
only. The evidence still belongs to the earlier execution; run the accepted
source explicitly to get fresh evidence and `12`. Editing the original scratch
before acceptance makes the proposal stale, even if you restore the same text.
Switching selection does not retarget a proposal: acceptance still edits its
original scratch. This is an **unevaluated local rule**, not validation evidence
or semantic acceptance.

### See a refusal

```text
:scope scratch new scope-script
let mut n = 6;
n = n + 2;
n
:compact
:show
```

The program evaluates to `8`, but compaction refuses without changing it. The
experimental reducer supports only a whole program consisting of one mutable
binding initialized with a nonnegative decimal integer literal, one or more
consecutive same-binding decimal-literal assignments, and a final bare read of
that binding. It is limited to 4096 source bytes and 64 statements. Intermediate
reads, arithmetic right-hand sides, calls/output, branches, loops, functions,
negative or parenthesized literals, and extra statements are not supported.
`:compact` also refuses while multiline input is pending. Refusal is not a claim
that your program is wrong; this is not a general optimizer.

## 5. Output requires an explicit run

```text
:scope scratch new scope-script
let n = 8;
print(n);
n + 1
:run
:script evidence
:run
```

Both appended lines after the declaration report that a run is required. Neither
implicitly prints anything: the full snapshot now contains `print`. Each explicit
run prints `8` and yields final value `9`, replaying the whole program. A program
ending with `print(n)` instead yields `null`, the real result of `print`.

The effect check is conservative: even `print` or `scope.*` calls in unused
functions or branches require explicit execution. `scope.*` needs separately
installed, authorized host operations; the stock workbench grants no semantic
authority. Use `print` here—no providers are required.

## 6. Save a small program deliberately

Saving is optional. In a **separate terminal**, from the repository root, create
a disposable workspace and an existing destination (do not substitute a file you
want to keep). These commands assume the directory does not already exist:

```sh
mkdir script-evening-workspace
touch script-evening-workspace/evening.scope
cargo run --locked --offline -p scope-workbench -- --workspace script-evening-workspace --writable shell
```

In this new writable shell:

```text
:scope script enter
:scope scratch new scope-script
let n = 6;
n * 7
:scratch save-as evening.scope
:show
:script leave
:scope script enter
:quit
```

The saved source evaluates to `42`, but saving itself does not run it. Paths are
workspace-relative; destinations must already exist. `save-as` is a checked
replacement, not file creation. Subsequent `:scratch save` writes to the bound
destination; external-change and workspace-authority checks still apply. The
normal launch is read-only and rejects saves. Pending input is not saved.

Leaving Script context retains committed scratch source; returning selects it
again. Quitting never implicitly saves, and unsaved scratches are lost. Reopen
the saved file in a later shell with `:scope scratch open evening.scope`, then
`:scope script enter` and `:run` (using that same workspace root).

For more detail, see [continuous entry](../continuous-script.md) and the
[current language reference](../scope-script.md). Older workshop material may
predate mutation, functions, comments, or continuous entry; use the syntax here
for these exercises.
