# Scope Script

Script is an embeddable orchestration language with a standalone CLI and the
DX4 default workbench. Root `cargo run` selects package `scope-workbench`, binary
`scope`, depending on `scope-ide` and `scope-script`.

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
changed; cancellation creates none. See the [editing tutorial](scratch-editing.md).

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
Conditions must be booleans; only the selected branch executes. Script has no
loops or user-defined functions. Host operations use the same structured values,
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
