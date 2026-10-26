# SE1 — Managed component execution

## Inspection findings (before implementation)

Scope pins Cell tag `v0.0.2`, lockfile revision `86145d1e6a2a1093e933c6754f812b1838817be0`. Inspected that checkout's `crates/cell/src/{lib,execution}.rs`, not a sibling checkout.

Scope currently preflights every component and exact envelope ABI before executing any start function; then executes dependency-ready nodes in fresh raw stores. Nominal graph/input/output validation, explicit Cell capability linking and completed artifacts must remain unchanged.

Cell `managed_store` and `instantiate_managed` enforce startup limits; `ManagedStore::call` supports dynamic `Val` only, automatically including canonical cleanup. There is deliberately no typed/raw mutable store escape hatch. Scope can convert its already ABI-checked envelopes to/from dynamic values without implementing another executor.

Supported controls: fuel refreshed separately for instantiation and invocation (invocation includes cleanup); per-linear-memory byte ceilings; absolute deadlines and sticky cancellation checked before/after operations and at guest epoch safepoints using Cell's ticker; recursive logical input/output payload accounting. Output accounting occurs after guest allocation and Wasmtime result lifting, not before host allocation. Input accounting also follows construction of host arguments. These are not total process-memory bounds. Native host functions cannot be forcibly interrupted; hosts need their own I/O timeouts/cooperation. Compilation/link preflight is not deadline/fuel bounded. No whole-pipeline fuel budget, aggregate memory ceiling, or hard wall-clock termination contract exists here; those require a different Cell contract.

Implementation and validation results follow below.

## Public contract and Cross adoption

```rust
pub fn execute_with_policy(
    cell: &cell::Cell,
    pipeline: &scope_types::Pipeline,
    bindings: &scope_execution::Bindings,
    inputs: std::collections::BTreeMap<String, scope_execution::Value>,
    granted: &scope_types::Capabilities,
    policy: &cell::ExecutionPolicy,
) -> scope_execution::ExecutionResult;
```

Cross can replace `execute(&cell, &pipeline, &bindings, inputs, &granted)` with:

```rust
use cell::{CancellationToken, ExecutionPolicy};
use scope_execution::execute_with_policy;
use std::time::{Duration, Instant};

let cancellation = CancellationToken::default();
let policy = ExecutionPolicy {
    fuel: Some(1_000_000),
    memory_bytes: Some(16 * 1024 * 1024),
    deadline: Some(Instant::now() + Duration::from_secs(2)),
    cancellation: Some(cancellation.clone()),
    input_bytes: Some(1024 * 1024),
    output_bytes: Some(1024 * 1024),
};
let result = execute_with_policy(
    &cell, &pipeline, &bindings, inputs, &granted, &policy,
);
```

These budgets are illustrative, not recommended universal production defaults. Cell's policy default has no configured limits. A cloned token can be cancelled by another thread. The same absolute deadline applies across nodes, but each node gets a fresh managed store; fuel resets for startup and call separately. A no-node identity pipeline executes no component, so Cell policy checks do not apply to its passthrough value. Payload limits apply to each component envelope, not aggregate retained artifacts or external graph inputs.

Keep the existing explicitly configured Cell: its linker remains the only source of host capabilities. `granted` only authorizes graph requirements; it does not install imports. This API installs no filesystem/network capabilities. Host state remains `()` as in the original Scope API. No capability bindings or ABI change is required.

The exact ABI remains `func(list<tuple<string,string,string,list<u8>>>) -> list<tuple<string,string,string,list<u8>>>`. All components are compiled, linked and ABI-checked before any guest starts. Full graph validation, input nominal checks, node ordering, output port/version checks and atomic publication of each node's outputs are shared with `execute`.

### Failures and retained evidence

`ExecutionError::Managed { node, phase, kind, message }` preserves Cell's public `ExecutionErrorKind` and records `ExecutionPhase::{Compile, Link, Binding, Instantiate, Invoke}`. Compile/link/ABI errors use `Invalid` with their known phase; no speculative parsing of error strings is used. Missing local implementations remain `Binding`; nominal graph/input/output errors remain `Validation`, `Input`, and `Output`. Managed invocation errors preserve `Trap`, `Fuel`, `Memory`, `Deadline`, `Cancelled`, `Invalid`, `Input`, or `Output` as classified by Cell. In particular, not every out-of-bounds memory trap indicates a configured memory ceiling. Unknown backend failures remain `Invalid`, rather than invented precise classifications.

Inspect structured fields rather than matching message wording. Messages retain backend details; treat them as diagnostic data and sanitize before exposing them across trust boundaries (host capability errors can contain sensitive data). A managed failure returns immediately, never publishing a successful final output. Previously completed nodes remain in `completed` and `artifacts`; the failed node contributes neither. Stores are discarded; host side effects are not rolled back. Cross should require empty `errors` and a present `output` before semantic adaptation/verification and retain partial artifacts as failure evidence, not successful computation.

Legacy `execute` retains its raw-store path, resource behavior and error variants. Both entry points share validation and pipeline execution code. Adding a public error enum variant requires downstream exhaustive matches to add a `Managed` arm; existing call arguments and result structure do not change.

## Observed enforcement and validation

Tests use actual Component Model WAT fixtures, with guest processing, loops, memory allocation and growth executed by Wasmtime; no simulated managed errors or external LLM is involved.

- `cargo test -p scope-execution --offline`: 14 legacy execution tests and 13 policy tests passed. Adequate fuel returns typed two-node results; real start/invocation loops exhaust fuel; second-node exhaustion retains the first node's typed artifact. Initial memory and growth exceedances report `Memory`. Recursive envelope input/output limits report `Input`/`Output`, including rejection after a successful guest call. Expired deadlines, pre-cancelled tokens, live-loop deadlines and live-loop cancellation are verified. Compile/link/trap phases, exact ABI rejection (including `u16` payload impostors), nominal input/graph/output failures and full preflight are tested.
- `cargo test -p scope-types -p scope-script --offline`: passed, including structural composition/manifest tests, contract-first interoperability, execution transcript, portability and Scope Script regressions.

The live interruption tests include finite fallback fuel to avoid an unbounded hang. Timing is cooperative and not a hard real-time guarantee. Cargo emits duplicate-package warnings from Cell's vendored example packages and a fixture dead-code warning; these do not affect execution.
