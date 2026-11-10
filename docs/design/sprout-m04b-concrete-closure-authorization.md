# Sprout / Scope M0.4b — concrete closure authorization

## Status and scope

Implemented an opt-in, Scope-owned registered-Document authorization pathway with
real Sprout discovery and **inert counting dispatch only**. No multi-module Wasm
execution, native fallback, new guest protocol, filesystem module discovery, save
capability, or editable proposal was added.

This is not unconditional security sign-off. Full all-feature integration
verification remains incomplete, strict Sprout lint checks have residual failures,
and the concrete dispatch uses an explicitly trusted test/host artifact handle,
not a prepared executable component. Do not treat the counter result as Sprout
program output or contained multi-module parity.

## A. Baseline and ownership

Scope's existing `closure_inspection` contract seals sources, graph, operation,
compiler metadata, execution policy and artifact identity. It consumes invocation
tickets and recaptures before policy, after policy and before dispatch. Its
original callback was not itself an integration with semantic-tool enforcement.

`Document::evaluation_source_id()` is the actual Arc-backed language-layer source
identity. `DocumentId`, paths and revision numbers alone are insufficient:
constructing a reopened Document allocates a new evaluation identity. The generic
closure contract instead uses `source_edit::SourceId`; these types must not be
compared by numeric equality.

At initial inspection Scope's tree was clean. Sprout already had changes to
`src/executable.rs` and untracked M03 documentation/coordinator/tests. Those were
preserved, not reverted or attributed entirely to this milestone. No clean,
pre-edit baseline suite was completed; recorded M04 results were inspected but
are not newly reproduced baseline evidence. Focused existing suites were run
against the evolving implementation, followed by current-source regressions.
Consequently, residual lint failures are not claimed to be empirically reproduced
clean-baseline failures.

## B. Concrete source and compiler adapters

Scope adds `registered_document_closure` behind the default-off
`experimental-sprout-closure` feature. `RegisteredDocuments` owns actual Documents,
a registry owner identity, monotonic registration identities, and all mutable
operation/policy/artifact-selection state. Each entry explicitly maps its real
Document evaluation identity to a registry-local generic binding identity.
Registration labels are compiler transport labels, not evidence of ownership.

`register_with_language` records trusted host language metadata;
`register` is the explicit host assertion that a Document is Sprout. Capture
copies exact in-memory text/version, never saves or rereads disk, and rejects a
Document replacement that no longer matches its registered evaluation identity.
Removal and re-registration create new binding identities even for identical
text/path/version. Historical language-layer snapshots retain the original Arc
identity and text after closure of the registry.

Scope cannot depend on Sprout: Sprout already depends on Scope. The resulting
boundary is therefore a Scope-defined read-only `Discovery` trait with bounded
owned evidence. Sprout's default-off `closure-discovery` feature enables the Scope
experimental feature and supplies `SproutDiscovery`. Scope validates returned
source evidence against registered inputs; Sprout remains trusted to define and
validate the language graph.

Sprout reuses the real module parser, registered-source acquisition, sealed
compilation input, aggregate admission and static checker. No second import
parser was added. The façade returns canonical sources, graph, public declarations,
effective limits, identity version and portable equality material. Compiler index
mapping comes directly from `CompilationInput::units()`, not registry order.
Portable equality bytes contain source and remain sensitive, never review text.
A SHA-256 content identity is exposed as source-free compiler metadata; this is
not compiler-binary attestation or authenticity evidence.

Only read-only infrastructure gates were widened. Native closure evaluation and
M03 simulated grants/coordinator dispatch remain test-only. Ordinary production
single-source parsing and Check/Run remain on their existing route.

## C. Actual authorization integration

`tool::semantic` now defines `HostSemanticPolicy` and explicit
`HostAuthorizationRequest::{Single, Closure}`. Existing semantic-provider dispatch
and final proposal registration call the shared `authorize_host_request` boundary.
Legacy single-source callbacks remain supported, but **always deny closure
requests**. A root-only grant cannot accidentally authorize dependencies.

The concrete path calls this same boundary with the complete sealed binding and
its exact bounded review. The boundary verifies that the review matches the
binding before calling host policy. The language discovery provider has no policy
or dispatch authority.

```text
Owned registered Documents
  -> Sprout read-only discovery and static validation
  -> Scope immutable closure plus retained actual source identities
  -> exact source-free review
  -> shared Scope host-policy boundary (Closure request)
  -> complete recapture / equality validation
  -> sealed binding supplied to inert bounded counter
  -> read-only retained observation
```

