# Scope Script

For installation, saved-file execution, output formats, and host authority, use the
[canonical Script guide](../../docs/scope-script.md). See the
[example catalog](../../examples/README.md) for supported demos, explicit example
hosts, fixtures, and historical capture tools. These hosts are not automatically
installed in the standalone interpreter or the default IDE shell.

## Embedding

The minimal `Host` trait is an interpreter boundary, not an operation registry.
The embedding owns provider installation, target snapshots, authorization, and
interpretation of domain results. Start with the actual implementations:

- [`Interpreter` and errors](src/lib.rs), [`Host` and `RegistryHost`](src/host.rs).
- [`SemanticHost`](src/semantic.rs) adapts an explicitly constructed `AgentSession`.
- [`ScriptRunner` / `RunRecord`](src/runner.rs) bind separate script and semantic
  target snapshots and retain catalog, complete envelopes, prints, and outcome.
- [Registry host example](examples/registry.rs),
  [semantic host example](examples/semantic.rs), and
  [opt-in scratch shell runner](examples/scratch_runner.rs).
- [Normal launcher with an independent live scratch target](examples/semantic_target_cli.rs)
  and [SI6 API contract / Path integration handoff](docs/SI6.md).

`RegistryHost` delegates validation and dispatch to `OperationRegistry::invoke`;
missing policy denies invocation. This adapter additionally rejects non-read
operations even if policy would allow them. Scripts provide provider and action
as separate strings; the host retains the opaque expected revision and session.
`host.results` retains complete accepted `OperationResult`s with original document
identity/revision; scripts receive structured values. Successful dispatch or
interpreter completion is not semantic acceptance or authorization evidence.

`Error { offset, kind }` uses UTF-8 byte offsets. `ErrorKind::Operation` preserves
`OperationError`, and `ErrorKind::Tool` can preserve `ToolError`; syntax, runtime,
and resource failures remain distinct. Parsing and duplicate-binding checks
precede effects. Runtime failures do not roll back prior bindings, prints, or
host calls; hosts may drain output after failure.

Source is limited to 64 KiB, nesting/expression depth to 64, and evaluation has
finite work and retained-value/output budgets. Registry history is bounded;
retention failure after dispatch does not undo a read. These are logical budgets,
not allocator limits or host cancellation. Native providers are trusted,
synchronous code: Script adds no sandbox, hard deadline, or rollback.

For multi-operation downstream composition, see the [SI7 integration contract](../../docs/scope-script.md#composing-downstream-semantic-reads-si7)
and runnable [multi-operation example](examples/multi_semantic.rs). Exposure and
per-operation authorization are separate; existing single-operation constructors
remain supported.

## Validation

```sh
cargo test -p scope-script --all-targets --locked --offline
```

Completed experiment reports and original evidence are indexed in the
[archive](../../docs/archive/README.md), not maintained as duplicate user guides.
