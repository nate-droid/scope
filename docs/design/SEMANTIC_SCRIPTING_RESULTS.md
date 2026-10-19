# S8 — Read-only semantic scripting results

## Outcome

Scope Script can discover and invoke the existing `example.declarations` language's
`describe_selection` semantic operation, then report the selected binding's name,
kind and explicit unknown inferred-type status. Run from the repository root:

```sh
cargo run -p scope-script --locked --offline --example semantic
cargo test -p scope-script --locked --offline
```

The Rust embedding explicitly creates and scopes a scratch document and selects its
first complete declaration line. `examples/semantic.scope` is evaluated by the
embedding, not by the standalone CLI (which does not install semantic providers).
The language implementation is reused from Scope IDE's existing example module;
it is an actual LanguageService operation, not a production language integration.
No production integration with an applicable registered semantic read was found.

## Already present in Scope

- `LanguageService::semantic_operations` advertises language-local descriptors,
  argument contracts, effects and target requirements.
- `SemanticToolProvider` turns these into namespaced tools with structured metadata,
  validates language arguments, document permission scope, exact expected source
  and decimal version, and consults source authorization before dispatch.
- `AgentSession` catalogs and dispatches installed tools with a call policy.
- Read-only results retain their value and rationale. Other supported outcomes,
  including diagnostics/evidence, travel in the existing structured envelope.
- Source identity, selection and revision belong to the host; no editing lifecycle
  or semantic implementation was added to the interpreter.

## Interoperability glue

`scope-script/src/semantic.rs` implements the existing `Host` trait over an installed
session. One exact host-selected `language.*` read is visible; no registry or
arbitrary session invocation surface is added. The adapter supplies host-captured
source/version tokens and nests script arguments under `arguments`. Scripts cannot
retarget the provider or override those tokens. Discovery is independent of permission;
missing/false call authorization denies dispatch, and the provider's separate source
policy must also authorize. Hosts must install a trusted SemanticToolProvider with
the intended target and selection; a tool name is not a provider authenticity proof.

Invocation is capped at 64 attempts per adapter and structured values use the
interpreter's existing value limits. Interpreter syntax and types are unchanged.
Native provider execution remains trusted synchronous host code: the bridge does
not add a timeout or prove provider determinism/purity.

## Validation

`tests/semantic_inspection.rs` uses the public adapter for successful script discovery
and invocation, checks preserved metadata, partial/unknown/unsupported payload fields,
and verifies unchanged identity, text, version, dirty state and zero edit proposals.
Boundary cases cover unavailable operations, an action outside the allowlist, incorrect
arguments, a stale host revision, absent/denied call authorization, and denied source
policy. Every rejected case verifies no language invocation and no mutation.

Staleness is exercised with an already-stale host token. A mutable live-target race is
not simulated: the borrowed scratch target cannot be edited while the provider holds
it. Scope's provider owns pre/post-dispatch checks for targets with interior mutation.

## Payload and ownership limitations

- The declaration provider only parses integer literal bindings. References are
  explicitly unsupported, type analysis explicitly unknown; neither means empty or
  false. Semantic coverage is a language integration concern, not Script work.
- Payload shapes remain provider-owned. Script field/index access can report known
  fields, but cannot generically iterate or branch over arbitrary catalogs/payloads.
  A one-operation embedding needs no interpreter expansion; general reporting would
  require separately justified Script features.
- Scope's semantic provider currently constructs `LanguagePath::new("")` for dispatch.
  The bridge preserves existing host targeting but cannot supply a real language path
  through that API. Path-dependent semantics need a Scope-side API improvement, not
  a script-supplied path workaround.
- Freshness covers the target snapshot, not dependency closure or durable semantic
  evidence. That belongs to Scope/language contracts.
- ToolValue cannot represent arbitrary JSON numeric values. Unsupported values keep
  Scope's existing structured failure behavior; Script does not coerce them.
- Read-only providers still require a SourceEditController and document permission
  scope. This inherited API coupling belongs in Scope; this bridge creates no proposals.
- Discovery metadata and results may contain source information. Host installation
  and disclosure policy remain necessary even when invocation is denied.
