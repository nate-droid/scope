# Sprout / Scope M0.4c-1 — prepared artifact admission

## Boundary

Scope's experimental `TrustedLoadedArtifact::prepared` accepts a host-supplied
immutable `PreparedImplementationDescriptor` (loaded-byte SHA-256, required ABI,
effective execution-policy metadata) and an owned, type-erased prepared-host pin.
Scope does not depend on Sprout. Descriptor validation is structural; the trusted
language host remains responsible for deriving truthful metadata from admitted
bytes. A digest is byte identity, not authenticity or proof of Sprout semantics.
The legacy default handle remains explicitly inert test/host infrastructure; it
has no prepared descriptor and is not executable admission.

Sprout's feature-gated `prepared_host_artifact`, `shared_host_artifact`, and
`prepare_host_artifact` adapters use `Host::identity()` after import-free ABI
admission. The SHA-256 comes from the bytes compiled and retained by that Host,
not a later path read. The policy snapshot derives ceilings from the same recipe
as `policy()`, recording the five-second relative timeout rather than a reusable
absolute deadline. No ceilings or capabilities change.

The registry seals digest and selection generation in artifact identity, and ABI
and effective policy in execution-policy metadata alongside the existing host
policy. Sources, compiler identity, operation and catalog retain their M0.4b
binding. Selecting another handle increments generation even for identical bytes.
Cloning a handle retains its selection identity. Pending selections and retained
results own the original pin. The registry holds an exclusive borrow through
recapture, authorization and dispatch; its only provider remains the inert
counter, which receives the original sealed binding after exact recapture.
There is no independently reloading dispatch path or executable provider.

## Evidence

Actual commands run from `scope/`, serially using its configured root target:

- `cargo test --locked --manifest-path ../sprout/Cargo.toml -p sprout --features closure-discovery --test registered_document_closure prepared_adapter_ -- --nocapture`
  — initial adapter checkpoint: 4 passed.
- `cargo test --locked --manifest-path ../sprout/Cargo.toml -p sprout --features closure-discovery --test registered_document_closure --test closure_discovery`
  — expanded coverage: 15 + 2 passed (including six prepared-adapter tests).
- `cargo test --locked -p scope-ide --features experimental-sprout-closure --lib closure`
  — 22 passed, including exact binding, boundary mutation, denial, single-use
  invocation, passive history and concrete registry regressions.
- `cargo test --locked -p scope-ide --features experimental-sprout-closure --lib tool::semantic::tests`
  — 21 passed, including shared policy and existing single-source dispatch.
- `cargo test --locked --manifest-path ../sprout/Cargo.toml -p sprout --features closure-discovery --lib contained::tests::prepared_identity_describes_loaded_bytes_without_execution -- --exact`
  — 1 passed.
- `cargo fmt --all -- --check`,
  `cargo fmt --manifest-path ../sprout/Cargo.toml -- --check`,
  `git --no-pager diff --check`, and
  `git --no-pager -C ../sprout diff --check` — passed.

- `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`
  — 5 passed.
- `python3 -B .github/scripts/check_markdown_links.py` — passed.

Real prepared-host regressions cover loaded A surviving path replacement, new B
selection (including identical bytes), ABI failure, failed preparation followed
by success, denial, stale dependency with unchanged artifact, and passive retained
A artifact/source evidence after B selection. One explicitly test-owned descriptor
and pin exercises effective-policy substitution; it is not a changed runtime
configuration or evidence that arbitrary fake metadata describes a real Host.
Root-only policy closure denial and single-use tickets reuse existing tests.
No program was evaluated natively or in Wasm by these commands. No guest rebuild.

## Compatibility and remaining obligations

Single-document Check/Run routing, evaluate-v4, sprout-preview-1, source registry,
syntax, tokenizer and component imports are unchanged. The shared policy tests
passed; the full single-document/real-Wasm execution and PTY matrices were not
rerun because this slice forbids program execution and reserves broad escalation
for M0.4c-2.

This is a tested inert admission checkpoint, **not complete security sign-off**.
M0.4c-2 must complete the authorization/host-adapter Full obligations in
`docs/verification.md`: complete affected security/policy and real-Wasm suites,
workspace default/all-feature tests and lint, isolated feature absence/tools/TUI,
docs, applicable downstream/scaffold checks, and relevant standalone Sprout
checks. Preserve stale-source, rejection, execution-policy and PTY coverage.
No known implementation blocker requires a Scope policy refactor. An executable
closure provider, contained bundle validation and multi-module protocol remain
out of scope; any future provider must consume the retained prepared selection
and its bound policy rather than reload by path.
