# Building Scope with Cell

Scope is an experimental Rust workspace. Its host integrations and executor use
Cell from https://github.com/nate-droid/cell at tag `v0.0.1`, declared through
workspace dependencies. Cargo fetches Cell automatically; no sibling checkout is
required. Scope crates still use local path dependencies within this checkout.

## Tested development configuration

- Rust/Cargo **1.97.1**, with `rustfmt` and `clippy` installed. The root
  `rust-toolchain.toml` selects this version automatically through rustup;
  first use may download the toolchain and components.
- Cell Git tag **`v0.0.1`** from https://github.com/nate-droid/cell.
- Local validation on macOS. Linux is also targeted by the CI workflow, but a
  successful hosted CI run is still needed to establish that configuration.

This is a tested configuration, not a minimum supported Rust version or a
compatibility guarantee. Record the resolved Cell commit from `Cargo.lock` when
reporting results. The lockfile pins the resolved Git revision as well as registry
dependencies.

## Checkout and dependencies

Only a Scope checkout is required. Scope's workspace dependency declarations use
this Git source (with the same source and tag for the Cell capability crates):

```toml
[workspace.dependencies]
cell = { git = "https://github.com/nate-droid/cell", tag = "v0.0.1" }
```

Member crates inherit these dependencies with `workspace = true`. Do not modify
or relocate an existing Cell checkout to build Scope.

From **Scope's repository root**, verify the toolchain:

```sh
rustc --version
cargo --version
rustup component add rustfmt clippy
```


No Wasm target, model server, API key, Python installation, or `wasm-tools` binary
is required for Scope's host tests and the demonstrations below. Cargo may need
network access to download dependencies on the first build.

## Build and first run

All commands below run from **Scope's root**, not Cell's root:

```sh
cargo build --workspace --locked
cargo run --locked -p scope-ide -- --workspace . capabilities
cargo run --locked -p scope-ide -- --workspace . list
cargo run --locked -p scope-meta --example snapshot
```

The default `scope-ide` build has no optional features. The first two CLI commands
inspect the local workspace without invoking a model. Capability names describe
installed interfaces, not their permissions or proof of runtime authority.

For a scripted tool-discovery and authorization demonstration:

```sh
cargo run --locked --manifest-path examples/harness/Cargo.toml
```

It uses temporary fixtures, demonstrates approved and denied edits, and requires
no model credentials. See the [Harness Guide](HARNESS_GUIDE.md).

## Validation

Check local Markdown link targets independently of Cell (Python 3 required for
this documentation check, not for Rust builds):

```sh
python3 -m unittest discover -s .github/scripts -p 'test_*.py' -v
python3 .github/scripts/check_markdown_links.py
```

The checker covers inline local file and directory links in tracked Markdown;
it does not validate heading anchors or remote URLs.

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo test --locked --manifest-path examples/downstream/Cargo.toml
cargo test --locked --manifest-path examples/harness/Cargo.toml
cargo run --locked --manifest-path examples/harness/Cargo.toml
cargo doc --workspace --all-features --no-deps --locked
```

The examples are separate Cargo workspaces; `cargo test --workspace` does not
include them. After dependencies are cached, add `--offline` to Cargo build,
test, run, and documentation commands to prevent network access, including Git fetches.

Run the required strict lint check:

```sh
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

CI requires this check to pass on the tested toolchain. A few API-preserving
exceptions use narrowly scoped `#[expect(..., reason = "...")]` attributes;
Clippy checks that these expectations still apply. Do not add blanket lint
suppression or weaken rejection tests to make a contribution pass.

## Optional interfaces

- `tools`: model-independent tool discovery and dispatch.
- `agent`: includes `tools` and Cell's OpenAI-compatible chat capability.
- `tui`: experimental browsing interface.
- `logging`: explicit native logging integration.

```sh
cargo run --locked -p scope-ide --features tui -- tui .
cargo run --locked -p scope-ide --features agent -- \
  --workspace . agent --open README.md 'summarize this file'
```

The agent command sends selected context and tool feedback to the configured
model endpoint. The CLI defaults to `http://localhost:11434/v1` with model
`apple-foundationmodel`; that endpoint must be running for model calls to work.
CLI agent writes remain denied. Embedders can configure their own approved
endpoint and credentials through `AgentConfig`; see the
[Harness Guide](HARNESS_GUIDE.md) and [Security Policy](SECURITY.md).

For Cell guest development, consult the documentation at the matching
[Cell tag](https://github.com/nate-droid/cell/tree/v0.0.1).
Guest targets, WIT synchronization, component encoding, and Cell's own examples
are separate from building Scope. Native language services and tool providers
are trusted host code, not automatically sandboxed WASM guests.

## CI setup for maintainers

Build jobs need the Scope checkout and network access to fetch the tagged Cell
Git dependencies and registry packages, or a populated Cargo cache for offline
builds. No sibling Cell checkout or `CELL_REPOSITORY` variable is required by
Cargo. Obtain successful hosted CI runs on Linux and macOS before treating those
configurations as validated.

## Public launch prerequisites

- [ ] Publish and verify the canonical public Scope repository URL.
- [ ] Obtain successful hosted CI runs on the
  tested toolchain for Linux and macOS; skipped build jobs are not validation.
- [ ] Establish and document a working private vulnerability-reporting channel in
  [SECURITY.md](SECURITY.md) before public launch.

Registry publication additionally requires a dependency/release strategy because
Scope crates still use local path dependencies and Cell uses Git dependencies.