Bindings include the selected operation and complete catalog, canonical graph,
exports, compiler source-index mapping, requested/effective limits, identity
version, compiler content identity, effective inert policy and artifact generation.
No second CandidateEvaluation operation or ordinary Check/Run routing was added.
Every explicit invocation requests fresh authorization. The concrete selection is
non-Clone and consumed on invocation; the underlying invocation ticket is also
single-use. There is no reusable approval object.

## D. Freshness, substitution and concurrency

Recapture uses current owned Documents and current catalog, limits, policy and
artifact selection. It compares the generic binding and exact portable compiler
identity against the original, then dispatches the original sealed binding.
New dependency bytes are never substituted under old approval.

Real compiler integration tests cover root/dependency edits, replacement/reopen,
removal, operation/catalog, limits, policy and artifact changes; separate
reentrant test hosts exercise mutation during policy. Those test hosts are
explicitly not the production registry mutation model. The ordinary path holds
an exclusive synchronous registry borrow across policy, final recapture and
counter dispatch; callbacks cannot borrow that registry mutably. There is no
filesystem transaction or protection against a dishonest externally mutable
Discovery implementation: the trusted Discovery contract prohibits that behavior.
No shared mutable editor references are retained by the registry.

Unrelated registered edits are excluded from the sealed reachable source table
and do not ordinarily stale it. Registry-wide admission, mapping integrity and
catalog validation still apply: exhausting the bounded registry or corrupting an
unrelated ownership mapping can cause recapture failure. This is fail-closed,
not a claim of isolation from every possible invalid registry state.

Equal revision with different text is covered by generic contract tests; normal
Document edits monotonically advance version. Generic tests also establish
boundary rejection and replay/substitution properties. Those tests are not
misrepresented as arbitrary live-editor mutation capabilities of the concrete
exclusive-ownership API.

## E. Dispatch and historical evidence

The only concrete dispatch implementation is a bounded counter. It accepts the
sealed binding, validates it again, increments once, and returns controlled
read-only evidence. It cannot call Sprout main, a native evaluator, a guest,
filesystem imports, network, or source-edit machinery.

`RegisteredClosureResult` inseparably retains the invocation result, discovered
closure, exact historical source snapshots and compiler-index mapping. It
implements passive `SourceResolver`; `review`, `compiler_source` and result reads
never acquire sources, request permission or dispatch. An independent review
found that a public embedded result could be swapped away from its retained
sources; it was made private with shared-reference accessors and a passing
compile-fail regression.

Historical identity is not current freshness, current component selection or
current authorization. Retained reads do not declare an observation fresh and
confer no new execution authority. There is no persistent history or new tree UI;
the existing bounded closure review provides textual inspection.

`capture_with_rejection` preserves typed discovery phase/category and only a
known actual root identity. Unknown roots and missing dependencies receive no
invented location. Authorization/freshness/dispatch use existing typed
`ClosureRejection`. Compiler diagnostics are not exported as arbitrary error
strings or fabricated source locations. Successful compiler-source ownership is
mapped precisely; richer rejected partial-closure source-local diagnostics remain
unimplemented. Successful observations and capture rejections are distinct types.

## F. Artifact and Wasm-first audit

`TrustedLoadedArtifact` is an opaque process-local trusted handle. Replacing it
changes artifact generation and stales a pending closure. It is **not a fabricated
loaded digest** and does not claim executable authenticity. The concrete pathway
has no executable ABI: its dispatch target is explicitly `inert-counter`.
Binding a future component ABI and admitted runtime settings is still required
before that provider may execute anything.

Separately, Sprout's contained `Host::identity()` exposes its retained loaded-byte
SHA-256, selected path label and existing ABI. `prepare()` caches the loaded host;
changing a configured path or its file does not replace already retained bytes.
The accessor does not reread or execute. Preparation itself performs filesystem
I/O and native component loading/compilation. Its identity accessor is available
for later admission, but **is not wired to the inert registry's artifact handle**.
A digest identifies bytes, not authenticity. Current tests check the accessor's
stored identity, not a full cache/file-replacement scenario.

| Processing | Classification and authority |
| --- | --- |
| Document snapshots and explicit identity mapping | Trusted host metadata handling; sensitive source retention, no disk save |
| Ordinary Sprout Check | Native parsing/checking of untrusted single-source text |
| Experimental closure discovery/static validation | Native parsing/checking of untrusted registered text, bounded; not contained |
| Review and host policy | Trusted Scope metadata handling and permission enforcement |
| Component loading/compilation | Trusted host admission of untrusted component bytes, native preparation |
| Existing guest parsing/type checking | Contained Wasm, existing single-source ABI only |
| Existing guest evaluation | Contained Wasm, managed runtime policy; unchanged |
| Host protocol decoding | Trusted host validation of untrusted bounded output; unchanged |
| Retained evidence presentation | Passive trusted host presentation, source-free review; source resolver is sensitive |
| Future multi-module parse/resolve/check/evaluate | Future contained processing, not implemented |

