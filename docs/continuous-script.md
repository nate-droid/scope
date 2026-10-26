# Continuous Script entry

For independent hands-on exercises, start with
[An evening with continuous Script](tutorials/script-evening.md).

Launch `cargo run -- shell`, then enter `:scope script enter`. This selects
continuous Script input and creates an in-memory Script scratch if none is
selected. If another language is selected, first use
`:scope scratch new scope-script`.

## Write and inspect a program

```text
scope> :scope script enter
script> let radius = 5;
5
script> rad<Tab> + 1
6
script> radius * 2
10
script> :show
```

Tab completes `rad` to `radius`. Source grows into ordinary Script:

```scope
let radius = 5;
radius + 1;
radius * 2
```

Each accepted submission updates the same scratch. `:show`, evaluation, and
explicit saving use that actual source, not a presentation-only rendering.
A semicolon is inserted at a submission boundary when needed: newline alone
could continue a call, indexing expression, or `if`/`else`. An existing final
semicolon is reused. A trailing comment without a preceding terminator can
still require a separate semicolon line so the comment does not swallow it.
This is boundary assembly, not a formatter; existing whitespace and comments
are retained.

Every evaluation uses a fresh interpreter and the full snapshot. Values are
concise, but runtime bindings are not kept between runs. Execution evidence
retains the exact snapshot, source identity/revision, prints, result, and outcome.
`:script evidence` inspects the last execution without running again; it can
refer to an older revision than `:show`.

## Completion

Tab uses the same source-completion path as the existing editor. It offers
keywords, builtins, and completed top-level `let`/`let mut` declarations before
the cursor. Editing or renaming a declaration changes completion immediately;
there is no cached symbol index and completion never evaluates code. Colon
commands retain command completion: `:sho<Tab>` completes `:show`.

Block-local bindings and incomplete declarations are not offered. Invalid source
prefixes conservatively contribute no source bindings. While entering a pending
block, completed earlier top-level declarations remain available; completion is
not a full scope/type analyzer.

## Multiline drafts and recovery

```text
script> if radius > 3 {
...>   radius * 2
...> :show
Committed Script source:
...
Pending Script snapshot (not committed; not evaluated):
...
if radius > 3 {
  radius * 2
...> }
10
```

`:show` distinguishes committed source from the entire proposed snapshot,
including the unfinished draft. Inspection neither commits nor evaluates it.
`:cancel` or Ctrl-C discards only pending input, leaving the committed scratch
unchanged. `:run` is blocked until you finish or cancel the draft. Pending input
is tied to scratch identity and revision; after editing or switching sources,
use `:cancel` if a stale-draft error is reported.

A syntax-invalid submission discards the entire pending construct, not just its
last line. Start it again with the correction. Runtime failures differ:
syntactically valid source has already been committed; use `:edit` to repair it.
Neither failure hides diagnostics. Leaving Script context clears pending input.

## Edit without running or saving

Use `:edit` for the existing scratch editor. To make a reproducible command-editor
change to the first example:

```text
script> :edit --commands
1:1> :at 1 14
1:14> :delete 1
1:14> :insert 8
1:15> :done
script> :run
16
```

The editor accepts a change into the same scratch without executing or saving.
The next run reconstructs bindings from the edited source. Use Home/End, cursor
keys, history, and bracketed multiline paste through the normal terminal editor.

## Output and host calls require explicit execution

Any snapshot containing `print` or `scope.*` calls requires explicit `:run`,
even calls in unused functions or branches:

```text
script> print(radius);
Source updated. Run required (:run, full snapshot).
script> :run
8
null
script> :script evidence
```

Each explicit run deliberately replays the entire snapshot, including prints and
authorized host calls. Appending a pure expression does not implicitly replay
older effects; the same run-required notice remains until those effects are
removed. The default workbench has no semantic authority; embedded hosts retain
their existing authorization and target checks.

The trailing `null` is the real result of `print`, not an omitted value. The
execution record does not reliably distinguish incidental statement results
from explicitly requested values, so ordinary presentation preserves it.
Entering `null` explicitly also displays `null`. Evidence preserves exact
results; errors retain diagnostic/provenance output.

## Save, leave, and return

- `:show`: inspect the current committed scratch and, if present, the distinctly
  labeled pending snapshot.
- `:script evidence`: inspect the last execution without another run.
- `:scratch save-as PATH`: explicitly save the actual committed source to an
  authorized workspace destination. Launch with `cargo run -- --writable shell`
  when writes are needed; existing destination/save/promote rules apply.
  Pending input is not saved.
- `:script leave`: return to shell commands, retaining committed source.
- `:scope script enter`: reenter the selected Script scratch.
- `:quit`: exit without implicitly saving source.

The explicit `:scope ...` command interface remains available. Source selection
and semantic target selection remain independent. The standalone `scope-script`
executable is unchanged.

## Recorded actual-PTY session

The existing Python PTY tests ran against `target/debug/scope shell`. This excerpt
removes ANSI sequences, per-keystroke redraws, startup instructions, and editor
prompt metadata; commands and results come from actual terminal reads:

```text
script> let n = 6;
6
script> n * 7
42
script> :edit --commands
Editing draft; :help for commands, :done to keep, :cancel to cancel. Nothing is executed or saved.
1:1> :at 1 9
1:9> :delete 1
1:9> :insert 8
1:10> :done
Returned to REPL (no save or evaluation).
script> :run
56
script> :script leave
Left Script context (scratch retained).
scope> :scope script enter
script> n + 1
9
script> print(n);
Source updated. Run required (:run, full snapshot).
script> :run
8
null
script> :script evidence
```

The retained source is:

```scope
let n = 8;
n * 7;
n + 1;
print(n);
```

## Regression checks

```sh
cargo test -p scope-ide --lib -j 1 --offline --locked
cargo test -p scope-script -j 1 --offline --locked
cargo build -p scope-workbench -j 1 --offline --locked
python3 crates/scope-script/tests/pty_continuous_shell.py -v
# Include ANSI-stripped actual reads, including redraws:
python3 crates/scope-script/tests/pty_continuous_shell.py --transcript -v
```

The stdlib-only PTY suite accepts `--binary PATH`, isolates workspace/history,
and checks fresh output, not a terminal emulator's screen cells. Its five tests
cover navigation/history/paste, completion, pending inspection and cancellation,
invalid input, editing, leave/reenter, effects/evidence, explicit null, and a
20-expression source-growth session. Startup waits up to ten seconds for output
rather than assuming a prompt arrives within 400 ms. The command editor is
covered here; `pty_scratch_runner.py` separately covers the raw scratch editor.

UX3 validation: the IDE library suite passed twice consecutively (391 tests),
the full Script suite passed (including 10 continuous-shell integration tests),
the workbench built, and all five PTY tests passed. A repeated IDE run exposed a
shared test-fixture isolation defect: clock-only temporary directory names could
collide across parallel tests. Fixture names now include process ID and an atomic
counter, with a concurrent creation/cleanup regression test. Builds were serialized and reused artifacts. No cleanup was needed:
free space started at 25 GiB and was 18 GiB after validation (target: 8.4 GiB).
Remaining limitations are conservative top-level completion, occasional comment
boundary separator lines, unchanged incidental `null` presentation, and explicit
full-snapshot replay. No persistent interpreter or second scratch owner is used.
