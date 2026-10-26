# Development

Build, features, crate inventory and validation live here. See
[Integration](integration.md) for language/session/component contracts and
[Agents and harnesses](agents.md) for model loops, tool policy and proposals.
Contribution expectations are in [CONTRIBUTING.md](../CONTRIBUTING.md).
The [consolidation report](CLEANUP.md) records cleanup decisions, validation results,
and deferred work; it is not an additional setup guide.

## Workspace inventory

All nine workspace packages are version `0.1.0`, edition 2024, MIT licensed.
The root default member is `scope-workbench`, not the entire workspace.

| Crate | Role / boundary |
| --- | --- |
| `scope-workbench` | Default `scope` binary; composes the CLI with pure Script `WorkbenchRunner`; defaults to `tui` |
| `scope-ide` | App, Cell-backed workspace, document/scratch editing, CLI, optional tools/model/TUI/logging |
| `scope-language` | Language-neutral analysis, semantic and interactive/source contracts; Serde/JSON, no Cell runtime |
| `scope-types` | Nominal types, graph validation/discovery and portable composition manifests; no runtime |
| `scope-meta` | Immutable validated metadata graphs/providers over `scope-types` |
| `scope-execution` | Typed component binding/preflight/execution via Cell/Wasmtime; legacy and managed-policy entry points |
| `scope-script` | Separate interpreter/CLI and explicit host adapters; `scope-ide` tools enabled; pure execution uses `NoHost` |
| `scope-terminal` | Reusable UTF-8 editing/completion and terminal input over Crossterm |
| `xtask` | Language scaffolding and standalone downstream checks; no runtime dependency |

`examples/downstream` and `examples/harness` are independent Cargo workspaces,
not additional members. Other experiment workspaces may depend on local siblings;
root workspace tests do not validate them or those siblings.

## Toolchain and Cell

Run commands from the Scope repository root. `rust-toolchain.toml` selects Rust
**1.97.1**, minimal profile, with `rustfmt` and `clippy`. This is the configured
toolchain, not a minimum supported Rust version or a claim of platform validation.

```sh
rustc --version
cargo --version
```

Workspace Cell dependencies use Git tag **`v0.0.2`** from
[Cell](https://github.com/nate-droid/cell/tree/v0.0.2). Cargo fetches them directly;
no sibling Cell checkout, `CELL_REPOSITORY`, or relocation is required. Scope
member crates use local path dependencies. `Cargo.lock` pins the resolved Git
revision and registry versions; include it when reporting reproducibility issues.

The first build may need network access for the toolchain and dependencies.
After caching them, add `--offline` to Cargo commands to prevent fetches.
No Wasm target, model server, API key, Python, or `wasm-tools` is needed for the
default host build and shell. Cell guest development has its own target, WIT,
and component-encoding requirements; consult the matching Cell tag.

## Runtime defaults

Bare `cargo run` starts the shell in the current existing directory, mounts the
workspace read-only, and registers no language evaluator. The workbench does
install a separate pure Script callback (`:scope script run` / direct workspace
Script action): fresh interpreter, `NoHost`, no semantic or filesystem authority.
Startup does not create
directories. Enable workspace writes explicitly with
`cargo run -- --writable shell`; place `--writable` before the command. `meta`
always uses a read-only mount, even with `--writable`.

The `scope-ide` default feature set is `["tools"]`. Tool availability does not grant write
authority. `--no-default-features` disables tools; it does not change filesystem
authority. Agent/model, TUI, and logging integrations remain opt-in.

## Build and run

```sh
cargo build --workspace --locked
cargo run --locked
cargo run --locked -- --help
cargo run --locked -p scope-script -- crates/scope-script/examples/daily.scope
```

The root default member is `scope-workbench` (binary `scope`). No arguments opens the shell in the
current existing directory; it does not create a workspace. Script is a separate
binary. See [getting started](../TUTORIAL.md) for argument placement and
read-only authority boundaries.

The shell can run without optional model, TUI, logging, or language evaluation
support, but the default **workbench** build enables TUI. To omit its TUI use
`cargo run --locked -p scope-workbench --no-default-features -- --help`.
That does not disable `scope-ide` tools pulled in through its dependencies.
For the minimal IDE configuration use `-p scope-ide --no-default-features`. Feature declarations live in `crates/scope-ide/Cargo.toml`:
`tools` is enabled by default and gates tool integration; `agent` includes tools and the OpenAI-compatible
capability, `tui` includes tools and the browser/composable workspace, and `logging`
adds native logging. Use `--no-default-features` when explicitly checking the
minimal feature configuration.

```sh
cargo run --locked -p scope-ide --no-default-features -- --help
cargo run --locked -p scope-ide --features tui -- tui .
cargo run --locked -p scope-ide --features agent -- --workspace . agent --open README.md 'summarize this file'
```

The agent command sends selected context and tool feedback to the configured
endpoint. Its defaults are `http://localhost:1234/v1` and model
`qwen3-30b-a3b`; the server must be running. Global options go before
`agent`: `--agent-model NAME`, `--agent-base-url URL`, `--agent-openai`, and
`--agent-api-key-env NAME`. The last reads a secret from an environment variable;
never put credentials in source or documentation. Ordinary CLI/shell agent
investigations deny writes. Explicit proposal/edit workflows have their own
approval boundaries; see [Agents](agents.md). Native extensions are trusted host
code, not automatically sandboxed.

## Validation commands

These are commands to run, **not a claim that they have passed**:

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo doc --workspace --all-features --no-deps --locked
```

Check focused crates/features first when changing their code. Do not silence
failures with blanket lint suppression or weaken rejection tests. Report the
actual command, toolchain, platform, and result; a skipped CI job is not validation.

The downstream and harness examples are separate Cargo workspaces and are not
included in `cargo test --workspace`:

```sh
cargo test --locked --manifest-path examples/downstream/Cargo.toml
cargo test --locked --manifest-path examples/harness/Cargo.toml
cargo run --locked --manifest-path examples/harness/Cargo.toml
```

For Markdown checks only, Python 3 is required:

```sh
python3 -m unittest discover -s .github/scripts -p 'test_*.py' -v
python3 .github/scripts/check_markdown_links.py
```

The link checker checks inline local file/directory targets in non-ignored
working-tree Markdown, including new docs and excluding deleted paths. It does
not check heading anchors or remote URLs.

## Contribution and compatibility discipline

Keep changes narrow, add regression tests including stale-revision and rejection
paths, and explain breaking API/format changes. Do not weaken authorization tests,
add blanket lint suppression, or use ambient host APIs to bypass Cell confinement.
Native services/providers are trusted code, not automatically sandboxed guests.
Keep language semantics in downstream engines and credentials/private traces out
of source control. Use synthetic fixtures and deterministic model adapters;
contributions must not require paid/live model calls. Retain applicable MIT and
dependency license notices. Report vulnerabilities through
[SECURITY.md](../SECURITY.md), not a public exploit report.

Use `cargo run --locked -p xtask -- check-downstream --offline` for the separate
consumer/harness plus freshly scaffolded language checks when caches are populated.
Its offline switch follows `check-downstream`; Cargo's own `--offline` may also
precede `-p`. Scaffolding a real language is documented in
[Integration](integration.md#2-scaffold-a-language).

## Maintainer notes

CI needs network access to Cell's tagged Git source and registry packages, or
populated caches. Do not treat Linux/macOS as validated without successful hosted
runs. Registry publication needs a release strategy for local path and Git
dependencies. Before public launch, verify the canonical repository URL and a
working private vulnerability-reporting channel in [SECURITY.md](../SECURITY.md).