M0.5 must independently validate the admitted bundle inside Cell, bind actual
loaded bytes/ABI and effective runtime policy, provide no ambient filesystem or
network imports, and validate bounded output before retaining it. Native discovery
must not become the guest's sole proof of bundle correctness. No changes were
made to `sprout:c2/evaluate-v4`, guest behavior, tokenizer, resource ceilings or
component bytes. The guest manifest recognizes the shared source's feature name
through lint-only `check-cfg`, not an activatable guest feature; no component
rebuild is warranted. Independent final review caught and corrected an initial
feature declaration that would have enabled native-only modules in the guest.

## G. Compatibility and review

Single-document semantic callbacks retain their API through the legacy policy
implementation. Ordinary App Check, Ctrl-R Run, semantic inspection, source
proposal validation/registration, draft publication and disk-save distinctions
remain covered by existing suites. Independent read-only review found no further
confirmed authority bypass after the retained-result sealing fix. This is bounded
review evidence, not proof of absent vulnerabilities.

## H. Verification evidence

All commands reused root artifacts, used bounded timeouts, and made no live/paid
model calls. Cargo emitted cached Cell duplicate-package warnings. No cargo clean,
component rebuild/re-encoding, branch or commit was performed. Focused evidence
below came from implementation subagents; broader evidence came from a separate
verification subagent. Final Scope library/doctest/Clippy commands were also run
by the coordinating agent after the final Scope changes.

### Focused and final checks

From Scope root, with `--locked -p scope-ide --lib`:

| Remaining arguments | Result |
| --- | --- |
| `--features experimental-sprout-closure registered_document_closure::tests` | 7 passed after language/rejection changes |
| `--features experimental-sprout-closure tool::semantic::tests` | 21 passed |
| `--features experimental-sprout-closure closure_inspection::tests` | 14 passed before final adapter-only changes |
| `--features experimental-sprout-closure semantic_inspection::tests` | 11 passed |
| `--all-features` | Passed again after final Scope changes (earlier pre-final run: 882 passed) |

Also passed:

```sh
cargo test --locked -p scope-ide --doc --features experimental-sprout-closure
cargo clippy --locked -p scope-ide --all-features --all-targets -- -D warnings
cargo fmt --all -- --check
```

Sprout commands from its root unless noted:

| Command | Result |
| --- | --- |
| `cargo test --locked --offline -p sprout --lib --features closure-discovery m03` | 32 passed |
| Same command with filter `m02` | 21 passed |
| Same command with filter `compilation_input` | 13 passed |
| Same command with filter `closure_discovery` | 5 passed |
| `cargo test --locked --offline -p sprout --features closure-discovery --test registered_document_closure` | Final: 9 passed |
| `cargo test --locked --offline` | 302 passed, 1 ignored before final narrow compatibility fixes |
| `cargo test --locked --offline --features closure-discovery` | Final: 318 passed, 0 failed, 1 ignored |
| `cargo check --locked --offline -p sprout --lib --features closure-discovery` | Passed |
| `cargo test --locked --offline -p sprout --no-default-features --test b05_processing` | 5 passed |
| `cargo fmt --all -- --check` | Passed |
| `git --no-pager diff --check` (both repositories) | Passed |

The first complete feature-root attempt failed with E0283 in unchanged
`b05_processing` string comparisons. Explicit `as_str()` comparisons fixed the
underconstrained type without changing semantics; the complete final suite above
then passed. New feature-related dead-code and guest unexpected-cfg warnings were
also corrected by narrow gates and manifest declaration.

### Full Scope matrix and outstanding PTYs

These broader runs preceded the final retained-result/language/rejection changes;
the final all-feature library, doctest and Clippy passes above cover those changes,
but the earlier broad passes are not asserted to be final-tree integration runs.

