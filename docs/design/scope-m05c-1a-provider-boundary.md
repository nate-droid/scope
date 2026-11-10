# Scope M0.5c-1a — executable closure provider boundary

## Scope and inspected boundary

Extends the existing `experimental-sprout-closure` feature, not ordinary Check/Run.
No Sprout executable adapter, bundle evaluator, guest change, native fallback,
permission system, filesystem discovery, or UI was added.

The starting bridge retained prepared implementations behind an opaque `Any`
pin, always constructed `CounterHost`, reviewed `inert-counter`, retained only
`ClosureInspectionResult<u64>`, and reran discovery on every recapture. Context:
[M0.4b](sprout-m04b-concrete-closure-authorization.md),
[M0.4c admission](sprout-m04c-1-prepared-artifact-admission.md), and
[M0.4c verification](sprout-m04c-2-verification-closure.md).
The sibling Sprout report `docs/M05C1_AUTHORIZED_DISPATCH_BLOCKER.md` identifies
these same blockers; it is not a tracked file in this repository.

## Provider and ownership contract

`RegisteredDocuments<P = InertProvider>` retains the inert constructor and `u64`
result by default. `with_prepared_provider` consumes an inert registry, a prepared
descriptor, typed `T: Send + Sync + 'static`, and this trusted function:

```rust,ignore
fn(&T, ProviderRequest<'_>) -> Result<O, ProviderFailure>
```

`PreparedProvider<T, O>` stores an `Arc<T>` and that fixed function. The identical
Arc is pinned in `TrustedLoadedArtifact`; dispatch needs no `Any` downcast or
path reload. Prepared registries intentionally expose no artifact-only setter:
a host cannot update the reviewed pin while silently continuing to dispatch the
old typed implementation. Existing opaque admission remains compatible for the
inert pathway.

`TrustedClosureProvider` has associated `Output`, `identity`, and `dispatch`.
`ProviderRequest` has private construction and read-only access to the original
sealed discovery closure, binding, exact operation, effective execution policy,
and compiler-index-to-original-source mapping. Provider selection is host-owned;
providers receive neither policy callbacks nor mutable registry access. The shared
`HostSemanticPolicy` authorization pathway remains unchanged.

The trusted callback must bound output/work, enforce effective policy, and retain
immutable code/configuration identity. The interface does not sandbox native Rust
or prevent malicious interior mutation/ambient access. The descriptor is a host
assertion, not proof of authenticity. These obligations must be discharged by the
next contained execution adapter; this milestone supplies no production executor.

## Typed historical results

`RegisteredClosureResult<T = u64>` privately retains its generic result, original
discovery evidence, exact source snapshots and implementation pin. Accessors are
read-only; output cannot be swapped independently of evidence. A prepared dispatch
returns `RegisteredClosureResult<Result<O, ProviderFailure>>`: provider rejection,
limit failure, and failure are typed historical observations, not successful `O`
values. Authorization/staleness/before-dispatch failures remain `ClosureRejection`.

Historical reads neither discover, authorize, refresh text nor dispatch. Source
identity/revision and compiler source-index mappings remain the original ones.
Compile-fail tests preserve result privacy and demonstrate consumed-selection
replay rejection. No history storage or generalized value protocol was introduced.

## Non-compiling freshness

The private registered host implements the existing generic `recapture` contract
by validating live registry state and returning a clone of the original sealed
binding, not by capturing again. Checks before policy, after policy, before dispatch
and inside dispatch remain. Validation compares owner, complete catalog, host
policy, requested limits, dispatch ceiling, provider identity, artifact pointer and
selection generation. The descriptor, ABI and prepared policy are immutable behind
that exact artifact pointer. Compiler identity/options, effective compilation
limits, dependency graph and source-index mapping remain immutable sealed evidence;
they are not recomputed from a mutable discovery service.

Each participating logical module must retain registration identity, evaluation
identity, generic binding mapping, exact source text/version and language. Every
registry document must still match its registered evaluation identity (a direct
Document replacement violates this admission invariant). Valid unrelated edits do
not invalidate the closure. Discovery is private and fixed for the registry's
lifetime; changing a discovery adapter's hidden state cannot replace sealed compiler
evidence. New captures still perform bounded native discovery and source admission.

The registry owns Documents and dispatch metadata without shared interior
mutability. Its exclusive synchronous borrow covers final validation through
provider return; safe policy callbacks cannot mutate that registry. Tests retain
existing generic mutation-at-boundary rejection and check this registered pathway.
No atomic filesystem or general thread-safety claim is made. External state inside
a prepared implementation remains subject to the trusted callback contract above.

## Regression and verification evidence

Focused tests cover inert compatibility, exactly-once typed dispatch, denial,
root/dependency edits, identical-text identity replacement, artifact/policy/catalog
changes, cross-owner substitution, consumed selections, typed failure provenance,
passive historical reads and valid unrelated edits. A discovery-counting adapter
asserts the initial capture is the only discovery call, including inside the policy
callback and after dispatch/history reads. Existing generic tests cover mutations
at each authorization boundary and exact-binding substitution.

Commands run from Scope root (all Cargo commands use the root target cache):

- `cargo test --locked -p scope-ide --features experimental-sprout-closure --lib registered_document_closure::`
  — initial compile error corrected; an old rediscovery-based expectation was then
  updated to assert original sealed compiler evidence. Seven existing tests passed.
- `cargo test --locked -p scope-ide --lib --features experimental-sprout-closure registered_document_closure::tests`
  — 12 tests passed after provider regressions. An initial agent command without
  the feature matched zero tests and was not treated as evidence.
- `cargo test --locked -p scope-ide --features experimental-sprout-closure --lib`
  — 429 passed.
- `cargo test --locked --manifest-path ../sprout/Cargo.toml -p sprout --features closure-discovery --test registered_document_closure --test closure_discovery`
  — 15 registered-closure and 2 discovery tests passed, including real prepared
  artifact retention and policy-time substitution regressions.
- `cargo fmt --all -- --check` — passed.
- `cargo test --locked -p scope-ide --features experimental-sprout-closure --doc`
  — passed, including privacy and replay compile-fail coverage.
- `cargo test --locked -p scope-execution --all-features` — passed, including real
  Wasm execution and all 13 policy tests.
- `cargo test --workspace --all-features --locked` — **incomplete**, timed out at
  200 seconds in `scope-ide`'s `downstream_launcher`, after entering its five tests.
  Not repeated; earlier targeted passes remain valid.
- `cargo clippy --locked -p scope-ide --all-targets --features experimental-sprout-closure -- -D warnings`
  — passed (Cargo emitted its duplicate upstream package notice).
- `cargo test --locked -p scope-ide --no-default-features --lib` — 312 passed.
- `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`
  — 5 passed.
- `python3 -B .github/scripts/check_markdown_links.py` — passed before this report;
  final documentation checks are recorded in the completion response.

A read-only independent diff review found no concrete security/correctness defect.
No guest rebuild or live/paid model call was made.

## Remaining Full obligations

This is not Full security sign-off. Under [the verification contract](../verification.md),
CI/maintainers still need successful workspace default and complete all-feature
tests; isolated full tools/TUI/feature-absence suites; workspace and isolated
strict-lint configurations; warning-free all-feature rustdoc; standalone downstream,
harness and managed-transfer checks; and scaffold checks. The bounded all-feature
attempt above did not finish PTY coverage. Those commands were not all launched
locally after the timeout; targeted libraries and lint are not substitutes. Existing
M0.4c verification limitations remain historical open obligations, not passes here.

Stop here: connecting Sprout execution is explicitly deferred.
