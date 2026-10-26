# UX1: implemented workspace assessment

This historical assessment records the UX1 command-entry baseline of the normal
`scope workspace [path]` executable, before UX2 direct actions and typed results.
Use [UX2](UX2_WORKSPACE_ACTIONS.md) for the subsequent evidence; descriptions of
missing actions/results below are historical, not current limitations. The existing no-argument
shell, explicit CLI commands, standalone Script runner, and legacy `--tui` remain
available. See the [interactive tutorial](../../TUTORIAL.md#interactive-workspace).

## State and interaction model

| Object | Current contract |
| --- | --- |
| Pane | Persistent registered instance with local state, focus and scroll; instance keys differ from `Pane::id()` type identifiers. Only visible panes receive focus. |
| Layout / tab | Named layouts reuse pane instances. F7 cycles layouts. `View::Tabs` displays its application-selected child; there is no tab bar or interactive tab controller. A tab is not a scratch. |
| Scratch | Caller-owned committed in-memory source with scratch ID, evaluation source identity and revision. The normal workspace starts with an empty selected `text` scratch. |
| Editor draft | Local uncommitted text over a checked baseline. The workspace editor follows scratch selection and retains drafts across scratch switches. Ctrl-S commits in memory; Ctrl-C cancels. Neither saves nor evaluates. |
| Semantic target | Independently bound scratch, initially unbound. Selection does not bind it. Closed explicit targets remain unavailable rather than falling back to another source or App document. |
| Result | Pure runner owns historical source snapshots, ordered printed values and final value/error. The UI displays retained shell transcript bytes, not a typed result browser. |

The inspector sees committed source, not drafts. `ScratchEditAccess::selected`
is the workspace editing path; the separate bound-target access API still exists
for integrations. Identity/revision/baseline checks prevent stale commits. An
unchanged commit creates no revision. Scratch unsaved state and draft dirty state
are distinct. The App document, selected scratch, semantic target and evaluator
session must not be conflated.

## Actual command contract

F4 opens a small command line isolated from pane input; Enter submits, Escape or
F4 cancels. Local actions are `help`, `pane help`, and `pane NAME` (visible key,
one-based visible index, `editor`, `output`, or `last`). This is not a contextual
action registry: there are no discovered action descriptors, applicability menu,
or unified policy-aware action catalog.

Other submissions go through `WorkspaceCommandOutput::submit` and the existing
shell dispatcher. It requires one line starting exactly with `:scope `, rejects
CR/LF and `edit` block entry, and captures ordinary shell output and diagnostics.
It does not add a second command grammar for scratch operations. A successful
submission API return is not evidence of operation success. The TUI specifically
blocks `:scope script run` while **any** registered editor has a dirty draft,
including drafts for other scratches. This is a UI guard, not a general execution
policy enforced by the submission API or the shell.

F2/Escape outside the command line still drops the terminal guard and borrows the
normal shell; `:scope quit` returns. `run_with_sources` retains caller-owned
scratches, selection and semantic binding across that handoff. The older `run`
path does not promise retained shell-local scratches. There are no competing
terminal readers. Plain typing/paste in an editor remains literal source, even
when it looks like a command. Inspector paste does not edit source.

## Friction baseline and counted improvement

The baseline is the existing editable workspace's shell handoff, not a claim that
the CLI was removed or a historical timing experiment. Start with committed Script
selected, all drafts clean, editor focused; finish back in the editor after one
run. Count terminal-owner transitions (alternate-screen workspace to shell and
back), separately from command-mode transitions and source edits:

| Deterministic trace | Terminal-owner transitions | Command-mode transitions |
| --- | ---: | ---: |
| Baseline: F2 → shell `:scope script run` → `:scope quit` → editor | 2 | 0 F4 transitions |
| Current: F4 → `:scope script run` + Enter → editor | 0 | 2 (enter/leave command line) |
| Three repetitions of baseline | 6 | 0 F4 transitions |
| Three repetitions of current | 0 | 6 |

These are counted transitions in the specified traces, **not elapsed-time,
keystroke, throughput, or user-study measurements**. Commit, command text entry,
output scrolling and any corrective edits are excluded equally. F4 avoids terminal
handoff but does not eliminate command entry or modal interaction. Results remain
visible beside the work area rather than disappearing with shell return.

Remaining friction: users must know explicit commands and live scratch IDs; there
is no clickable scratch/tab navigator. Hidden dirty drafts can block a run.
Output is a small flattened transcript, with repeated shell banners and separate
appended UI errors rather than one chronological typed event stream. PageUp/Down
now uses the output pane's inner height, with regression coverage for resizing
and tiny panes. Structured `PureRunRecord` values exist in the runner, but
are flattened for this UI; typed result selection, filtering, stale badges and
source-linked diagnostic navigation are not implemented.

## Execution and authority boundary

The core `WorkbenchRunner` runs the selected committed Script snapshot in a fresh
`Interpreter` with `NoHost` on every invocation. It retains identity, revision,
exact source, ordered prints and `Result<Value, Error>`. Runtime errors retain
prints already emitted; parse failure can precede all evaluation. Diagnostics
show one-based Unicode-scalar line/column and original byte offset. Interpreter
success is not semantic acceptance, validation, approval or downstream success.
No live bindings, filesystem authority, native providers or evaluator session are
installed. A separately bound semantic target does not grant this runner access
to it. `scope.operations()` is empty and native invocations fail.

Authorized downstream execution is **unsupported by the stock pure runner**.
Extension points are not completed downstream functionality: a trusted host must
install the appropriate `ReadOnlyCommand` callbacks/providers, resolve the explicit
source/target snapshot, authorize the concrete operation and retain provenance.
Neither F4 nor layout registration supplies authority. Native Rust callbacks are
trusted code, not sandboxed by the name `ReadOnlyCommand`. No implicit save,
automatic run-on-commit, or permission transfer from source edits exists.

## Evidence and limits

Source anchors: `cli/workspace_ui.rs`, `cli/workspace_command.rs`,
`tui/composition.rs`, `tui/scratch_editor.rs` in `scope-ide`, and
`scope-script/src/workbench.rs`. Relevant executable checks run for this docs pass:

- `cargo test --locked -p scope-workbench --test workspace_commands` — passed.
- `cargo test --locked -p scope-script --lib workbench::tests` — 5 passed.
- `cargo test --locked -p scope-ide --features tui --lib cli::workspace_command` — 2 passed.
- `cargo build --locked -p scope-workbench` — passed.
- `python3 crates/scope-workbench/tests/pty_workspace.py --binary target/debug/scope -v`
  — 1 passed on macOS.

The workspace PTY uses the real executable and a small reconstructed VT screen.
It covers Unicode/bracketed paste, inspector non-editability, F4 cancellation,
named/last focus, retained scratch drafts, all-drafts run guard, resize down/up,
commit/cancel, interpreter failure and corrected rerun, retained output, no
workspace files, and normal Ctrl-Q exit. Its wall-clock runtime is test harness
runtime, not UX performance evidence. It does not assert exact termios restoration;
the separate editable-workspace PTY has that assertion. Neither suite establishes
all-emulator portability. This pass did not test Linux or Windows, forced-kill or
injected-I/O-error recovery, authorized downstream evaluation, or a contextual
action registry. Scalar editing is not grapheme-aware; no claim is made for undo,
merge, persistent draft recovery or dynamic layout editing.
