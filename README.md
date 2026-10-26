# Scope

Scope is a developer workbench for editing small programs, running scripts, and
exploring language-specific tools in one place. You can start with an in-memory
scratch program—no model server or project setup required—and add a language's
own analysis and execution services through Rust integrations.

Scope supplies the workspace, editor, operation discovery, and review controls.
Languages supply the meaning of their programs. Cell supplies capability-oriented
execution and filesystem boundaries where those capabilities are installed.

The goal is to let people and tools ask useful questions about programs without
forcing every language into one runtime or treating every interaction as a text
edit. Agents can discover operations and propose changes, while the host retains
authority over invocation, approval, and persistence.

Today you can edit and run Scope Script in memory, inspect registered language
services, embed semantic operations in a Rust host, and review revision-aware
proposals. Scope is experimental: these are usable building blocks, not a complete
autonomous development environment.

## Try it

Clone this repository and run these commands from its root. You need the Rust
toolchain selected by `rust-toolchain.toml`; the first build downloads dependencies,
including [Cell](https://github.com/nate-droid/cell) at `v0.0.2`.
No model server, API key, or Wasm target is needed.

```sh
cargo run --locked
```

At the prompt, paste:

```text
:scope scratch new script
:scope edit --commands
print(2 + 3)
:done
:scope script run
:scope quit
```

The run reports `print: 5` and `status: interpreter success`. Editing commits a
revision to memory, not disk. Each Script run uses a fresh interpreter with no host
providers or filesystem authority. Workspace access is read-only by default;
explicitly opt into saves with `cargo run --locked -- --writable shell`.

In continuous Script context (`:scope script enter`), successful evaluations show
only printed values and the final value. Failures retain detailed diagnostics.
`:scope script evidence` inspects the host's latest retained evaluation, including
its source snapshot, without executing again; semantic hosts also show their target,
catalog, and returned envelopes. Evidence describes that recorded run, not the
current scratch. `:scope script leave` restores the usual detailed run output.

For a saved-file example:

```sh
cargo run --locked -p scope-script -- examples/workbench.scope
```

Expected output:

```text
{"name": "daily", "status": "ok", "total": 12}
```

On a terminal, `:scope edit` opens the multiline keyboard editor: Ctrl-S accepts;
Ctrl-C or Escape cancels. The explicit `--commands` mode above also works with
piped input. Follow the [Scope tutorial](TUTORIAL.md) for editing, rerunning,
cancelling a draft, and inspecting source ownership.

For retained editing with an inspector and output pane, run
`cargo run --locked -- workspace .` in an interactive terminal. Follow the
[workspace tutorial](TUTORIAL.md#interactive-workspace) for F1 actions, F5 execution,
result history, F4 commands, and scratch switching. Ctrl-S commits in memory, never to disk.
This is not a full IDE or an automatically authorized downstream runner.

Want a longer, hands-on introduction? Try [Scope Script: A Weekend of Little
Programs](docs/tutorials/SCOPE_SCRIPT_TUTORIAL.md), with runnable examples and a
[field notebook](docs/tutorials/SCOPE_SCRIPT_FIELD_NOTES.md).

## How it fits together

```text
Human / agent / Scope Script
             |
Scope host: discovery, authorization, buffers, proposals
             |
Registered language services: domain semantics and operations
             |
Cell: capability-oriented execution and filesystem boundaries
```

- **Scope's interfaces** describe languages, source snapshots, semantic operations,
  and tools. Hosts register implementations and decide what may run.
- **Scope Script** is lightweight automation and composition. Its standalone CLI
  has no host providers; embedded hosts explicitly install and authorize them.
- **Languages** own their semantics. The stock shell includes Scope Simple analysis
  and a language-neutral fallback, not a general collection of language runtimes.
- **Agents** discover and propose through host-owned workflows. Evaluation evidence
  is not approval; accepting a proposal is not the same as saving it.
- **Cell** provides capability and sandbox boundaries for Cell execution. Native
  Rust extensions remain trusted host code, not automatically sandboxed components.

The default binary is `scope`, in `scope-workbench`; it composes `scope-ide` and
`scope-script`. The [architecture guide](docs/architecture.md) maps all crates and
explains state ownership, revision safety, and extension points.

## Extend Scope

Language integration is compile-time Rust composition, not dynamic plugin loading.
Start with the [downstream example](examples/downstream/README.md) and
[Integration guide](docs/integration.md) to register language-specific semantic
operations and compose a launcher. The [example catalog](examples/README.md)
distinguishes runnable demonstrations from explicit fixture hosts.

For tool authorization and proposal workflows, see the
[Agents guide](docs/agents.md) and [interactive sessions](docs/integration.md#interactive-language-sessions).

## Status and development

Public interfaces are developing. Agent/model integration, the browsing TUI, and
research fixture hosts are optional. The retained `workspace` editor and the
legacy browsing TUI are separate interfaces; neither is a full IDE. Discovery and
type compatibility do not establish semantic correctness or grant authority.
There is no complete autonomous compile/test/review loop.

- [Development](docs/development.md): prerequisites, features, builds, and tests.
- [Scope Script](docs/scope-script.md): language, output, and explicit host adapters.
- [Contributing](CONTRIBUTING.md) and [Security](SECURITY.md).

## License

[MIT](LICENSE). Dependencies, including Cell, have their own licenses.