| Exact command | Result |
| --- | --- |
| `cargo test --locked -p scope-ide --features tui,agent --test source_proposal_launcher` | 1 passed, 1 ignored PTY child; build 8.38s, tests 147.99s |
| `cargo test --locked -p scope-ide --features tui --test interactive_semantic_inspection` | 8 passed, 1 ignored PTY child; build 4.21s, tests 10.69s |
| `cargo test --workspace --locked` | Passed including doctests and workbench PTYs |
| `cargo test --workspace --all-features --locked` | Timed out at 570s, outer bound 600s; no reliable partial count, not repeated |
| `cargo test -p scope-ide --locked --features tools` | Passed |
| `cargo test -p scope-ide --locked --features tui` | Passed |
| `cargo test -p scope-ide --locked --no-default-features` | Passed |
| `cargo test --locked --manifest-path examples/downstream/Cargo.toml` | Passed |
| `cargo test --locked --manifest-path examples/harness/Cargo.toml` | 7 passed |
| `cargo run --locked --manifest-path examples/harness/Cargo.toml` | Passed approval/denial scenarios |
| `cargo test --locked --manifest-path examples/managed-transfer/Cargo.toml` | 1 passed |
| `cargo run --locked -p xtask -- check-downstream --offline` | Passed including generated-language scaffold |

Large passing-suite aggregate counts were not retained in bounded output and are
not invented here. The timeout's newly orphaned PTY child pair was stopped; older
processes were left untouched. Artifact-reusing measurements are not cold builds.
During broader verification root target grew from 24 to 31 GiB and available disk
fell from 23 to 16 GiB; no disk-exhaustion error occurred.

The following broader commands also passed:

```sh
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p scope-ide --all-targets --features tools --locked -- -D warnings
cargo clippy -p scope-ide --all-targets --features tui --locked -- -D warnings
cargo clippy -p scope-ide --all-targets --no-default-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
cargo fmt --manifest-path examples/managed-transfer/Cargo.toml -- --check
python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v
python3 -B .github/scripts/check_markdown_links.py
```

Python ran 5 tests. Documentation checks are rerun for this report separately.

### Guest/host regressions and remaining failures

From `sprout/experiments/c2/guest`:

```sh
cargo test --locked --offline --test abi
cargo check --locked --offline --lib
cargo clippy --locked --offline --lib -- -D warnings
```

ABI: 2 passed; production library check passed. After the final guest cfg fix,
`cargo check --locked --offline --lib --all-features` and
`cargo test --locked --offline --all-features --test abi` also passed (2 tests),
without warnings. Guest metadata declares no features and adds no dependencies.
Strict library Clippy fails on
four diagnostics shared with root code: duplicate names module loading, two
collapsible conditionals and a question-mark suggestion. These remain uncorrected
to avoid unrelated lint churn; no clean-baseline build was performed.

From `sprout/experiments/c2/host`:

```sh
cargo test --locked --offline
cargo clippy --locked --offline --all-targets -- -D warnings
```

Tests: 8 passed; Clippy passed. Root strict Clippy was attempted with and without
`closure-discovery`, `--all-targets`, and again feature-enabled `--lib` after gate
fixes. Final production-library diagnostics are the same four listed above;
earlier all-target checks also encountered internal test-code lint issues.

Guest all-target Clippy and guest/host standalone format checks encountered shared
source test-module resolution (`sprout/src/tests.rs`), missing test dependencies
and guest-unavailable contained state. Source inspection shows the test-inclusion
structure predates these changes; these are residual checks, not claimed passes.
They were not rewritten or rerun after the final production-only gate fixes.
Root Sprout formatting passes. No guest ABI changes were made.

Full workspace all-feature integration, a separately complete IDE all-feature
integration run, final-tree broad reruns, hosted-platform parity, and clean Sprout
strict lint/standalone-format evidence remain outstanding. The current focused
and library passes do not waive those requirements.

## I. Next milestone recommendation

Recommend **M0.4c — Closure Admission and Verification Closure**, not immediate
M0.5 implementation. Keep it narrowly scoped:

1. Close the remaining all-feature integration timeout with preserved diagnostic
   logs and complete the required final-tree Full evidence.
2. Resolve or formally baseline the residual Sprout/standalone lint and format
   failures without weakening coverage.
3. Specify the executable provider's exact prepared-component ABI/policy identity
   adapter and test cached-byte identity behavior. Do not mistake the inert
   handle for that enforcement.
4. Decide whether richer rejected partial-closure diagnostics are required before
   bundle execution; retain only known owners and valid source-local locations.

The real source-truth/host-policy bridge is established for this inert pathway:
only its reviewed immutable closure reaches its counter under a single-use
invocation. Extending that statement to an executable component requires the
admission work above. M0.5 can then introduce a versioned contained bundle ABI and
independent guest validation without reopening a native execution escape hatch.
