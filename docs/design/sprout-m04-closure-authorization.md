# Sprout M0.4 — closure authorization contract and security report

## Status and decision

**Partial milestone: tested Scope contract, not completed Sprout integration.**
The implementation adds an opt-in `closure_inspection` module behind Scope's
existing `tools` feature. It does not install a Sprout provider, adapt the M0.3
registry, change App Run/Check, or enable multi-module execution. In particular,
its fake-host tests do **not** establish that the existing production semantic
provider authorizes a Sprout closure. The M0.4 end-to-end success criteria remain
open. Do not advance to contained multi-module execution on this evidence alone.

Implementation: [closure_inspection.rs](../../crates/scope-ide/src/closure_inspection.rs).
The only existing-code change is its module export. Initial working-tree status
was clean; sibling Sprout and Cell sources were inspected but not edited.

## A. Existing boundaries, verified against source

The sibling Sprout M0.3 implementation and `docs/M03_SEALED_CLOSURES.md` authority
audit were read before implementation. M0.1 compilation inputs, M0.2 resolution,
and M0.3 registry/coordinator remain test-only native compiler experiments.
`Registry::capture` follows only explicitly registered logical modules using the
real bounded parser, seals `CompilationInput`, and checks it. Its coordinator's
boolean approval is simulated authority, not Scope authorization.

Scope's existing [InspectionBinding](../../crates/scope-ide/src/semantic_inspection.rs)
contains one IDE source identity, text, version, language and complete operation.
`SemanticAuthorization` in
[tool/semantic.rs](../../crates/scope-ide/src/tool/semantic.rs) exposes one borrowed
IDE snapshot and arguments. This is the precise single-document boundary.
`SemanticToolProvider` independently checks permitted target scope, expected
source/version, operation/arguments, host policy and post-policy freshness before
dispatch. A root-only policy callback cannot be interpreted as closure approval.

[AnalysisContext and CheckEvidence](../../crates/scope-ide/src/analysis.rs) capture
bounded requested dependencies and successful acquisition edges. Historical
`SourceResolver` lookup is exact and performs no I/O. Validation rechecks live
ownership/origin and acquired paths/text under bounded work; the host separately
checks the editor draft. This records requested analysis reads, not proof of
complete Sprout dependency discovery. Relative analysis paths are not logical
module names. Sequential filesystem validation is not an atomic transaction.

### Identity mapping

| Identity | Owner and meaning |
| --- | --- |
| IDE `source_edit::SourceId::Document(DocumentId)` | Host-assigned editable target within its owner; not a path |
| Language `SourceId` | Opaque retained `Arc` identity; process-local, not serializable global identity |
| `Document::evaluation_source_id()` | Existing document-owned association with its language identity; not numeric conversion |
| `RevisionId` | Version within that language source; never authority on its own |
| Analysis draft/dependency snapshots | Fresh language identities allocated by analysis capture; not automatically IDE document identities |
| M0.3 snapshot identity strings | Experimental host-assigned registry labels; not Scope IDs |
| Logical module and compiler source index | Semantic compiler names/indexes; not document ownership or acquisition capabilities |
| Display path/origin | Host metadata and provenance, never owner identity |

A concrete adapter must retain the mapping for each registered editor snapshot,
rotate document identities on replacement/reopen, and publish/compare unsaved
text explicitly. Equal bytes may have equal portable compilation content while
remaining distinct authorization inputs. Independently allocated IDs must not be
represented as globally stable.

## B. New binding contract

`ClosureInspectionBinding::capture` seals caller-supplied owned snapshots. Private
binding fields prevent editing a sealed request through this API. Equality covers:

- A retained, process-local `ClosureOwner` and exact root IDE identity.
- Every source's IDE identity, exact UTF-8 text, version, language, logical module
  and origin; distinct equal-text sources remain distinct.
- Exact dependency edges and selected complete `SemanticOperationDescription`.
- Host-supplied compilation identity, options and effective limits.
- Host-supplied execution policy and artifact identity.

