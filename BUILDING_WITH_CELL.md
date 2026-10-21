# Building Scope with Cell

The canonical setup, build, feature, and validation instructions are now in
[Development](docs/development.md). For the first shell and Script run, use
[Getting started](docs/getting-started.md).

Scope uses Cell Git tag **`v0.0.2`**, declared in the root workspace dependencies
and resolved in `Cargo.lock`. Cargo fetches Cell automatically; no sibling
checkout or `CELL_REPOSITORY` variable is required. Run Scope commands from the
Scope root, not Cell's root:

```sh
cargo build --workspace --locked
cargo run --locked
```

The root toolchain file selects Rust **1.97.1** with rustfmt and clippy. Initial
builds may download dependencies; cached builds can use `--offline`. The default
host shell needs no Wasm target, model server, or API key. Cell guest development
is separate: consult the [matching Cell documentation](https://github.com/nate-droid/cell/tree/v0.0.2)
for guest targets, WIT synchronization, and component encoding.

See [Development](docs/development.md) for validation commands and reproducibility
notes, and [Architecture](docs/architecture.md) for execution and authority
boundaries. Native language services are not automatically sandboxed WASM guests.
