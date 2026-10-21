# Getting started

Run commands from the Scope repository root. See [development](development.md)
for toolchain and dependency setup; the first build may download dependencies.
No model server, API key, or Wasm target is needed for the default shell.

## Start and inspect

```sh
cargo run
```

DX4 defaults to package `scope-workbench`, binary `scope`, depending on
`scope-ide` and `scope-script`. No arguments starts the workbench shell in the current directory, including with piped
input. Startup requires an existing workspace and does not create directories.
Workspace access defaults to read-only. In-memory editing is not permission to
save: opt in with `cargo run -- --writable shell` to enable workspace writes.
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

## In-memory Script loop

Paste this exact sequence at the shell prompt (the script is also saved as
[`examples/workbench.scope`](../examples/workbench.scope)):

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

On a TTY, `:scope edit` opens a raw multiline editor. Use arrows/Home/End,
Enter, Backspace/Delete, Tab, and bracketed paste to edit existing source.
Ctrl-S accepts the draft; Ctrl-C or Escape cancels. See the
[short editing tutorial](scratch-editing.md).

The transcript explicitly uses `:scope edit --commands`, the legacy cooked-line
editor (also the default for non-TTY input). Ordinary lines insert literally with
a newline. Within command mode:

- `:at LINE COLUMN` moves the cursor (one-based Unicode scalar positions).
- `:delete COUNT` deletes forward, including newlines.
- `:insert TEXT` inserts at the cursor; escapes are `\n`, `\t`, and `\\`.
- `:complete` lists source completions; `:complete INDEX` applies a current item.
  Completion uses fresh Script metadata with no live bindings and does not run
  the draft or grant authority.
- `:show [START]` previews a numbered page of source.
- `:done` commits to the in-memory buffer; `:cancel` or EOF discards the draft.

Accepting a changed draft creates exactly one revision; accepting unchanged text
or cancelling creates none. Neither mode saves or evaluates source.

For example, reopen `:scope edit --commands`, use `:at 1 50`, `:delete 1`, and
`:insert 8` to change the final sample from `6` to `8`; finish with `:done`
and rerun `:scope script run` to take the `unexpected` branch.

`:scope scratch list` shows scratch buffers; `:scope context` distinguishes the
selected buffer from the App document. Editing and running never implicitly
save source, write results, or write a report. Opening a `.scope` file grants no
authority; its suffix does not install a host or downstream language runtime.

`:scope run` remains the configured downstream semantic runner, separate from
`:scope script run` and Scope Simple analysis (`scope.simple`). Downstream
hosts must explicitly install and authorize their operations.

## Exact CLI forms

```sh
cargo run -- --help
cargo run -- open .
cargo run -- --workspace . shell
cargo run -- --workspace . --writable shell
cargo run -- --workspace . list
cargo run -- --workspace . show README.md
cargo run -- --workspace . capabilities
cargo run -- --workspace . authority
cargo run -- --workspace . meta --max-entries 10000 --max-depth 64
```

`open` and `shell` are aliases. An explicit workspace **without a command** lists
files (`cargo run -- .` or `cargo run -- --workspace .`); only the no-argument
invocation defaults to the shell. Put workspace options before the command.
Metadata budgets follow `meta`: entries must be positive; depth may be zero.
The defaults are 10,000 entries and depth 64.

The workbench executes pure Script via `:scope script run`, not by treating
ordinary shell input as Script. Interactive evaluators for other languages
require explicit host registration, then selection with `:scope session <id>`.
With a session active, Scope commands need the `:scope` prefix; other input is
language source. See [Interactive Sessions](../INTERACTIVE_SESSIONS.md).

## Daily Script loop

```sh
cargo run -p scope-script -- crates/scope-script/examples/daily.scope
cargo run -p scope-script -- --output summary crates/scope-script/examples/daily.scope
```

This standalone path reads saved files: edit, explicitly save, rerun. It remains
a no-host alternative to the in-memory workbench loop. See
[Scope Script](scope-script.md) for output modes and explicit host examples. Return to the [README](../README.md) for the five-minute path.
