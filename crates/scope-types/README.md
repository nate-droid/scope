# scope-types

A runtime-independent semantic planning layer for Scope. It describes and validates
transformations without knowing their language, domain, implementation, or runtime.
It does not execute transformations or encode WASM components.

## Model

- `TypeId { name, version }` is a nominal identity. Both strings must match exactly;
  use qualified names to avoid collisions. Versions are opaque, not semver ranges.
- `Transformation` has a named/versioned `id`, explicit named input/output `Port`s,
  and a set of required capability identifiers. Input and output port names are
  unique within their respective directions; the same name may occur in both.
- `Registry::register` rejects duplicate transformation identities and duplicate
  port names without replacing existing registrations. Types do not need a separate
  registration step.
- `Graph` is an editable description: named transformation nodes, typed external
  inputs, and explicit connections from external inputs or node outputs to node
  inputs. Its public data remains inspectable even when invalid or incomplete.
- `Graph::validate(&granted)` returns structured, displayable diagnostics for unknown
  endpoints, duplicate ports, mismatched types, missing inputs, multiple sources,
  cycles, and missing capabilities. Invalid wiring does not satisfy a required input.
  All inputs are required, outputs may be unused, and output fan-out is allowed.
  Multi-input and multi-output transformations are supported. Cycles are rejected;
  iteration and recursive execution semantics are not defined in this foundation.

`Registry::discover(input, output, permitted)` uses breadth-first search to return
one shortest chain as a `Pipeline { graph, output }`. `output` identifies the selected
result port, or the external input for a zero-step identity pipeline. Discovery
considers single-input transformations and any of their outputs. It deliberately
excludes transformations requiring multiple inputs: a single supplied type does
not establish that their additional values exist. General multi-input planning is
future work. Visited nominal types prevent cycles from causing unbounded search;
ties follow deterministic registry and output-port order.

`None` means capability-unfiltered discovery, **not permission to execute**.
`Some(&empty_set)` permits only transformations with no requirements. Validate the
returned graph against externally supplied grants before accepting a plan.

## Capability and Cell boundary

`Capabilities` is a set of opaque strings supplied by a trusted host for planning.
Requirements never populate it automatically. A requirement can reuse a Cell
interface identifier (for example `cell:fs/read@0.1.0`) or use a host-defined identifier
for a scoped effect. Scope performs exact identifier membership checks; it does not
interpret WIT, filesystem roots, credentials, operation policy, or authority tokens.
The host must choose identifiers granular enough for its planning policy.

Cell's installed interface list and `CapabilityDescriptor` / `AuthorityDescription`
are advisory metadata, **not grants**. Do not translate them automatically into a
permission set. Passing graph validation is not runtime authorization and does not
guarantee an operation will succeed. Cell must independently enforce capabilities
during execution.

Cell's `cell-compose` graph already describes component imports, exports, and
WASM/WIT wiring. It is intentionally not reused as a semantic type graph: component
items are not domain values such as syntax trees or proof candidates. Scope's graph
is a plan above that boundary; component inspection, encoding, composition, and
execution remain in Cell. No WASM dependencies are introduced here.

## Registering future language or Cell components

A language adapter or trusted component manifest reader can register declarations
using the same public API; no language-specific logic belongs in `scope-types`:

```rust
use scope_types::{Port, Registry, Transformation, TypeId};
use std::collections::BTreeSet;

let mut registry = Registry::default();
registry.register(Transformation {
    id: TypeId::new("example:language/parse", "1"),
    inputs: vec![Port::new("source", TypeId::new("example:source-text", "1"))],
    outputs: vec![Port::new("tree", TypeId::new("example:syntax-tree", "1"))],
    requires: BTreeSet::new(),
}).unwrap();
```

The adapter should separately retain a mapping from transformation identity to its
Cell component and invocation entry point. The existing `scope-execution` crate
consumes these local bindings; the mapping itself cannot grant authority. Semantic type names must not be inferred merely
from matching WIT layouts: nominal meaning is declared explicitly by the adapter.
Component manifests are declarations, not trusted authorization policy. The host
supplies planning grants separately and Cell validates component wiring and enforces
runtime authority independently.

## Portable composition manifests

`Manifest` is a versioned JSON description of an editable semantic composition.
Schema version `MANIFEST_VERSION` is currently `1`. Its fields are:

