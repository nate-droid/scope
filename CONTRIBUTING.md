# Contributing to Scope

Scope is experimental (`0.1.0`) and [MIT licensed](LICENSE). APIs, command
interfaces and formats are unstable; do not assume compatibility across releases.

## Set up and validate

Use [Development](docs/development.md) for the configured toolchain, crate
inventory, feature matrix, build commands, standalone consumer checks and CI
expectations. Cell is fetched from Git tag `v0.0.2`; no sibling Cell checkout or
`CELL_REPOSITORY` is required. Include the Scope revision, `Cargo.lock`, toolchain,
platform and local changes when reporting reproducibility problems.

Start with focused tests, then run the relevant workspace checks:

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

These are requested checks, not a claim they pass on every platform. Report exact
commands/results, features and skipped checks with reasons. Separate example
workspaces are not covered by root `--workspace`; see Development for their commands.
Preserve meaningful tests when fixing lint findings. Necessary exceptions should
be narrow, use `#[expect(..., reason = "...")]`, and explain the tradeoff rather
than suppressing warnings globally.

## Keep changes focused and authority explicit

- Submit narrow changes with regression tests, including denial, conflict and
  stale-source cases where relevant. Avoid unrelated refactors/formatting churn.
- Update affected documentation and call sites, and explain breaking changes.
- Keep domain semantics in independent downstream parsers, runtimes and kernels.
  See [Integration](docs/integration.md) for adapter contracts and local consumer risks.
- Native `LanguageService`, `ToolProvider` and session adapters are trusted host
  code, not sandboxed plugins. Do not bypass host authorization or Cell policy
  using ambient filesystem, network or process APIs. Discovery and declarations
  grant no authority.
- Keep invocation approval, candidate validation, proposal acceptance and saving
  distinct. See [Agents and harnesses](docs/agents.md) for concrete boundaries.
- Never commit credentials, private source, sensitive logs or real provider
  configuration. Use synthetic fixtures and mocked/scripted model responses;
  tests must not require live credentials or paid calls.

For vulnerabilities, follow [SECURITY.md](SECURITY.md), not public exploit reports.
Confirm contribution destinations and private reporting channels with the maintainer
if they are unavailable; this guide does not invent a canonical submission URL.

## License

Contribute only material you have the right to distribute under Scope's existing
MIT license. Retain applicable copyright and dependency license notices.
