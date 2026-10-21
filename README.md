# Scope

Scope is an experimental, capability-oriented development environment. Projects
supply language semantics and typed operations; Scope provides discovery,
composition, workspace inspection, and human/agent orchestration. [Cell](https://github.com/nate-droid/cell)
provides WebAssembly execution and capability enforcement.

## Five minutes: Script workbench

From this checkout's root, with the [development prerequisites](docs/development.md):

```sh
cargo run
```

DX4 uses the `scope-workbench` package (binary `scope`), which depends on
`scope-ide` and `scope-script`. This starts the Script workbench shell in the current directory. The workspace must
already exist; startup does not create it. Workspace access is read-only by
default. To enable saves, opt in with `cargo run -- --writable shell`.
At the prompt:

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
:scope quit
```

The transcript uses `:scope edit --commands` for explicit cooked lines; `:done`
commits the draft to memory, not disk. On a TTY, plain `:scope edit` opens the
multiline keyboard editor: arrows/Home/End move, Enter inserts a newline,
Backspace/Delete remove text, Tab inserts a tab, and bracketed paste inserts source.
Ctrl-S accepts; Ctrl-C or Escape cancels. Non-TTY input defaults to commands.
Use `:at LINE COLUMN`, `:delete COUNT`, and `:insert TEXT` inside the editor.
`:complete` lists source completions from fresh Script metadata, without live
bindings; `:complete INDEX` applies a listed completion in command mode. See the
[short editing tutorial](docs/scratch-editing.md) and [getting started](docs/getting-started.md).

`:scope script run` evaluates selected scratch source with a fresh interpreter
and `NoHost` on every invocation. It neither saves source nor writes results.
`:scope run` remains a separate, configured downstream semantic runner; Scope
Simple (`scope.simple`) is analysis, not Script execution or semantic acceptance.
Optional agent/model and TUI integrations are not needed for this path. The
`tools` feature does not grant write authority.

The same runnable source is [examples/workbench.scope](examples/workbench.scope).
Run it separately with the standalone CLI:

```sh
cargo run -p scope-script -- examples/workbench.scope
```

Expected output:

```text
{"name": "daily", "status": "ok", "total": 12}
```

Edit the file in your editor, save, and rerun. Each file execution uses a fresh
interpreter with **no host providers or authority**. A `.scope` suffix does not
install a language, select a host, or grant workspace access.

## Where to go next

- [Getting started](docs/getting-started.md): exact CLI forms and shell workflow.
- [Scope Script](docs/scope-script.md): file execution, REPL, output, explicit hosts.
- [Architecture](docs/architecture.md): responsibilities and authority boundaries.
- [Development](docs/development.md): Rust, Cell `v0.0.2`, build and validation commands.
- [Downstream Guide](DOWNSTREAM_GUIDE.md): Rust embedding and language registration.
- [Harness Guide](HARNESS_GUIDE.md): tool dispatch, authorization, and agent loops.
- [Security](SECURITY.md) and [Contributing](CONTRIBUTING.md).

Scope supports typed composition, bounded metadata snapshots, registered semantic
reads, and reviewable text proposals. These are building blocks, not a complete
autonomous compile/test/review workflow. Native language services are trusted host
code, not automatically sandboxed WASM. See [architecture](docs/architecture.md)
for current limits.

## License

[MIT](LICENSE). Dependencies, including Cell, have their own licenses.
