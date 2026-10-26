# Scope Meta

`scope-meta` is a language-neutral, in-memory structural graph library. It has
no Cell, filesystem, process, parser, model, or IDE integration. It reuses
`scope_types::TypeId` for exact nominal name/version identity; Meta relationships
are not executable pipeline connections and confer no authority.

## Construct and inspect a snapshot

Register node kinds with `TypeRegistry::register_node`, then relation kinds with
`register_relation(relation, from_kind, to_kind)`. Types require a nonempty
namespace (dot- or colon-separated names) and nonblank opaque version. Each
relation supports explicit exact directed endpoint-kind pairs. Add further pairs
with `register_relation_pair(relation, from_kind, to_kind)`; pairs are canonicalized
and checked individually, not combined into a permissive cross-product.

Pass the registry, nodes, edges, `GraphId`, and `RevisionId` to `MetaGraph::new`.
Construction validates registered types, unique node IDs, existing endpoints,
relation compatibility, source ranges, and provenance. Published graphs are
immutable; constructing a replacement snapshot is not an unrestricted mutation
API. Cycles and self-edges are permitted when their endpoint types match.

Read-only operations:

| API | Result |
| --- | --- |
| `nodes()` / `edges()` | Canonically ordered snapshot contents |
| `get(id)` | Node or structured `Error::UnknownNode` |
| `find(kind, optional_label_substring)` | Exact kind, case-sensitive label match |
| `related(id)` | All outgoing edges, preserving provenance |
| `related_by(id, relation)` | Outgoing edges of a registered relation kind |
| `source(id)` | Optional source metadata; does not read source text |
| `describe(id)` | Node plus incoming and outgoing edges |
| `node_ref(id)` / `resolve(reference)` | Graph- and revision-aware references |

Unknown query types return errors rather than silently returning empty results.
Known types without matches return empty results. Nodes sort by ID, and edges
by `(from, relation, to, provenance)`, independent of insertion order. Exact
edge duplicates are rejected; different provenance claims remain distinct.

Run the complete in-memory example from the repository root:

```sh
cargo run --offline -p scope-meta --example snapshot
cargo test --offline -p scope-meta
```

## Generic provider contract

`MetaProvider` is object-safe and has only two methods:

```rust
use scope_meta::{MetaProvider, MetaSnapshot, ProviderDescription, ProviderError};

fn inspect(provider: &dyn MetaProvider) -> Result<(), ProviderError> {
    let description: ProviderDescription = provider.description();
    let snapshot: MetaSnapshot<'_> = provider.snapshot()?;
    println!("{}: {} nodes", description.id, snapshot.graph.nodes().len());
    Ok(())
}
```

Providers return a borrow of their existing validated `MetaGraph`, not a rebuilt
or cloned graph. Use `MetaSnapshot::complete(&graph)`,
`MetaSnapshot::incomplete(&graph, warnings)`, or `MetaSnapshot::new(&graph,
completeness)`. The snapshot cannot outlive the provider's borrow.

`SnapshotCompleteness` is `Complete` or `Incomplete { warnings }`, relative to
the provider's declared scope. An incomplete snapshot may have no diagnostics.
`SnapshotWarning::new(code, message).with_locator(locator)` can represent a
truncated logical subtree or any other omission without domain-specific types.
Locators are opaque metadata, not filesystem access. Completeness does not
establish freshness, trust, or authoritative domain validity.

Failures use `ProviderError { kind, message, .. }`, with categories
`ProviderErrorKind::{Unavailable, InvalidSnapshot, Other}`. Use
`ProviderError::new(kind, message)` or `ProviderError::with_source(kind, message,
error)`. The latter retains the original error for `std::error::Error::source()`
and downcasting, including graph validation errors. Valid partial results should
return incomplete snapshots rather than errors.

This contract imposes no thread-safety bounds on providers and adds no host
access, refresh policy, discovery, domain types, or serialization protocol.

## Identity, revisions, and sources

IDs are opaque strings assigned by snapshot producers. A node ID is unique
within its graph; producers should reuse it for the same structural entity across
snapshots when appropriate. `NodeRef` includes graph identity and revision.
`resolve` checks graph, revision, then node existence. Producers **must change
the revision whenever snapshot information changes** and keep graph identities
unambiguous in their own context. The core cannot detect external changes or
prevent a producer from incorrectly reusing a revision.

`SourceRef` contains a logical locator, optional half-open UTF-8 byte-offset span
(`[start, end)`), and optional opaque source revision. Locators are metadata,
not host paths or access grants. Empty locators and reversed ranges are rejected;
source existence, length, UTF-8 boundaries, and freshness require the authoritative
source provider. No source material is read by the library.

## Provenance and trust

Every edge explicitly declares `Observed`, `Inferred`, or `Proposed`, with a
nonblank provider identifier. Queries preserve all statuses and do not silently
promote claims. `Observed` records the producer's claimed observation; it is
**not independently verified proof**. Consumers must use authoritative providers
or domain validators when establishing truth. The core checks structural typing,
not domain semantics, evidence, or provider trust.

## Serialization and limits

Graphs and registries support serde serialization. Deserialization runs the same
validation as construction, so malformed references cannot become a queryable
graph. Individual DTOs may be assembled freely; only a validated `MetaGraph`
provides snapshot queries. The wire format is an initial Rust/serde contract,
not a WIT interface or a promised versioned transport protocol.

Node lookup is binary search; scans, traversal, and descriptions are linear and
currently unbounded. There is no graph database, inheritance, cardinality checking,
provider discovery, automatic invalidation, or agent tool in this core.
The capability-backed workspace adapter lives in `scope_ide::workspace::meta`;
see [Structural workspace snapshots](../../docs/integration.md).