Duplicate source/module/edge identities, missing endpoints/root, unreachable extra
sources, dependency cycles, empty/mixed language identities, and unsupported
operation shapes reject. Operations must be read-effect, document-target and
zero-argument, as in the existing inspection bridge. This generic admission does
not validate Sprout syntax, exports, module-name grammar, compiler limits, or the
truth of a host's graph. A future adapter must supply those guarantees from the
real compiler; passing a syntactically valid graph alone is insufficient.

Limits are contract-envelope ceilings, **not increased Sprout compiler ceilings**:
128 sources, 1,024 edges and 1 MiB combined admitted source/metadata bytes. The
adapter must also enforce the existing smaller compiler bounds. Opaque JSON has
shared depth 32, node 8,192 and string/key-byte admission; serialized byte counting
uses a bounded writer. Rejected nested JSON is dismantled iteratively. Incoming
owned allocation and raw capture cloning require upstream admission; this API
cannot retroactively bound caller allocation.

`review()` streams a bounded 64 KiB metadata projection without source-text
fields. It exposes root/source descriptors, revisions, language, origins, graph,
operation, compilation options/limits and artifact/runtime policy. It fails rather
than truncating approval material. Debug formatting redacts snapshots. Host
metadata must itself be source-free and rendered safely by a future UI. The
compilation identity is supplied by the host, not a canonical Sprout identity
computed here. A concrete adapter must use M0.1's versioned portable representation
and keep host-local identity separate. Neither a path nor digest is a grant.

## C. Policy integration: what is and is not real

`ClosureAuthorization` is an explicit new host callback contract containing the
complete closure plus Scope's actual `SemanticAuthorization` envelope for the
root. The API requires a closure-aware callback; it does not silently widen the
existing root-only callback. A language provider must not supply this decision.

**Nesting the existing envelope is not integration with SemanticToolProvider.**
There is no production caller, registered editor acquisition adapter, permitted
closure-scope controller, App confirmation surface, or managed policy enforcement
in this module. The trusted `ClosureInspectionHost` is responsible for complete
discovery, truthful recapture, owner lifetime, operation/artifact selection,
execution-limit enforcement and exact snapshot dispatch. The current tests use
counting fake hosts. This is a narrow tested contract proposal, not a second
Sprout permission system and not a claim that the central M0.4 integration
acceptance requirement has passed.

## D. Freshness and invocation consumption

A non-Clone invocation ticket is consumed on success, denial or failure. Before
policy, after policy, and immediately before dispatch, the host must recapture and
the core compares the entire binding. This catches changed text at unchanged
version, changed version at unchanged text, owner/root/dependency replacement,
graph/language/metadata changes, and policy/artifact changes when truthfully
reported by the adapter. Denied or mismatched requests never call `dispatch`.
Dispatch receives the sealed binding, not permission to resolve imports again.

This is per-invocation policy, not an at-most-once-program ledger: selecting a
binding again creates a new ticket and requests policy again. Historical results
contain no grant. Closure ownership is host-maintained; copying an owner token to
a different App would violate the adapter contract. Exterior/interior mutations
must be excluded by a lock or immutable snapshot handles spanning validation and
dispatch. Repeated recapture alone does not prove atomic filesystem behavior.

## E. Historical and rejected evidence

`ClosureInspectionResult` retains the exact binding and host observation. Reading
it has no provider or policy callback and cannot refresh authorization. Fake-host
tests cover repeated historical reads after dependency change/removal and shared
dependencies across roots. No Details/paging/reopening UI is added; existing
single-document retained-result UI remains unchanged.

`ClosureRejection` is separate from successful results. It retains bounded
phase/category and only known identity/lifecycle facts. Failed capture does not
invent a validated root or unavailable-dependency location. Arbitrary host error
strings/source text are not retained. Pre-dispatch failures record no dispatch;
a dispatch error records execution as unknown rather than claiming zero guest
work. Optional source-local spans require valid UTF-8 boundaries in the exact
original seal (including across binding clones); a later capture cannot annotate
an older rejection. This validates provenance/coordinates, not diagnostic truth.
Rich partial acquisition evidence from M0.3 is not wired into this API.

## F. Wasm-first audit and future requirements

