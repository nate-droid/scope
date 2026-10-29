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

## Start here

From the repository root, in an interactive terminal:

```sh
cargo run
```

This launches **Scope**, the retained workbench editor, using the pinned Rust
toolchain and default features. No package selection, feature flags, model server,
API key, or Wasm target is required. The first build downloads dependencies,
including Cell `v0.0.2`; the application does not contact a model on startup.

1. Press **F1**, search **New Script**, then Enter to create and select a scratch.
2. Type `print(2 + 3)` in the editor.
3. Press **Ctrl-S** to publish the draft to the authoritative in-memory scratch.
4. Press **F5** to run that committed revision. Output and source/revision evidence
   remain in the output pane; use F1 → **Focus output** to inspect it.
5. Press **F3** to return to editing the same source. **Ctrl-Q** exits Scope.

F1 discovers actions, F4 accepts workbench commands, and F2 temporarily opens the
shell. Shell `quit` returns to the same workspace. Source text in the editor is
Script, not a workbench command. Ctrl-S is **not a disk save**. Startup filesystem
access is read-only; pure Script runs with no host providers or native authority.
Agent compilation, discovery, and proposal acceptance do not grant authorization.

The [canonical quickstart](docs/quickstart.md) explains the contexts, draft/run
boundaries, shell transitions, and saved-file execution. `cargo run -- --help`
shows the supported CLI; `cargo run -- --version` reports the binary version.
The single recommended application binary is `scope` (`scope-workbench`).

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
research fixture hosts are optional. The default retained workbench and the
legacy developer browsing TUI are separate interfaces; neither is a full IDE. Discovery and
type compatibility do not establish semantic correctness or grant authority.
There is no complete autonomous compile/test/review loop.

- [Development](docs/development.md): prerequisites, features, builds, and tests.
- [Scope Script](docs/scope-script.md): language, output, and explicit host adapters.
- [Contributing](CONTRIBUTING.md) and [Security](SECURITY.md).

## License

[MIT](LICENSE). Dependencies, including Cell, have their own licenses.
