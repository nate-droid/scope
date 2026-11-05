# Scope shell and workflow reference

Start with the [canonical quickstart](docs/quickstart.md) for the normal `cargo run`
workbench. This longer reference explicitly launches the shell for its transcripts.
The retained workspace is now the default, not an optional launch mode.

This tutorial covers the no-host Script loop, revising scratch source, and the
retained workspace. Shell transcripts go at the Scope prompt; `cargo`
commands go in your system terminal. Do not paste the Markdown fences.

For direct code entry with automatic pure evaluation, start with the short
[continuous Script tutorial](docs/continuous-script.md). The explicit editing/run
workflow below remains available.

- [Start and inspect](#start-and-inspect)
- [Enable the optional agent](#enable-the-optional-agent)
- [In-memory Script loop](#in-memory-script-loop)
- [Loops and reusable computations](#loops-and-reusable-computations)
- [Editing and rerunning](#editing-and-rerunning)
- [Interactive workspace](#interactive-workspace)
- [Exact CLI forms](#exact-cli-forms)
- [Saved Script files](#daily-script-loop)

Run commands from the Scope repository root. See [development](docs/development.md)
for toolchain and dependency setup; the first build may download dependencies.
No model server, API key, or Wasm target is needed for this shell workflow.

## Start and inspect

```sh
cargo run --locked -- shell
```

The default entry point uses package `scope-workbench`, binary `scope`, depending on
`scope-ide` and `scope-script`. No arguments starts the retained workbench editor.
Explicit `shell` starts the shell in the current directory, including with piped
input. Startup requires an existing workspace and does not create directories.
Workspace access defaults to read-only. In-memory editing is not permission to
save: opt in with `cargo run --locked -- --writable shell` to enable workspace writes.
Put `--writable` before the command. `meta` always uses a read-only mount, even
with that flag. Host integrations can configure different authority explicitly.
The `tools` feature is enabled by default, but does not grant write authority.

```text
help
languages
:scope session
:scope context
files
capabilities
authority
open README.md
show
language
dirty
quit
```

`capabilities` lists installed interfaces; `authority` displays advisory policy
metadata. Neither command authorizes an operation. `open` above selects an
App-owned document. `:scope open` selects a separate shell buffer; `context`
shows the owners and command targets. Do not assume they refer to the same text.
Use `help` for buffer, proposal, and explicit authorization commands.
`languages` lists registered analysis services, semantic operations, evaluator
providers, and the active session. The stock build has Scope Simple analysis
(`scope.simple`) and Language Neutral fallback (`scope.neutral`), but no semantic
operations or interactive evaluators. `:scope inspect` discovers eligible reads
for an open App document; an empty list is not a missing authorization grant.

## Enable the optional agent

Agent support is compiled out by default. Enable it with Cargo's `--features agent`
**before** the `--` that separates build options from Scope options:

```sh
cargo run --locked -p scope-workbench --features agent -- \
  --agent-model qwen3-30b-a3b \
  --agent-base-url http://localhost:1234/v1 \
  shell
```

Start your OpenAI-compatible model server first. At the Scope prompt, use
`agent summarize this project` (or `ask summarize this project`). For a one-shot
request, replace `shell` above with `agent 'summarize this project'`.

There is no `--agent` switch: `agent` is a command. No `--allow-read` flag is
needed; workspace access is read-only by default. Enabling the feature does not
grant write authority. Source and tool feedback may be sent to the configured
model endpoint; see the [Agents guide](docs/agents.md) for policy details.

For the standalone CLI, use `-p scope-ide --features agent` instead. Add
`--offline` to Cargo only if dependencies are already cached. Running
`target/debug/scope` directly uses whichever features were compiled into that
binary; rebuild it with `cargo build --locked -p scope-workbench --features agent`
if needed. Runtime model flags cannot enable a missing build feature.

## In-memory Script loop

If you used `quit` above, start `cargo run --locked -- shell` again. Paste this exact
sequence at the shell prompt (the script is also saved as
[`examples/workbench.scope`](examples/workbench.scope)):

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

The run prints a record with name `daily`, status `ok`, and total `12`, followed
by a final value and `status: interpreter success`. Every run uses a fresh
interpreter with `NoHost`: no persistent bindings, providers, workspace grants,
or semantic target. This is interpreter success, not downstream acceptance.

Continue with [editing and rerunning](#editing-and-rerunning) below, or try the
[interactive workspace](#interactive-workspace) for retained panes and result history.

`:scope scratch list` shows scratch buffers; `:scope context` distinguishes the
selected buffer from the App document. Editing and running never implicitly
save source, write results, or write a report. Opening a `.scope` file grants no
authority; its suffix does not install a host or downstream language runtime.

`:scope run` remains the configured downstream semantic runner, separate from
`:scope script run` and Scope Simple analysis (`scope.simple`). Downstream
hosts must explicitly install and authorize their operations.

## Loops and reusable computations

For a saved-file utility, try this source in a Script scratch or a `.scope` file:

```text
fn total_minutes(tasks) {
    let mut minutes = 0
    for task in tasks {
        if !task.done {
            minutes = minutes + task.minutes
        }
    }
    minutes
}
let checklist = [
    {name: "test", done: true, minutes: 10},
    {name: "review", done: false, minutes: 25}
]
print({remaining_minutes: total_minutes(checklist)})
```

It prints `{remaining_minutes: 25}`. `for` visits lists in order, with fresh
locals each iteration. `let mut` explicitly allows assignment; ordinary `let`
is immutable. Assignment keeps the same runtime value kind. Function parameters
are immutable and operations check their argument types at runtime, without
coercion. A function returns its final expression; `if` and `for` themselves
return `null`. Names cannot shadow outer bindings. Functions snapshot visible
values when declared, cannot mutate caller bindings, and cannot recurse.

Run the three complete, host-free examples from the repository root:

```sh
cargo run --locked -p scope-script -- crates/scope-script/examples/collection-processing.scope
cargo run --locked -p scope-script -- crates/scope-script/examples/reusable-computations.scope
cargo run --locked -p scope-script -- crates/scope-script/examples/release-readiness.scope
```

The last is a roughly 50-line release checklist report with structured inputs,
reusable summaries, and actionable pending work. Edit its input records for your
own checklist. The same sources work through `:scope script run`; no semantic
host is required. Functions exist only within one submitted evaluation, so use
a saved file or scratch for multiline programs rather than separate REPL lines.
See [language rules and limits](docs/scope-script.md#local-computation-loops-and-functions)
for capture, return-value, typing, and budget details.

## Editing and rerunning

Start `cargo run --locked -- shell` in a terminal (or use your existing shell). For this
exercise, create a **new** scratch at the shell prompt with
`:scope scratch new script`, then `:scope edit`.

1. Type `let n = 6;`, press Enter, then type `n * 7`.
2. Press Ctrl-S to accept the draft. This changes memory, not disk.
3. Run `:scope script run`; the final value is `42`.
4. Reopen `:scope edit`. Existing source is still there: navigate to the `6`
   with arrows/Home/End, delete it with Delete or Backspace, and type `8`.
   Do not retype the whole program over the existing text.
5. Press Ctrl-S and run again; the final value is `56`.
6. Reopen, make a change, then press Ctrl-C or Escape. Run again: the accepted
   source and its revision are unchanged.

The editor uses an inline viewport of up to 12 terminal rows, scrolling to follow
its cursor. Printable ASCII is shown literally; Unicode and tabs are displayed as
safe ASCII escapes so cursor placement does not depend on terminal character
widths. The underlying source is unchanged, and navigation/deletion still uses
Unicode scalars, not the individual characters of the displayed escape.

Enter inserts a newline; Tab inserts a tab. Bracketed multiline paste inserts
source, including colon-leading text, without executing shell or editor commands.
Ctrl-S accepts, not saves or evaluates. An accepted changed draft creates one
revision regardless of keystroke count; an unchanged draft creates none.

### Replace one line without opening the editor

`:scope line N JSON_STRING` replaces one complete line in the selected shell
buffer. `N` is one-based; the replacement uses JSON quoting, not shell quoting.
For example, `:scope line 1 "  let n = 8;"` preserves the two leading spaces;
`""` leaves an empty line. Escaped tabs, quotes, and backslashes are supported,
but decoded CR/LF, invalid numbers, nonexistent lines, and extra arguments fail
without editing.

Existing LF/CRLF terminators and all bytes outside the line are preserved. A
trailing LF includes a final empty line; empty source has one empty line. Each
successful replacement advances the buffer version once, even for identical
text, without changing identity or saving. This command targets the shell buffer,
not an App document with the same path.

### Explicit command mode

Use `:scope edit --commands` for the legacy Enter-submitted editor. Non-TTY
`:scope edit` selects this mode automatically. For example, in an empty buffer:

```text
:scope edit --commands
let n = 6;
n * 7
:done
```

To revise it, reopen with `:scope edit --commands`, then enter:

```text
:at 1 9
:delete 1
:insert 8
:show
:done
```

`:at LINE COLUMN` uses **one-based Unicode scalar** positions, not UTF-8 bytes,
grapheme clusters, or terminal display cells. `:delete COUNT` deletes scalars
forward, including newlines. `:insert` supports `\n`, `\t`, and `\\` escapes.
`:show [START]` previews source; `:complete` lists metadata-only completions and
`:complete INDEX` applies one. `:cancel` or EOF discards the command-mode draft.
These commands are not keybindings in the raw editor.

Command mode starts at the beginning and **inserts**, rather than replacing the
old program when you reenter source. Inspect with `:show` before revising.
Repeated diagnostics can result from duplicated source, not parser state; inspect
the complete draft before inserting it again.

Editing does not grant save authority, run providers, or accept agent proposals.
Use explicit save and evaluation commands separately. See
[Interactive Sessions](docs/integration.md) for details.

## Interactive workspace

The normal executable also provides an opt-in retained editing workspace (default
`tui` feature). This is separate from the legacy `tui` browser and does not
replace the shell or existing CLI:

```sh
cargo run --locked -- workspace .
```

Use a real interactive terminal. Startup selects an empty `text` scratch, focuses
the editor, and leaves the semantic target unbound. Nothing runs or saves.

### App document: open, run, check, save

**Ctrl-O** opens an App document by path. Edit it, then press **Ctrl-R** to
publish the exact focused draft in memory using its checked baseline. Run selects
exactly one eligible zero-argument, read-only document `CandidateEvaluation`
operation and directly invokes it: the trusted native-host Run gesture authorizes
the exact captured operation, source identity, version, text, language and empty
arguments once. No extra Enter is required. Missing or ambiguous eligible
operations are not auto-selected; capability-requiring operations are not a
fallback. There is no fallback to the selected scratch.

`Read` is a declared effect, **not a sandbox**: native language providers remain
trusted host code. This gesture grants no ambient session or agent authority;
agents retain their separate allowlist. Advanced **Evaluate / inspect** still
requires explicit capture/review and confirmation before invocation.

**Shift-F5** publishes and checks diagnostics, leaving editor focus in place.
Neither Run nor Check saves to disk. **Ctrl-S remains Publish**, not disk Save;
use **Ctrl-G** → **Save** for explicit persistence. Ctrl-G opens actions/help;
**F1** remains an alias. The primary catalog still exposes advanced actions.

Opening an App document from the launcher's conventional default split selects
the editor-only layout, with a small output strip and a short Open / Run / Quit /
More footer. Custom layouts are preserved; **F7** still cycles layouts. Direct Run
leaves editing in place and shows the provider's evaluation summary, or a generic
completion message for other result types. Source changes and unpublished drafts
mark retained output stale. Summaries are not truncated, so a verbose provider can
still fill the strip; focus output to scroll. Use **Ctrl-G** → **Evaluate / inspect**
for retained evidence and **d** for Details in the modal. Scope does not parse
provider prose or opaque JSON to infer success or certification.
Esc cancels pending modal confirmation first, then closes the modal on a subsequent
Esc. Outside a modal, Esc stays in the App editor workspace; **F2** opens the
advanced shell. Scratch/legacy Esc still hands off to the shell.

**Ctrl-Q** warns if editor drafts, the App document or the selected scratch are
dirty. A second Ctrl-Q discards and exits; **Esc** cancels quitting. This is not an
automatic save. See the [UX5 quiet workspace status and evidence limits](docs/ux/quiet-workspace.md)
for what has and has not been verified.

### Edit, run, inspect, correct (Script scratch)

Press **F1**, type `New Script`, then Enter to invoke **New Script scratch
(select and edit)**. This explicitly selects a new `scope-script` scratch; the semantic
target stays unchanged. No shell command entry is needed for this loop.
F1 filters action labels and stable IDs; Up/Down selects, Enter invokes, and
Escape cancels without inserting text or leaving the workspace. Disabled actions
show their reason. Creation focuses the first visible authorized editor, even
when invoked from output. It never grants editing authority to a read-only pane.

In the editor, type or bracketed-paste this tested Script syntax:

```scope
print("script雪😀"); 42
```

Press **Ctrl-S** to commit to the selected scratch in memory, then **F5** to run.
Typing changes only local working text; Ctrl-S publishes the whole draft as one
scratch revision (unchanged text adds no revision). Neither key saves to disk or
marks source semantically validated. The editor keeps its cursor and focus, so typing can
resume immediately. An uncommitted change blocks execution rather than being
silently committed or run.

The structured output shows source identity/revision, interpreter status,
`print 1: "script雪😀"`, and `final: 42`. It labels whether the result belongs to
the current committed revision, an older revision, or an unavailable source.
Each run uses a fresh pure interpreter, not an installed downstream runtime.
Interpreter success is **not** semantic approval.

Use F1 → **Focus output**, then Up/Down for individual lines or PageUp/PageDown.
**F3** returns to the previous pane immediately. In the editor, move End, append
` + 1`, Ctrl-S, and F5: the new final value is `43`. **Alt-Left/Alt-Right** selects
previous/next retained direct results (also available by name in F1). The first
result still says `42` and is now marked **older committed revision**. Its captured
source, revision and prints never change. F3 returns to editing without changing
scratch selection or semantic binding.

For failure/recovery, append `; print("before failure"); missing`, commit and run.
The result retains earlier prints, then reports `unknown binding` with the source
line, Unicode-scalar column and byte offset. Remove the suffix to recover. An
incomplete expression such as `print(` instead gives a parsing failure; parsing
can fail before any prints execute. Error details scroll with the result.

F4 → `:scope script run` remains supported and uses the separate command
transcript. F1 → **Show command transcript** returns to that view; direct history
is still retained. In the Script scratch context, F5 never invokes a bound semantic target.

### Local editing, selection and history

In the focused workspace editor, **Ctrl-Z** undoes and **Ctrl-Y** redoes local
edits. This is bounded, disposable working-text history, not scratch revision
history: undo/redo does not publish a revision, save a file or change a retained
result. Ctrl-S still publishes the whole changed draft as one scratch revision;
use it explicitly after undo/redo if you want to publish the resulting text.

Use **Shift-Left/Right/Up/Down** or **Shift-Home/End** to extend a selection.
Typing or pasting replaces the selection; **Delete** or **Backspace** removes
it. Unshifted navigation clears selection (Left/Right collapse it to the
corresponding edge). Left/Right movement and ordinary character deletion respect
extended Unicode grapheme clusters, such as a letter plus combining accent or a
joined emoji, rather than stepping through their individual code points.

A clean editor follows newer committed source on redraw and refreshes its baseline
before the next edit; that external refresh resets local undo/redo history.
Unpublished text never refreshes automatically: if another operation changes its
baseline, Ctrl-S reports a conflict and retains the draft. There is no automatic
rebase or merge. **Ctrl-C discards local working text**, including its local
history; it is not Copy. Copy any work you need to keep using your terminal's
copy facility **before** discarding, especially when resolving a conflict.
Proposal acceptance is blocked while any editor draft is dirty, including parked
drafts; publishing a draft instead makes older proposals stale. No automatic
proposal acceptance is involved.

Shift selection requires the terminal to send distinct modified-key sequences;
some terminal emulators or OS shortcuts intercept Shift-arrows/Home/End. The
editor does not provide a system clipboard copy binding. Bracketed paste requires
terminal support and must reach the focused editor: it inserts source literally,
including multiline and colon-leading text, replacing any selection rather than
executing commands. Without bracketed-paste framing, pasted input may be decoded
as individual keys; do not assume equivalent literal insertion or undo grouping.
F4 command input has its separate single-line limitations below.

### Switch scratches without confusing the target

F4-submit `:scope scratch list` and note the actual reported IDs; do not assume
allocation order. Create another scratch with `:scope scratch new text`, type an
uncommitted note, then F4-submit `:scope scratch select ID` using the Script's
reported ID (replace `ID` with the number). The editor restores that scratch's
draft. A Script run is blocked while **any** editor draft is dirty, including the
note you left behind. F1 offers **Select blocking draft** actions with scratch,
source and revision details; invoking one explicitly changes selection and focuses
its visible editor. Ctrl-S commits or Ctrl-C cancels. Then explicitly select the
Script again and run. Hidden editor panes and closed scratches show disabled
reasons. Custom panes without draft metadata can only identify the blocking pane.
This conservative rule is shared by F5 and F4 Script runs; it has not been relaxed
to a source-specific guard. These operations never implicitly save.

Selection controls this workspace editor, not the semantic target. F4-submit
`:scope context` to inspect ownership. If a downstream integration requires a
semantic target, its explicit binding command is `:scope semantic-target ID`;
binding alone does not install providers or authorize evaluation. The stock pure
Script runner has no downstream authority, even with a target bound.

### Controls and command-line limits

| Control / F4 text | Effect |
| --- | --- |
| Ctrl-G / F1 | Search workspace actions/help; Escape cancels; disabled actions explain why. |
| Ctrl-O | Open an App document by path. |
| Ctrl-R | Publish focused App draft and directly invoke the unique eligible candidate evaluation once. |
| Shift-F5 | Publish focused App draft and check diagnostics; retain editor focus. |
| F5 | App editor: same direct Run as Ctrl-R; Script scratch: run selected committed Script. |
| F3 | Focus previous visible pane. |
| Alt-Left / Alt-Right | Previous / next retained direct Script result. |
| F4 | Open command input; Enter submits; Escape or F4 cancels without dispatch. |
| `help` or `pane help` | Workspace control help. |
| `pane editor`, `pane inspector`, `pane output` | Focus a visible named pane. |
| `pane last` | Previous visible focus. |
| `pane 1`, `pane 2`, … | One-based visible pane index; invalid/hidden targets report errors. |
| F6 / Shift-F6 | Next / previous visible pane. |
| F7 | Cycle workspace and editor-only layout; not scratch tabs. |
| Ctrl-S / Ctrl-C in editor | Commit / discard draft; neither writes disk. Ctrl-C is not Copy. |
| Ctrl-Z / Ctrl-Y in editor | Undo / redo local edits, not committed revisions. |
| Shift-arrows / Shift-Home / Shift-End in editor | Extend selection; typing/paste replaces it, Delete/Backspace removes it. |
| Up / Down, PageUp / PageDown with output focused | Scroll current structured result or command transcript independently of editor. |
| F2 | Suspend workspace for the advanced shell; `:scope quit` returns. |
| Escape outside command input/modals | App editor: stay in workspace; scratch/legacy: suspend for shell. |
| Ctrl-Q outside command input/modals | Warn if drafts/App/selected scratch are dirty; second Ctrl-Q discards and exits, Esc cancels. |

F4 forwards existing single-line `:scope` commands unchanged, including
`:scope scratch list`, `:scope scratch select ID`, and `:scope script run`.
Bare `script run` and ordinary Script expressions are not workspace commands.
Multiline/CR submissions are rejected, as is `:scope edit` block entry: use the
workspace editor, or F2 and the existing shell editor. The command input supports
characters, Backspace and paste, not the full shell's history/completion/cursor
editor. Plain Tab and `q` remain pane input rather than workspace navigation.

F2 handoffs retain this launcher's scratches; shell commands may explicitly
change them. Workspace and shell share terminal ownership sequentially, never
read concurrently. The [workspace PTY scenarios](crates/scope-workbench/tests/pty_workspace.py)
exercise these transitions. F1/F5 are tested
with xterm sequences; terminal emulators or OS shortcut mappings may intercept
function keys. The same actions can be selected by name through F1.

## Exact CLI forms

```sh
cargo run --locked -- --help
cargo run --locked -- open .
cargo run --locked -- --workspace . shell
cargo run --locked -- --workspace . --writable shell
cargo run --locked -- --workspace . list
cargo run --locked -- --workspace . show README.md
cargo run --locked -- --workspace . capabilities
cargo run --locked -- --workspace . authority
cargo run --locked -- --workspace . meta --max-entries 10000 --max-depth 64
```

`open` and `shell` are aliases. An explicit workspace **without a command** lists
files (`cargo run --locked -- .` or `cargo run --locked -- --workspace .`); only the no-argument
invocation defaults to the retained workbench. Put workspace options before the command.
Metadata budgets follow `meta`: entries must be positive; depth may be zero.
The defaults are 10,000 entries and depth 64.

The workbench executes pure Script via `:scope script run`, not by treating
ordinary shell input as Script. Interactive evaluators for other languages
require explicit host registration, then selection with `:scope session <id>`.
With a session active, Scope commands need the `:scope` prefix; other input is
language source. See [Interactive Sessions](docs/integration.md).

## Daily Script loop

```sh
cargo run --locked -p scope-script -- crates/scope-script/examples/daily.scope
cargo run --locked -p scope-script -- --output summary crates/scope-script/examples/daily.scope
```

This standalone path reads saved files: edit, explicitly save, rerun. It remains
a no-host alternative to the in-memory workbench loop. See
[Scope Script](docs/scope-script.md) for output modes and explicit host examples. Return to the [README](README.md) for the five-minute path.
