# Native downstream example

This standalone Cargo workspace exercises Scope's public native `LanguageService`
and host application APIs. Run it from the repository root with:

```sh
cargo run -p xtask -- check-downstream --offline
```

The Scope dependency paths are for local development in this repository, not
portable external dependencies. Cell is fetched from
https://github.com/nate-droid/cell at tag `v0.0.1`; Scope crates inherit it through
workspace dependencies, while this standalone workspace declares its direct Cell
dependency with the same Git URL and tag. No sibling Cell checkout is required.
Offline commands require cached dependencies, including the Cell Git source.

## Authority and component boundary

`CalcLanguage` analyzes supplied text in-process. It is not a sandboxed Cell
guest: native language implementations remain trusted host code. The runners
explicitly construct a read-only `Workspace`; registering a language does not
add filesystem write permission or a chat capability. The tests cover analysis,
host registration, read-only write denial with unchanged contents, and parent
traversal rejection.

The standalone workspace also validates downstream WASM transformation binding:

```sh
cargo test --manifest-path examples/downstream/Cargo.toml --test wasm_binding --offline
```

`fixtures/reverse.wat` is a downstream-owned component that reverses opaque bytes
inside WASM. `tests/wasm_binding.rs` uses only public `scope-types`,
`scope-execution`, and Cell APIs to declare `consumer:ReverseBytes@2026-10`, bind
its custom `reverse-bytes` export, and execute it with `consumer:Bytes@v7` values.
It covers empty, odd/even-length, and non-UTF-8 payloads, plus rejection of an
output type-version mismatch without recording successful artifacts. No Scope
implementation changes or Scope test helpers are required. The fixture uses the
port-envelope component ABI documented in `../../crates/scope-execution/README.md`;
its small bump allocator is intended only for small test inputs.

There are no guest WIT packages or embedded providers in this example. It does
not exercise WIT synchronization, component composition, or missing-import
instantiation failures. Native registration is not sandboxed WASM execution.

For Scope setup, see [Building Scope with Cell](../../BUILDING_WITH_CELL.md).
If extending this example with a Cell guest, consult the [Cell v0.0.1 guest documentation](https://github.com/nate-droid/cell/tree/v0.0.1):
use Cell bindings rather than guest `std::fs`, synchronize filesystem WIT
(including `metadata` and `symlink-metadata`) with the host and `cell-guest`,
inspect the encoded component boundary, and explicitly validate any composition
with `Composer::check()` or `compose()`. Host declarations grant no authority:
test instantiation both with and without the required host capabilities.
