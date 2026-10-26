# CE4: Forth → explicit Church adapter → Path kernel

A standalone workspace using the real sibling Forth and Path providers. See [the CE4 report](../../docs/CE4_CROSS_PROVIDER_COMPOSITION.md) for the architecture audit, exact contracts, measurements and limitations.

From the Scope root, with sibling `../scope-forth` and `../path` checkouts and cached dependencies:

```sh
cargo test --offline --locked --manifest-path examples/ce4-composition/Cargo.toml
cargo run --offline --locked --quiet --manifest-path examples/ce4-composition/Cargo.toml > examples/ce4-composition/deterministic.json
```

## Actual execution

The reported Scope Script invokes three explicitly supplied host operations:

1. Forth `language.example.forth.evaluate_candidate` executes `: result 2 2 + ;` with a finite test. Its actual stack is `[4]`.
2. Host adapter `host.ce4.church-adapter.v1` requires the completed preceding producer evidence, a single passed observation and an actual singleton integer in `0..=8`. It encodes the **actual**, not expected, value as repeated function application.
3. Path `language.language.path.check_source` independently kernel-checks an identity comparing fixed Church `add two two` against the encoded observed result. Four is Checked; a real producer returning five passes its own test but receives MathematicalRejection from Path.

This checks one concrete arithmetic result, not general Forth correctness, native machine arithmetic equivalence or a verified translation.

## Arms and evidence

The executable reports direct sequential calls, Script composition, and a normal opt-in `AgentWorkflow` using the existing decision protocol with a deterministic replay model. Direct and Script evidence must match. The agent arm authorizes every registered inner binding wrapper via `AgentWorkflowContext::invoke`; it delivers a compact deterministic projection and retains full evidence locally. No independent agent runner or nested inference is added.

Reports retain arguments, complete tool envelopes/errors, source snapshots and actual revisions, evaluation classifications, grants/budgets, predecessor links and local attempt/source references. References are scoped to one report arm, not durable handles. Hypothetical candidates are not committed revisions. Scratch owners' raw IDs must be interpreted with their report-local source reference.

Tests cover success, incompatible/forged input, denial at each agent boundary, real Forth division-by-zero failure, real Path rejection, simulated incomplete evidence, shared call/byte bounds, actual stale source precondition rejection, source identity, compact projections and direct/Script evidence equivalence. Simulated semantic authorization denial is not a concurrent revision-change test.

Budgets are logical dispatch/delivery accounting, not native memory limits. Complete responses may be allocated and retained before return-byte rejection. Snapshots are immutable per call; concurrent live-source validation is not claimed.

Recorded wall times are nondeterministic. Serialized report/projection bytes are not model traffic. The agent records actual cumulative decision-message content UTF-8 bytes, not token counts. Its two prescribed decisions do not establish live-model discovery, reasoning, efficiency or speedup. Provider versions are not invented; local sibling dependencies remain checkout-dependent despite the lockfile.
