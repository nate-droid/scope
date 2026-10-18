# Contributing to Scope

Scope is an experimental Rust project, currently version `0.1.0`, licensed under the [MIT license](LICENSE). Its APIs, command interfaces, formats, and behavior are unstable; do not assume compatibility across 0.1 releases. Contributions should preserve explicit authority boundaries rather than promise stability that the project does not yet provide.

## Set up a checkout

Read [README.md](README.md) for the architecture and [BUILDING_WITH_CELL.md](BUILDING_WITH_CELL.md) for Cell build and capability guidance. Scope's Cargo manifests currently use local path dependencies on a sibling Cell checkout:

```text
parent/
  scope/
  cell/
```

Use Cell revision `ea19836fa758588ce0947aac46d1d80935e35614`, following the fresh-checkout and verification instructions in [Building with Cell](BUILDING_WITH_CELL.md#checkout-layout). Record both Scope and Cell revisions and any local changes when reporting build failures. The lockfile does not pin the contents of local path dependencies. Canonical public repository URLs and a contribution destination still need to be published; until then, obtain source locations and review instructions from the maintainer. See the [public launch prerequisites](BUILDING_WITH_CELL.md#public-launch-prerequisites).

Rust edition 2024 is configured in the workspace. Install a compatible Rust toolchain; optional features and downstream WASM builds may need additional targets or tools described in the relevant guides. Dependency downloads require network access unless dependencies are already cached.

## Keep changes focused

- Make narrow changes with a clear purpose; avoid unrelated refactors or formatting churn.
- Add regression tests for bug fixes and tests for new behavior, including rejection, stale-revision, and authorization cases where relevant.
- Update affected documentation and downstream call sites when changing an interface. Explain breaking changes explicitly.
- Keep domain semantics in downstream implementations rather than embedding language-specific behavior in Scope.
- Never commit credentials, tokens, private source, sensitive logs, or real model-provider configuration. Use synthetic fixtures and mocked model responses; tests should not require live credentials or paid model calls.
- Native `LanguageService`, `ToolProvider`, and interactive adapters are trusted host code, not sandboxed plugins. Do not use ambient filesystem, network, or process access to bypass host authorization or Cell capability enforcement. Capability declarations and discovery metadata are not permission grants.

For vulnerabilities, follow [SECURITY.md](SECURITY.md) rather than opening a public exploit report.

## Validate a change

From the Scope root, run:

```sh
cargo test --workspace --locked
cargo test --workspace --locked --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Formatting and strict Clippy are required CI checks on the tested toolchain documented in [BUILDING_WITH_CELL.md](BUILDING_WITH_CELL.md). Preserve meaningful rejection tests and public API contracts when fixing lint findings. Any necessary exception should be narrowly scoped, use `#[expect(..., reason = "...")]`, and explain the tradeoff; do not add blanket suppression. If another toolchain produces different findings, report its version and distinguish those differences from regressions.

The examples are separate workspaces, so root `--workspace` commands do not cover them. Validate affected examples separately:

```sh
cargo test --manifest-path examples/downstream/Cargo.toml --locked
cargo test --manifest-path examples/harness/Cargo.toml --locked
```

Include the commands run, results, toolchain, relevant feature flags, and any skipped checks in your contribution description. Explain the reason for skipping a check. Use minimal, non-sensitive reproduction cases for failures.

## License

Submit only material you have the right to contribute, suitable for distribution under Scope's existing MIT license. Retain applicable copyright and license notices.
