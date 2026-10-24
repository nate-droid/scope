# Architecture

## Responsibilities

| Layer | Implemented responsibility |
| --- | --- |
| `scope-workbench` | Default `scope` executable; composes the IDE shell with the unprivileged Script scratch runner |
| `scope-ide` | Embeddable host library and separate CLI, document and buffer state, workspace access, proposals, default tools, optional agent/TUI integration |
| `scope-language` | Language-service and semantic-operation contracts; downstream registration |
| `scope-meta` | Structural workspace snapshots, revisions, source references, read-only queries |
| `scope-types` | Versioned types, ports, transformation graphs, compatibility and composition validation |
| `scope-execution` | Binding and executing supported acyclic WASM pipelines through Cell |
| `scope-terminal` | Shared terminal interaction support |
| `scope-script` | Separate interpreter, no-host CLI/REPL, explicitly installed host adapters |
| `xtask` | Developer tooling, including downstream language scaffolding |
| Cell | WASM execution, capability interfaces and runtime resource policies |

The root Cargo workspace selects `scope-workbench` as its default member:
`cargo run` opens its shell. `cargo run -p scope-script -- FILE` runs a saved
Script file; `cargo run -p scope-ide` runs the host CLI without the workbench's
Script runner. See [getting started](getting-started.md).

## Discovery is not execution authority

Projects own semantics. Scope discovers typed operations and checks contracts;
matching types does not prove correctness. A runnable pipeline needs three
separate choices: composition, component bindings, and execution policy. A graph
alone supplies neither executable components nor permissions.

Capability names identify installed interfaces. Authority descriptors are
advisory metadata, not authorization tokens. Concrete tool dispatch, semantic
reads, and model calls still require the appropriate host policy.

The CLI requires an existing workspace and does not create it. Its mount is
read-only by default; `--writable` before the command explicitly enables workspace
writes. `meta` always mounts read-only, even with `--writable`. The `tools`
feature is enabled by default, independently of filesystem write authority.
Embedding APIs can supply different workspace policies; do not infer the CLI's
policy from `Workspace::new` alone. Read-only does not mean an OS sandbox:
native language services and tool providers are trusted Rust host code. Cell's
WASM boundaries apply to components executed through Cell, not automatically to
all extensions.

## Source owners and evaluation

The App document, shell workspace buffers, scratch source, and interactive
session are distinct state owners. Bare `open` selects the App document;
`:scope open` selects a shell buffer. `context` exposes identities, revisions,
and command targets. Changing source or target can invalidate captured requests.

No downstream interactive evaluator is registered by the stock IDE shell.
The default workbench separately installs `:scope script run` for pure Script
scratch execution; this does not install a language session or semantic authority. Downstream hosts can register
one and explicitly select a session. Script's standalone CLI instead uses
`NoHost`; its semantic adapters and shell runner require explicit installation
and authorization. See [Scope Script](scope-script.md).

## Review and current limits

Semantic inspection captures a request before one-shot confirmation of the exact
snapshot. Discovery alone does not execute it or grant agent authority. Text
proposals separate generation, review, acceptance, and saving. Source acceptance
changes memory, not disk or evaluator state. Workspace proposal acceptance checks
conflicts but applies multiple files sequentially, not atomically.

Agent/model integration and the browsing TUI are optional. Agent execution uses
a bounded decision loop; the stock CLI/shell agent policy denies writes. There
is no dynamic language-plugin loader or complete autonomous compile/test/review
workflow. The TUI is not a full editor.

## Extension points and repository map

Production implementations live in `crates/*/src` (the terminal library uses
`crates/scope-terminal/lib.rs`). Public library exports and their Rustdoc are the
API reference; integration tests live in each crate's `tests/` directory. Shared
example providers are fixtures, not installed languages. The standalone
`examples/downstream` and `examples/harness` workspaces validate embedding through
public interfaces; see the [example catalog](../examples/README.md).

Register language services and operation schemas through `scope-language` and the
IDE's `LanguageRegistry`. Compose a launcher with
`scope_ide::cli::run_with_registry_and_command`; Script hosts implement explicit
adapters rather than receiving implicit access to the workspace. Source targets
and snapshots carry ownership and revision identities: changing targets is not
permission to inspect them, and stale evidence must not authorize a new revision.
Candidate evaluation reports evidence separately from proposal acceptance.

Keep Cell-facing capability execution in the host/execution layers; languages
remain responsible for domain semantics, not Scope's approval policy. The
[interoperability guide](design/INTEROPERABILITY.md) explains the limits of
structural compatibility.

For integration details, see the [Downstream Guide](../DOWNSTREAM_GUIDE.md),
[Harness Guide](../HARNESS_GUIDE.md), and [Security Policy](../SECURITY.md).
