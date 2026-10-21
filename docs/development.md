# Development

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
workspace read-only, and registers no language evaluator. Startup does not create
directories. Enable workspace writes explicitly with
`cargo run -- --writable shell`; place `--writable` before the command. `meta`
always uses a read-only mount, even with `--writable`.

The default feature set is `["tools"]`. Tool availability does not grant write
authority. `--no-default-features` disables tools; it does not change filesystem
authority. Agent/model, TUI, and logging integrations remain opt-in.

## Build and run

```sh
cargo build --workspace --locked
cargo run --locked
cargo run --locked -- --help
cargo run --locked -p scope-script -- crates/scope-script/examples/daily.scope
```

The root default member is `scope-ide`. No arguments opens the shell in the
current existing directory; it does not create a workspace. Script is a separate
binary. See [getting started](getting-started.md) for argument placement and
read-only authority boundaries.

The lightweight shell does not require optional model, TUI, logging, or language
evaluation support. Feature declarations live in `crates/scope-ide/Cargo.toml`:
`tools` is enabled by default and gates tool integration; `agent` includes tools and the OpenAI-compatible
capability, `tui` includes tools and the experimental browser, and `logging`
adds native logging. Use `--no-default-features` when explicitly checking the
minimal feature configuration.

```sh
cargo run --locked --no-default-features -- --help
cargo run --locked --features tui -- tui .
cargo run --locked --features agent -- --workspace . agent --open README.md 'summarize this file'
```

The agent command sends selected context and tool feedback to the configured
endpoint. Its defaults are `http://localhost:11434/v1` and model
`apple-foundationmodel`; the server must be running. Global options go before
`agent`: `--agent-model NAME`, `--agent-base-url URL`, `--agent-openai`, and
`--agent-api-key-env NAME`. The last reads a secret from an environment variable;
never put credentials in source or documentation. CLI/shell agent writes remain
denied. Native extensions are trusted host code, not automatically sandboxed.

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

## Maintainer notes

CI needs network access to Cell's tagged Git source and registry packages, or
populated caches. Do not treat Linux/macOS as validated without successful hosted
runs. Registry publication needs a release strategy for local path and Git
dependencies. Before public launch, verify the canonical repository URL and a
working private vulnerability-reporting channel in [SECURITY.md](../SECURITY.md).
