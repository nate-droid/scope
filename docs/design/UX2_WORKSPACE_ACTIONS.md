# UX2: actions, committed Script and retained results

This records the implemented normal `scope workspace [path]` executable, not an
IDE framework proposal. [Tutorial](../../TUTORIAL.md#interactive-workspace).
UX1's assessment remains the historical command-entry baseline.

## Reconciliation and interfaces

- `scope-workbench/src/main.rs` installs one `WorkbenchRunner` through the existing
  CLI entry point. `scope-ide/src/cli/workspace_ui.rs` creates caller-owned
  `ScratchBuffers`, `InteractiveSession`, registered editable editor, read-only
  inspector and named layouts. Startup still selects an empty `text` scratch.
- `WorkspaceTui` owns the alternate screen and the single input loop. Panes retain
  local state. `register_editable` is the explicit source-edit capability; pane
  names do not confer execution rights. Focus only changes keyboard routing.
- F4 still uses `WorkspaceCommandOutput::submit`, the existing single-line shell
  grammar and dispatcher. F2 drops the terminal guard and borrows the normal shell;
  it does not create another reader. CLI, standalone Script and shell entry points
  remain intact.
- `ScratchEditAccess` commits against exact owner identity, revision and text
  baseline. Ctrl-S updates that baseline while retaining cursor/preferred column.
  Dirty and parked drafts remain pane-local. No save or evaluation occurs on edit
  or commit. An unchanged commit creates no revision.
- `WorkspaceAction` is a bounded enum; `ActionInfo` supplies stable ID, label,
  shortcut and disabled reason. `WorkspaceTui::actions` derives metadata from live
  selection, visible pane instances, editable opt-in, dirty state, history and host
  support. `invoke_action` rechecks availability. There is no registry, recursive
  command evaluation, shell-string construction or general plugin API.
- `ReadOnlyCommand::supports_direct_script` defaults false;
  `invoke_script_direct(&ScratchBuffer)` defaults denied. This is deliberately
  separate from legacy callbacks that can receive a semantic target. Existing
  downstream integrations do not acquire an F5 execution path automatically.
  Trusted native host callbacks remain trusted code, not a security sandbox.
- The stock callback validates `scope-script`, calls existing `WorkbenchRunner::run`
  with a fresh interpreter and `NoHost`, and returns `ScriptPresentation`. The
  runner retains the original `PureRunRecord`: owned `Snapshot`, ordered `Value`
  prints and `Result<Value, Error>`. The UI retains an owned formatted projection,
  not a parsed transcript or second evaluator. Final values and errors are distinct
  fields. Errors retain original byte offset and one-based Unicode-scalar location.

## Direct workflow and result ownership

F1 searches a small action list. It includes New Script scratch (explicitly
selecting it), run, visible pane focus, previous focus, focused editor commit,
blocking draft selection, result navigation, result/transcript view and help.
Disabled items remain visible with reasons and cannot be invoked. Escape cancels
without handing off the terminal or inserting the query into source. Paste is
never evaluated as an action query or command.

F5 runs **only the selected committed `scope-script`**. It neither reads a bound
semantic target nor commits, discards or saves any draft. Editor focus, cursor,
layout, scratch selection and semantic target are preserved. F1 Focus output and
F3 previous pane provide an immediate inspect/return loop. F6 still cycles focus.

The output view displays one direct result at a time: source identity/revision,
current/older/unavailable attribution, interpreter success/failure, ordered prints,
optional final value and diagnostic location/error. Runtime failures retain prints
already emitted; parse failures may produce none. Up/Down and PageUp/PageDown
scroll output independently of the editor. Alt-Left/Right navigate retained direct
history. Show command transcript and Show retained Script result switch views
without destroying either. F4 submissions select the transcript view.

Current/older attribution resolves the result's original identity across live
scratches, not just the selected scratch. Closing the source reports unavailable.
Source changes never rewrite captured source, revision, prints, value or error.
Interpreter success makes no claim about semantic acceptance or authorization.

## Dirty draft decision

The conservative all-drafts guard remains for both F5 and F4 Script runs.
`Pane::has_dirty_draft` cannot prove which source a custom pane edits, so relaxing
it based on stock-pane metadata would be unsound for existing integrations.
`dirty_draft_sources` is optional explanatory metadata, **not** permission to run.
The stock editor identifies active and parked dirty scratch IDs, source identities
and baseline revisions. A custom pane without metadata is identified by instance.

F1 provides explicit Select blocking draft actions: they select the scratch and
focus its visible editor, without binding a semantic target. Hidden editors and
closed scratches have disabled reasons. Hidden panes require an explicit layout
change first. Closed-source drafts cannot be committed; no automatic recreation
or discard is provided. This limitation is intentional rather than silent fallback.

## Executable evidence

Validation commands:

```sh
cargo test --locked -p scope-ide --features tui --lib
cargo test --locked -p scope-script --lib workbench::tests
cargo test --locked -p scope-workbench
python3 crates/scope-workbench/tests/pty_workspace.py --binary target/debug/scope -v
```

Rust coverage includes default-denied legacy hosts, wrong language/missing source,
selected source versus independent target, dirty selected and unrelated parked
drafts, hidden editor applicability, owned historical attribution after edits and
closure, picker filtering/cancellation, cursor-preserving commits, Unicode source
locations, prints before errors, NoHost and native invocation denial. Existing
layout/tiny-size and input routing tests remain in place.

The stdlib PTY suite launches the real `scope workspace` executable and models
Ratatui screen diffs rather than asserting ANSI-stripped output fragments. Its
direct test creates/selects Script through F1, commits, runs, scrolls, edits, runs
runtime and parse failures, inspects historical results, recovers, resizes to
8×36 and back, returns to editing and exits with restored canonical input/echo.
The legacy F4 test covers unrelated parked drafts and existing shell commands.
A separate F2 test checks shell prompt, `:scope quit`, retained selected scratch,
committed revision and uncommitted draft, then normal terminal restoration.

Observed direct-test trace:

| Measurement | Observed count |
| --- | ---: |
| Workspace/shell terminal-owner handoffs during workflow | 0 |
| F4 command entries/submissions | 0 |
| Picker invocations that execute actions | 5 (new scratch + 4 output focus) |
| F5 run invocations / retained executions | 4 / 4 |
| Result navigation actions | 2 (previous and next) |
| Editor focus departures / returns | 4 / 4 |
| Observed committed Script revisions | 1, 2, 3, 4 |
| Alternate-screen enters / leaves including startup/exit | 1 / 1 |

The canceled picker and attempted disabled dirty-source action execute nothing.
F5 itself causes no editor-focus transition. These are counts in a tested trace,
not latency, throughput or user-study measurements. The separate F2 trace has one
round-trip handoff (two owner transitions), one shell command and two alternate-
screen enter/leave pairs including startup/exit. Both paths restore terminal modes.

## Limits and next milestone

- Execution is synchronous, with no cancellation, background job or timeout. A
  nonterminating Script can still block this UI. Results/drafts are invocation-local
  and retained without a history cap; there is no restart persistence/export.
- Direct history excludes F4 runs, whose diagnostics remain in their transcript.
  There is no diagnostic jump-to-source, source diff, scratch browser or automatic
  focus/layout switch. New Script creation does not force editor focus.
- The picker filters labels/IDs and supports keyboard selection, not fuzzy ranking,
  mouse interaction, query paste or a general palette framework. Narrow terminals
  clip long metadata horizontally; resize to inspect full details.
- Function keys and Alt-arrow sequences are validated as xterm input, not every OS
  shortcut mapping. The small PTY emulator models cursor/erase and Unicode cells,
  not real font rendering, all terminal protocols or visual accessibility. Tiny
  layout coverage checks safety and recovery, not full readability at every size.
- Host support is explicit opt-in, not a new authorization grant. No native semantic
  operations, filesystem permissions or provider completions were added to NoHost.

**Recommended next milestone:** a source-aware draft contract and compact scratch
navigator. The retained unrelated-draft PTY case demonstrates the remaining blocker:
users still need explicit scratch reselection, and custom-pane dirty booleans
cannot establish source-local execution safety. First specify complete dirty-source
coverage (including an explicit unknown state), then narrow the guard only with
proof that every dirty draft belongs elsewhere. Add source-linked diagnostic
navigation on that same identity/revision contract rather than granting authority
through focus. Address bounded/cancelable execution separately before promising
responsiveness for arbitrary programs.