| Processing | Current classification |
| --- | --- |
| Snapshot ownership, byte comparison, policy selection, review projection | Trusted host metadata handling |
| M0.3 import discovery/parse/check | Bounded native untrusted-source parsing, test-only |
| Production Sprout LanguageService analysis, symbols and diagnostics | Native untrusted-source parsing/checking; no `main` execution |
| Source rendering, diagnostic projection, protocol decoding | Trusted host handling of bounded untrusted data; not Wasm isolation |
| Production `evaluate-v4` parse/check/root execution and preview | Contained single-source Wasm work |
| Multi-source discovery/check/resolve/evaluate | Future contained operation; not implemented |

Sprout `src/contained.rs` loads selected bytes via `SPROUT_COMPONENT` or the
default artifact, hashes loaded bytes, uses `Cell::empty`, rejects imports,
checks the exact v4 export/signature, and checks linking. Successful preparation
is cached in `OnceLock<Host>`: the installed loaded host wins, not whatever bytes
later appear at the path. Every execution uses a fresh managed store. A digest
identifies bytes, not authenticity or semantic correctness. The loader does not
use Cell's content-addressed artifact store as an approval allowlist.

Both lockfiles select Cell `v0.0.4`, commit
`8ece6bf88497b402a6a417c95adf9bf1a6bd6aae`. The pinned cached Cell source was
inspected; sibling Cell HEAD matches but is not the dependency-selection
mechanism. Cell admission/link compatibility is not source authorization.
Memory limits are per linear memory, transfer limits per crossing, and managed
fuel is replenished per phase—not one cumulative process-wide budget.

Existing Sprout defaults remain: 65,536 source bytes, 100,000 interpreter steps,
recursion 64, Cell fuel 100,000,000 per phase, 32 MiB per linear memory, a fresh
absolute five-second deadline, 128 KiB input/output and 1 MiB host transfer.
Native Check and host artifact compilation are outside guest containment.

A concrete closure adapter should bind a host-approved implementation policy
**and the exact loaded artifact digest**, ABI/compiler semantics and effective
limits. It must compare the loaded handle's identity, not merely rehash its path.
Changing effective policy or selecting another loaded artifact must require new
review/approval. An unchanged cached artifact does not become a new artifact
because the file changed. Host selection is never authority granted by imports.

A later versioned component must receive only authorized units, independently
admit the complete bundle, reparse imports, resolve/check inside containment and
execute only the root entry. It must have zero imports, no filesystem/network,
preserve interpreter/Cell budgets, return bounded source-indexed diagnostics and
previews, and fail closed without native fallback. A bounded contained discovery
stage could return imports and source-local spans per admitted unit for explicit
host acquisition; graph completeness still needs final contained validation.
Design/adversarial review of that protocol is future work, not an ABI introduced
here. `sprout:c2/evaluate-v4` is untouched; S2 preview is not bundle transport.

## G. Compatibility

Existing single-document binding/provider code, operation discovery, App Check,
Run gesture, explicit Evaluate confirmation, draft publication and retained UI
are unchanged. App Check/Run publish through the existing exact-baseline editor
source-event path, not implicit disk save or scratch fallback. Configured scratch
Check uses pinned analysis evidence without publication. Check does not execute
`main` or grant execution. No additional CandidateEvaluation is advertised.
No production native multi-module execution, tokenizer, syntax, collection,
resource ceiling, guest imports or component bytes changed.

## H. Verification record

Commands below ran from the Scope root, reusing its configured target directory.
No `cargo clean`, live model call or component re-encoding was used. Existing
cached components were exercised; no multi-module Wasm parity is claimed.
Baseline suites were run after the additive module was introduced, so these are
existing-regression results, not a pristine pre-edit binary baseline. Build
warmth varied and concurrent Cargo invocations waited for locks; no performance
claim is made.

### Completed

- `cargo test --locked --manifest-path ../sprout/Cargo.toml --offline` — complete
  root suite passed (301 tests, one intentional tokenizer ignore).
- `cargo test --locked -p scope-language -p scope-ide` — complete default affected
  package suites and doctests passed.