- `version`: the numeric format version (distinct from opaque nominal type versions).
- `inputs`: external input names mapped to `TypeId { name, version }` objects.
- `nodes`: graph-local node names mapped to transformation `TypeId` references,
  not full transformation declarations.
- `connections`: an ordered list of `{ from, to }` wires. Targets are
  `{ node, port }` endpoints. Sources are tagged objects: an external input is
  `{ "kind": "input", "value": "input-name" }`; a node output is
  `{ "kind": "output", "value": { "node": "node-name", "port": "port-name" } }`.
- `outputs`: named graph results mapped to sources in the same tagged format.
  Multiple results and direct external-input results are supported.

For example, a zero-step identity composition is:

```json
{
  "version": 1,
  "inputs": { "input": { "name": "example:text", "version": "1" } },
  "nodes": {},
  "connections": [],
  "outputs": { "result": { "kind": "input", "value": "input" } }
}
```

`Manifest::from_graph(&graph, outputs)` exports a graph with named results;
`Manifest::from_pipeline(&pipeline, "result")` exports an existing pipeline.
`to_json()` produces pretty JSON, and `from_json()` imports it without requiring
local transformations or valid wiring. Incomplete graphs and unresolved references
roundtrip intact so tools can edit them before they are executable. Malformed JSON
returns `ManifestError::Json`; unsupported schema versions return
`ManifestError::UnsupportedVersion` on both import and export. Unknown fields are
ignored as optional extensions and are not retained when re-exporting. Extensions
that require new semantics must use a new format version.

The document never serializes component implementations, bindings, capability
grants, or transformation requirements. Requirements and port declarations come
only from the local registry; host planning grants are supplied separately.
Transformation identities and semantic types match exact names and opaque versions.

### Downstream import, resolve, and execute

1. Import with `Manifest::from_json(json)`.
2. Resolve with `manifest.resolve(&registry, &granted)` using trusted local
   declarations and host-supplied planning permissions. Inspect
   `resolution.diagnostics`, `resolution.graph()`, and `resolution.outputs()`.
   `MissingTransformation` means a referenced declaration is absent;
   `Graph(Diagnostic::TypeMismatch { .. })` means invalid nominal wiring;
   `Graph(Diagnostic::MissingCapability { .. })` means a local requirement is
   ungranted. Invalid result references have named `Output` diagnostics. Resolution
   does not mutate or discard references in the original manifest. Optionally call
   `.with_implementations(bindings.keys())` on the resolution to diagnose
   `MissingImplementation` without loading components or executing anything. This
   checks binding presence only, not ABI compatibility or runtime authorization.
3. Select a named result with `resolution.pipeline("result")`. This returns the
   existing `Pipeline` type only if the entire composition has no diagnostics;
   even an invalid unselected output blocks adaptation. An absent result adds
   `UnknownOutput`. An identity pipeline selects an external input with no nodes.
4. Pass that pipeline to the existing executor with local runtime bindings and a
   configured Cell:

```rust,ignore
let manifest = scope_types::Manifest::from_json(json)?;
let resolution = manifest.resolve(&registry, &granted)
    .with_implementations(bindings.keys());
let pipeline = resolution.pipeline("result")
    .map_err(|diagnostics| format!("composition is not ready: {diagnostics:?}"))?;
let result = scope_execution::execute(&cell, &pipeline, &bindings, inputs, &granted);
```

Here `bindings` is `scope_execution::Bindings`, a local map from transformation
`TypeId` to `Binding { component: Vec<u8>, export: String }`. `inputs` maps external
input names to `scope_execution::Value { ty, bytes }`. Component exports must follow
`scope-execution`'s port-envelope ABI. These objects are supplied downstream, not
loaded from the portable manifest. Inspect `result.errors` and `result.output`.
Successful resolution establishes neither installed runtime bindings nor runtime
authority: the executor checks the plan and local bindings, and the supplied Cell
independently enforces operations. Planning grants do not install Cell capabilities
or override its policy.

## Demonstration and checks

From the workspace root:

```sh
cargo test -p scope-types --offline
cargo run -p scope-types --example composition --offline
```

The example registers compilation and proof pipelines, describes a discovered graph,
validates successful composition, rejects a nominal version mismatch, and reports
missing capabilities. Nothing is executed by this example. Manifest tests additionally
cover portable roundtrips, local resolution, named-result adaptation, and authority
boundaries. Serialization uses `serde` and `serde_json`; no runtime or WASM
dependencies are introduced here. Workspace Cargo resolution still requires the
existing sibling Cell checkout.
