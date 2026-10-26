# scope-execution

Synchronous execution of typed pipelines through Cell. Semantic declarations live in
`scope-types`; host-supplied `Bindings` associate each transformation's nominal
`TypeId` with component bytes and an export name. Values carry a nominal type and
opaque payload bytes. Transformations run inside real WebAssembly components, not
host transformation callbacks.

## Policy-aware execution

Use `execute_with_policy(&cell, &pipeline, &bindings, inputs, &granted, &policy)`
with Cell's `ExecutionPolicy` to enforce managed fuel, per-linear-memory limits,
cooperative deadlines/cancellation, and logical envelope payload limits. Existing
`execute` remains unconfigured and compatible. Both paths share typed validation
and artifact retention; no host capabilities are installed implicitly.

See [SE1 managed execution](../../docs/SE1_MANAGED_COMPONENT_EXECUTION.md) for
budget semantics, structured errors, containment limitations, and Cross adoption.
Run `cargo test -p scope-execution` for both legacy and managed execution tests.

## Run the tests and demonstration

Run from the repository root:

```sh
cargo test -p scope-execution --test execution
cargo run -p scope-execution --example text_pipeline -- 'Hello component world'
```

The demonstration prints `HELLO COMPONENT WORLD`, a word count of `3`, and the
completion order `z-uppercase`, `a-count`. Pass a single quoted argument to try
other input. No component build toolchain, network service, or installed host
capability is required: the existing `wat` dev dependency compiles fixtures at
runtime. Both tests and the example share `tests/support/mod.rs`.

## Component ABI

A binding's export (named `transform` in these fixtures) has this component-model
signature:

```text
func(list<tuple<string, string, string, list<u8>>>)
  -> list<tuple<string, string, string, list<u8>>>
```

Each tuple is `(port name, type name, type version, payload)`. Input ports are
identified by name, not by positional meaning. Output tuples must match the
transformation's declared output ports and exact nominal types, including version;
missing, duplicate, unexpected, or mistyped outputs are errors. The payload's
encoding is defined by its nominal type, not by the executor.

The fixtures use `Text@1` for UTF-8 bytes and `Number@1` for an unsigned 64-bit
little-endian integer. `Uppercase@1` changes ASCII `a`–`z` in the guest and leaves
all other bytes unchanged. `WordCount@1` counts runs separated by ASCII whitespace
(space or bytes `0x09`–`0x0d`); punctuation and non-ASCII bytes are non-whitespace.
This is deliberately not Unicode casing or Unicode word segmentation.

### Canonical ABI in the fixtures

The component lifts a core Wasm `run(i32, i32) -> i32` function using an exported
memory and `realloc`. Its two arguments are the input list's pointer and length;
the return value points to an eight-byte `(pointer, length)` output-list descriptor.
Each tuple occupies 32 bytes, aligned to four bytes, with four consecutive
pointer/length pairs for the three UTF-8 strings and byte payload. All canonical
pointers and lengths are 32-bit little-endian values. The word-count payload is
stored with a guest `i64.store`.

A small aligned bump allocator and two pages of memory are sufficient for these
short-lived, small-input fixtures. They are test components, not production
allocators: large inputs and persistent-instance allocation are outside their
scope. The host only supplies input bytes and reads results; the transformations
and counting loops execute in Wasm.

## Coverage and authority boundaries

`tests/execution.rs` covers dynamic values (including empty input, ASCII whitespace,
punctuation, and UTF-8), intermediate artifacts, selected output, and dependency
order despite reverse lexical node ordering. Negative tests cover graph type
mismatches, missing declared grants, missing bindings, invalid external inputs,
guest traps, malformed output declarations, identity pipelines, cycles, unknown
selected outputs, and incorrect export ABIs. A trapping core start function verifies
that preflight does not instantiate guests and that starts follow dependency order.

A separate component really imports an uninstalled function named `uninstalled`.
Its graph requirement is explicitly granted, so semantic validation succeeds, but
Cell rejects the component during link preflight with a binding error. Planning
grants do not install host functions or confer runtime authority. Tests do not
depend on backend error wording.

Before executing any node, the executor checks all component links and export ABIs
without instantiating components. A preflight failure prevents all guest execution.
Instantiation, including guest start functions, is deferred until each node's
dependencies have completed. Runtime failures stop execution while retaining
artifacts and completion records from successful preceding nodes.

Invalid first-node results must not become artifacts or allow dependent nodes to
run. Successful completion is reported only after output validation.