- `cargo test --locked -p scope-ide --lib closure_inspection::tests` — 14 passed.
- `cargo test --locked -p scope-ide --lib semantic_inspection::tests` — 11 passed.
- `cargo test --locked -p scope-execution --all-features` — complete real-Wasm
  execution/policy package suite passed.
- `cargo test --locked --manifest-path ../sprout/Cargo.toml --lib m03 --offline`
  — 32 passed.
- `cargo test --locked --manifest-path ../sprout/Cargo.toml --lib m02 --offline`
  — 21 passed.
- `cargo test --locked --manifest-path ../sprout/Cargo.toml --lib compilation_input --offline`
  — 13 passed.
- `cargo test --locked --manifest-path ../sprout/experiments/c2/guest/Cargo.toml --offline`
  — guest regression suite passed (2 tests).
- `cargo test --locked --manifest-path ../sprout/experiments/c2/host/Cargo.toml --offline`
  — host suite passed (8 tests).
- `cargo check --locked -p scope-ide --lib --no-default-features` — passed.
- `cargo test --locked -p scope-ide --lib --no-default-features` and
  `cargo test --locked -p scope-ide --lib --all-features` — passed.
- `cargo clippy --locked -p scope-ide --lib --tests -- -D warnings` — passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` and
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — passed.
- `cargo clippy -p scope-ide --all-targets --features tools --locked -- -D warnings`,
  the same command with `--features tui`, and with `--no-default-features` — passed.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked`
  — passed.
- `cargo test --locked --manifest-path examples/downstream/Cargo.toml`,
  `cargo test --locked --manifest-path examples/harness/Cargo.toml`,
  `cargo run --locked --manifest-path examples/harness/Cargo.toml`,
  `cargo fmt --manifest-path examples/managed-transfer/Cargo.toml -- --check`,
  `cargo test --locked --manifest-path examples/managed-transfer/Cargo.toml`
  — all passed.
- `cargo run --locked -p xtask -- check-downstream --offline` — passed, including
  generated downstream/scaffold checks.
- `cargo fmt --all -- --check` and `git --no-pager diff --check` — passed during
  focused implementation verification and again after report creation.
- `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`
  — 5 passed.
- `python3 -B .github/scripts/check_markdown_links.py` — all working-tree local
  Markdown link targets exist.

Initial implementation iterations encountered a missing direct `serde` import
and two Clippy issues; these were fixed without adding a dependency. Cargo
reported duplicate-package warnings in cached Cell example vendoring, not test
failures.

### Timeouts and remaining gaps

- `cargo test --workspace --locked && cargo test --workspace --all-features --locked`
  timed out at 200 seconds in `source_proposal_launcher`. The chained matrix is
  incomplete; the all-feature workspace run is not established as completed.
- `cargo test -p scope-ide --locked --no-default-features && cargo test -p scope-ide --locked --features tools && cargo test -p scope-ide --locked --features tui`
  timed out at 200 seconds in `interactive_semantic_inspection`. This chain is
  incomplete, not a pass of all three complete configurations.

Neither broad chain was blindly repeated. Complete Full/PTY assurance remains
required by [the verification contract](../verification.md). Isolated library
feature tests and passing Clippy do not substitute for those integrations.

Fake-host coverage establishes envelope admission, full equality, denial,
policy-time and recapture-boundary mutations, ticket consumption, historical
reads, source-local coordinate validation, source-free projection and metadata
resource admission. It does not establish real App draft/registered-source
capture, installed Sprout closure execution, complete import discovery,
production policy/artifact enforcement, or closure-aware Details/paging UI.
Existing Sprout integration suites cover the unchanged single-source paths.

## I. Narrow next step

Complete **M0.4 integration**, before M0.5: add an isolated registered-editor
Sprout capture adapter that maps real document/language ownership, derives the
closure from the existing compiler input representation, selects the real
loaded-artifact/effective-policy identity, and invokes the explicit closure-aware
host policy through a concrete Scope-owned dispatch surface. Exercise denial,
substitution, post-policy edits and historical reads through that adapter without
adding a native evaluator. Resolve the timed-out Full/PTY verification separately.
Only after that evidence should **M0.5 — Versioned Contained Multi-Module
Execution** be scheduled. This report intentionally does not certify that boundary
as already complete.
