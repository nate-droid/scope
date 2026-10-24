# Interoperability boundaries

Scope keeps contract inspection, explicit adaptation, and execution authority
separate. Existing APIs are sufficient for bounded downstream orchestration;
there is no general structural compatibility or automatic wiring service.

## Existing contracts

- `scope-types` supplies exact nominal `TypeId { name, version }` matching,
  typed ports, graph diagnostics, and discovery of registered single-input
  transformation routes. Names and versions are declarations, not authenticated
  publishers or semantic-version compatibility. No route means none was found
  under that search/filter, not that conversion is impossible.
- `scope_ide::tool::operations::OperationSchema` supplies recursive contracts for
  null, boolean, signed-i64 integer, string, homogeneous arrays, and closed
  objects with required fields. Registry invocation validates values around
  dispatch. Schema equality is not general assignability, native ABI agreement,
  or semantic equivalence.
- `LanguageService` owns native syntax, semantic operation descriptions, and
  provider-defined outcomes. Semantic argument descriptions are shallow;
  operation-specific recursive result contracts are not generally published.
  Names, prose signatures, sample results, and observed field accesses cannot
  supply missing owner-authored promises.
- Script transports structured values and orchestrates explicitly installed
  operations. See the [Script guide](../scope-script.md) for current multi-read
  adapters and independent per-operation authorization.

## Lasting integration rules

1. Preserve exact nominal identities and opaque versions. Equal integer backing
   shapes do not make distinct units or domains interchangeable.
2. Treat absent representations and unresolved obligations as **unknown**, not
   null, a wildcard, compatibility, or proof of universal incompatibility.
3. Reuse authoritative endpoint schemas where sufficient; do not maintain a
   second Script type algebra or infer contracts from successful executions.
4. Keep projection, renaming, conversion, and request assembly in explicit
   downstream adapters. Even a document read followed by replacement needs
   host-owned revision context and deliberate argument construction.
5. A discovered transformation is a declaration. Host acceptance is neither
   proof of adapter correctness, an executable binding, nor authorization.
   Domain, precision, encoding, provenance, and freshness obligations remain.
6. Discovery and effect metadata grant nothing. Concrete host policy and source
   checks remain mandatory; Cell separately enforces capabilities for component
   execution. Trusted native providers are not sandboxed by a read declaration.
7. Preserve complete envelopes, partial/unknown/unsupported information, and
   source correspondence. Tool dispatch, interpreter completion, semantic
   acceptance, and mathematical correctness are different outcomes.

The private probes in `crates/scope-ide/tests/structural_interoperability.rs` and
`crates/scope-script/tests/contract_first_interoperability.rs` exercise curated
contract comparisons and authority separation. They are not public compatibility
APIs, independently deployed interoperability evidence, or verified conversions.
The [cross-provider example](../CROSS_PROVIDER_DISCOVERY.md) demonstrates faithful
transport of distinct semantic models, not their equivalence.

Extend shared APIs only for a concrete producer/consumer connection that existing
owner-authored contracts and an explicit adapter cannot support. Establish the
payload/envelope boundary and missing declaration first. No universal type
language, automatic adapter synthesis, or promotion of private probe helpers is
justified merely by matching fixture shapes.
