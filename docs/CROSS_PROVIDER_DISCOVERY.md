# S4 — Cross-provider semantic discovery validation

## Result and scope

Two deterministic example-only providers with different semantic models participate through the existing `LanguageService` → `SemanticToolProvider` → `AgentSession` path. No production code, generic interfaces, dependencies, Cell sources, or edit workflows were changed.

This follows the [S1 inventory](design/semantic-inventory.md), reuses the completed [S2 declaration provider](SELECTION_DESCRIPTION.md), and retains the [S3 evidence/authority boundaries](EVIDENCE_VISIBILITY.md). These descriptions are provider observations, not evaluation results, proof, execution, or accepted task assurance.

## Run

From the repository root, using the sibling Cell checkout and dependencies described in [BUILDING_WITH_CELL.md](../BUILDING_WITH_CELL.md):

```sh
cargo run -p scope-ide --features tools --locked --offline --example cross_provider_discovery
cargo test -p scope-ide --features tools --locked --offline --test cross_provider_discovery
cargo test -p scope-ide --features tools --locked --offline
cargo fmt --all -- --check
```

No model, network service, execution backend, workspace read, or source-file write is required by the demonstration. The executable prints both catalogs and their structured results, then verifies unchanged source/revision and empty proposal registries.

## Compatibility matrix

| Concern | Declaration provider | State-machine provider |
|---|---|---|
| Implementation | Existing `examples/selection_description_support/mod.rs` | New `examples/state_description_support/mod.rs` |
| Language ID | `example.declarations` | `example.state_machine` |
| Model | Lexical integer-literal binding | Explicit finite-state transition topology |
| Source syntax | `let answer = 42` | `idle -> running on start` |
| Registered operation | `describe_selection` | `describe_selection` |
| Effect / target | Read / selection | Read / selection |
| Language arguments | Empty object | Empty object |
| Discovery / invocation | Existing semantic adapter and session catalog/dispatch | Same |
| Selection | Complete declaration line, excluding newline | Complete transition line, excluding newline |
| Known information | Binding name, literal spelling, selected span | Selected transition, explicit endpoint states, outgoing edges from selected source state |
| Unknown information | Inferred type, with reason | Runtime reachability, with reason |
| Unsupported information | References | Inferred type; guarded/action syntax and implicit states |
| Source context inspected | Selected line | Whole authorized document's explicit transitions |
| Snapshot preconditions | Exact source text and decimal revision | Same |
| Identity / permission | Host-bound target language and document scope, semantic and session policy | Same |
| Mutation / proposals | None | None |

Operation names are automatically namespaced by the existing adapter, so the same local operation ID coexists in one session. Installation and source/selection binding remain host responsibilities. The generic invocation loop uses discovered tool definitions; it does not branch on provider names or interpret result fields.

## Semantic differences preserved

The declaration provider returns a flat partial construct description: `kind`, `name`, `span`, `literal`, `partial`, `references`, and `inferred_type`. Integer spelling is a string; it is not promoted into a type inference. Unsupported references are not a known-empty reference set.

The state-machine provider returns a different nested structure:

- `transition: {from, to, event}` identifies the selected transition.
- `topology.states` contains unique explicit endpoint names sorted lexicographically.
- `topology.outgoing` contains all explicit transitions from the selected source state, in document order.
- `capabilities.topology.status` is `supported`.
- `capabilities.reachability` is `{status: "unknown", reason: "runtime guards unavailable"}`.
- `inferred_type.status` is `unsupported`.

These endpoint and edge facts are computed from the supplied document, not hardcoded answers. Selecting the second line changes the selected event and outgoing source. The provider rejects unsupported nonblank document lines instead of silently presenting incomplete topology as complete. Its topology does not claim enabled events, runtime state, behavioral reachability, transitive dependencies, or execution correctness. No universal graph or common type schema is introduced.

Unknown and unsupported values remain explicit objects/strings in both payloads, never `false`, null-as-false, or fabricated empty collections. Only provider-aware consumers/tests interpret these schemas; Scope transports them unchanged inside a `read_only` result.

## Deterministic coverage

`crates/scope-ide/tests/cross_provider_discovery.rs` contains two public integration tests:

1. Install both providers in one session with separate host-bound targets/controllers. Discover both namespaced operations, verify common effect/selection/schema metadata, invoke both through the same envelope/dispatch loop, and inspect their different payloads. Assert known topology, unknown type/reachability, unsupported capabilities, dispatch counts, unchanged source/revision, and no proposals.
2. Apply the same 12-case matrix to each `&dyn LanguageService`: second-line success, missing selection, partial selection, out-of-bounds selection, wrong permitted identity, semantic-policy denial, session-policy denial, stale revision, same-revision source mismatch, attempted retargeting, unadvertised arguments, and unsupported syntax. Assert statuses, provider call counts, unchanged identity/text/revision/dirty state, and empty proposal registries.

The standalone executable uses the same fixture implementations, with one identical catalog-driven loop over host-bound services. The coexistence test additionally verifies both can be installed simultaneously. Provider-specific knowledge is restricted to fixture implementations and payload assertions, not orchestration.

Validation observed locally: the runnable example succeeded; both new integration tests passed, including 24 shared boundary cases; the full `scope-ide --features tools` suite passed (295 library tests, 2 S4 tests, 5 S3 tests, 2 S2 tests, 8 controller tests); formatting passed. Feature-gated public-agent tests and doc tests contained zero runnable tests in this configuration.

## Integration friction and corrections

- **Language identity is enforced during adapter construction.** An initial fixture setup passed display labels to `ScratchBuffers::create`; this argument is the language ID. Construction correctly returned `Denied { tool: "language.semantic" }`. The example/tests were corrected to use `service.language_id()`. No adapter relaxation was needed.
- **Read-only operations still borrow an edit controller and use `PermittedEditScope::Document`.** Separate providers/targets use separate controllers when installed together. This is existing lifecycle coupling, not evidence that a new read API is required for these cases.
- **Selection is context, not range-limited permission.** The state provider needs the full authorized snapshot to compute outgoing edges, while the declaration provider only inspects one line. Neither receives a workspace capability. Native services remain trusted host code, not sandboxed executions.
- **The adapter supplies an empty language path.** Neither fixture depends on a filename. Source identity/freshness is enforced by host binding, not a provider-reported path.
- **Provider JSON has transport limits.** Existing `ToolValue` cannot represent fractional or oversized unsigned JSON numbers. Both models naturally use strings, arrays, objects, small coordinates, and booleans; neither needs a lossy conversion or generic transport correction. A future provider requiring numeric fidelity would need a concrete transport compatibility task, not a universal semantic model.
- **Snapshot freshness is narrow.** The tests reject stale invocation preconditions, not simulated concurrent mutation or transitive dependency changes. The normal immutable target borrow prevents mutation during these sessions. Results are not durable evidence or dependency-closure validity.
- **Capabilities remain opaque metadata/data.** Scope does not infer that one provider's unsupported references equal another's explicit outgoing transitions, nor require every provider to expose the same fields.

## Assessment

The existing abstractions are sufficient for these two meaningfully different models. The differences are contained in language-owned registration, parsing, constraints, and JSON results. No provider-name checks, language-specific orchestration, production corrections, migrations, or new generic interfaces were necessary.

This is bounded cross-provider validation, not proof that arbitrary providers or every JSON payload are compatible. A provider that concretely needs unsupported transport values, filesystem context, dependency freshness, or different authority semantics should supply a minimal failing integration and motivate a separate design task. S4 stops here: no universal semantic graph, type system, synthesis protocol, or evidence ledger.
