# Cross-provider semantic discovery

Two example language services expose distinct semantic models through the same
`LanguageService` → `SemanticToolProvider` → `AgentSession` path. This demonstrates
catalog-driven transport and guarded invocation, not structural type compatibility
or equivalence between their results.

## Run

From the repository root, with dependencies available (`--offline` requires a
populated cache):

```sh
cargo run -p scope-ide --features tools --locked --offline --example cross_provider_discovery
cargo test -p scope-ide --features tools --locked --offline --test cross_provider_discovery
```

The fixture needs no model, network service, workspace read, or source-file write.
The executable prints catalogs and structured results and asserts unchanged
source/revision and empty proposal registries.

## Provider-owned meanings

| Contract | Declaration provider | State-machine provider |
| --- | --- | --- |
| Implementation under `crates/scope-ide/examples/` | `selection_description_support/mod.rs` | `state_description_support/mod.rs` |
| Language ID | `example.declarations` | `example.state_machine` |
| Source | `let answer = 42` | `idle -> running on start` |
| Local operation | `describe_selection` | `describe_selection` |
| Effect / target | Read / selection | Read / selection |
| Arguments | `{}` | `{}` |
| Selection | Complete declaration line without newline | Complete transition line without newline |
| Known | Binding, literal spelling, span | Selected transition, explicit states, outgoing edges |
| Unknown | Inferred type | Runtime reachability |
| Unsupported | References | Inferred type, guards/actions, implicit states |

The adapter namespaces operation names, so identical local IDs can coexist.
Hosts bind each service to its own source, selection, controller, document scope,
source/version preconditions, and concrete-call policy. The generic orchestration
loop uses discovered names without interpreting provider-specific payload fields.

The declaration result retains `partial`, an unknown-type reason, and unsupported
references. The state result retains nested transition/topology/capability data;
explicit states are sorted, outgoing edges retain document order. Unsupported
nonblank syntax is rejected rather than presented as complete topology. Neither
result claims evaluation, proof, inferred type success, or runtime reachability.
Unknown and unsupported are not false or fabricated empty collections.

## Boundaries and executable coverage

`crates/scope-ide/tests/cross_provider_discovery.rs` covers simultaneous installation
and a shared boundary matrix: second-line success, absent/partial/out-of-range
selection, wrong identity, semantic/session denial, stale version/text, attempted
retargeting, unknown arguments, and unsupported syntax. Assertions inspect complete
provider-specific meanings, dispatch counts, unchanged source, and no proposals.
These describe checked-in coverage, not a new test run for this document.

Selection is context, not range-limited permission: the state service inspects the
whole authorized snapshot. Native services are trusted code, not sandboxed by a
Read declaration. The semantic adapter forwards optional `SourceTarget::language_path`
context when supplied; a path is not authority and these fixtures do not need one.
Freshness checks bind the in-memory target, not external dependencies or atomic
disk state.

Agent `ToolValue` numbers must fit signed i64; fractional and oversized unsigned
JSON values fail conversion rather than being coerced. These fixtures stay within
the supported domain. Discovery grants no execution permission. See
[interoperability boundaries](design/INTEROPERABILITY.md) before interpreting
successful structured transport as automatic composition.
