# Sprout / Scope M0.4c-2 — verification closure

## Decision: BLOCKED

No authorization defect was discovered by the completed regressions. Do not begin
M0.5 on the strength of this report: the exact all-feature workspace run remains
incomplete, and Sprout strict lint / standalone guest formatting remain failed.
No runtime, authority, ABI, tokenizer, language or production source was changed.

## Provenance and evidence

Execution date: 2026-11-10, macOS, configured repository toolchain. Scope started
clean at `acc319780b405c83d60ae2bbda73778b953a941b`. Sprout HEAD was
`00b5600b3c1bf16b821fc113d0f88981ec130050`, with the pre-existing dirty M03/M04
implementation, including untracked closure adapters/tests. That dirty tree—not
Sprout HEAD alone—is the subject of the fresh tests. Its status was unchanged at
the end. Scope's only resulting change is this report.

Historical evidence: [M0.4b report](sprout-m04b-concrete-closure-authorization.md)
and [M0.4c-1 report](sprout-m04c-1-prepared-artifact-admission.md). Those reports
explicitly reserve Full/final-tree integration completion. Their earlier broad
passes are context, not substituted for fresh final-tree results here.

New raw output and exact command/exit/wall-time records are preserved locally in
`target/m04c-2/*.log` and `batch1.json` through `batch6.json` (ignored artifacts,
not durable CI evidence). All Cargo invocations ran serially from Scope root,
reusing its configured target; no clean, guest rebuild, live model call or ceiling
increase. Bounds terminated the newly launched process groups, not pre-existing
processes. Some pre-existing PTY children remain; they were not killed.

## A. Verification matrix

Commands below ran from Scope root. `S` means
`--manifest-path ../sprout/Cargo.toml`; `G` means
`--manifest-path ../sprout/experiments/c2/guest/Cargo.toml`; `H` means
`--manifest-path ../sprout/experiments/c2/host/Cargo.toml`. Expand these aliases
literally; they are report abbreviations, not shell variables. Evidence names
identify `target/m04c-2/<name>.log`.

| Status | Exact command (with abbreviations above) | Fresh evidence |
| --- | --- | --- |
| Passed | `cargo test --locked -p scope-ide --features experimental-sprout-closure --lib` | `scope-security`: 424 passed; 0.77s wall |
| Passed | `cargo test --locked -p scope-execution --all-features` | `execution`: 27 passed, including 13 policy tests; 2.73s wall |
| Passed | `cargo test --workspace --all-features --locked --lib` | `all-feature-lib`: 1003 passed; 16.98s wall |
| Incomplete (invocation) | `cargo test --workspace --locked` | `workspace-default`: 240s bound at source-proposal launcher |
| Passed (partitioned coverage) | `cargo test --workspace --locked -- --skip source_proposals_through_real_launcher` plus the `source-tui` command below | `default-remainder`: 180.55s; omitted journey separately passed using the same printed test executable `source_proposal_launcher-0940e95a154b6219` |
| Incomplete | `cargo test --workspace --all-features --locked --tests` | `all-feature-integration`: 300s bound at source-proposal launcher |
| Passed | `cargo test --workspace --all-features --locked --doc` | `all-feature-doc`: 6.24s |
| Passed | `cargo test -p scope-ide --locked --features tools` | `tools`: 6.01s |
| Passed | `cargo test -p scope-ide --locked --no-default-features` | `minimal`: 5.73s |
| Incomplete (invocation) | `cargo test -p scope-ide --locked --features tui` | `tui`: 180s bound at source-proposal launcher |
| Passed (partitioned coverage) | `cargo test -p scope-ide --locked --features tui -- --skip source_proposals_through_real_launcher` | `tui-remainder`: 107.46s; combine with next row |
| Passed | `cargo test --locked -p scope-ide --features tui --test source_proposal_launcher -- --nocapture` | `source-tui`: journey passed, 1 ignored PTY child; 148.38s |
| Passed | `cargo test --locked -p scope-ide --all-features --test source_proposal_launcher -- --nocapture` | `source-isolated`: journey passed, 1 ignored PTY child; 148.30s |
| Incomplete (invocation) | `cargo test --workspace --all-features --locked -- --skip source_proposals_through_real_launcher` | `all-feature-remainder`: 240s bound as Script workspace-semantic target started; all earlier reported targets passed |
| Passed (isolated, not exact workspace feature-unification proof) | `cargo test --locked -p scope-script --all-features --test workspace_semantic` | `script-semantic`: 10.01s |
| Passed (isolated tail) | `cargo test --locked --all-features -p scope-terminal -p scope-types -p scope-workbench -p xtask` | `all-feature-tail`: 69.00s; includes real workbench PTYs |
| Passed | `cargo test --locked S` | `sprout-default`: 302 passed, 1 intentionally ignored tokenizer test; 62.62s |
| Passed | `cargo test --locked S --features closure-discovery` | `sprout-closure`: 324 passed, same ignored test; 60.28s; 15 registered-closure + 2 discovery integration tests |
| Passed | `cargo test --locked G --all-features --test abi` | `guest-abi`: 2 passed; 0.17s |
| Passed | `cargo check --locked G --lib --all-features` | `guest-check`: 0.04s |
| Passed | `cargo test --locked H` | `host`: 8 passed; 9.08s |
| Passed | `cargo fmt --all -- --check` | `fmt` |
| Passed | `cargo fmt S -- --check` | `sprout-fmt` |
| Failed | `cargo fmt G -- --check` | `guest-fmt`: cannot resolve shared `src/tests.rs` |
| Passed | `cargo fmt H -- --check` | `host-fmt` |
| Passed | `cargo clippy --workspace --all-targets --locked -- -D warnings` | `clippy-default` |
| Passed | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | `clippy-all` |
| Passed | `cargo clippy -p scope-ide --all-targets --features tools --locked -- -D warnings` | `clippy-tools` |
| Passed | `cargo clippy -p scope-ide --all-targets --features tui --locked -- -D warnings` | `clippy-tui` |
| Passed | `cargo clippy -p scope-ide --all-targets --no-default-features --locked -- -D warnings` | `clippy-minimal` |
| Failed | `cargo clippy --locked S --all-targets -- -D warnings` | `sprout-clippy`: production and test lints below |
| Failed | `cargo clippy --locked S --all-targets --features closure-discovery -- -D warnings` | `sprout-clippy-closure`: same diagnostics |
| Failed | `cargo clippy --locked G --lib -- -D warnings` | `guest-clippy`: four production diagnostics |
| Failed | `cargo clippy --locked G --all-targets -- -D warnings` | `guest-clippy-targets`: shared test compilation errors plus lints |
| Passed | `cargo clippy --locked H --all-targets -- -D warnings` | `host-clippy` |
| Passed | `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked` | `rustdoc` |
| Passed | `cargo test --locked --manifest-path examples/downstream/Cargo.toml` | `downstream` |
| Passed | `cargo test --locked --manifest-path examples/harness/Cargo.toml` | `harness-test`: 7 passed |
| Passed | `cargo run --locked --manifest-path examples/harness/Cargo.toml` | `harness-run`: approval/denial scenarios |
| Passed | `cargo fmt --manifest-path examples/managed-transfer/Cargo.toml -- --check` | `transfer-fmt` |
| Passed | `cargo test --locked --manifest-path examples/managed-transfer/Cargo.toml` | `transfer-test`: 1 passed |
| Passed | `cargo run --locked -p xtask -- check-downstream --offline` | `scaffold`: generated consumer checks; no tracked lockfile changes |
| Passed | `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v` | `python`: 5 passed |
| Passed | `python3 -B .github/scripts/check_markdown_links.py` | `links`; rerun after report creation |
| Passed | `python3 -B crates/scope-ide/tests/test_pty_sync.py -v` | `pty-sync`: 9 passed |
| Passed | `git --no-pager diff --check` and `git --no-pager -C ../sprout diff --check` | No whitespace errors |
| Not Applicable | New executable closure provider, multi-module evaluation, rebuilt guest | Forbidden/out of scope; inert dispatch retained |

The exact unpartitioned all-feature workspace command was not repeated. Fresh
isolated checks cover its unfinished targets, but do not establish identical
workspace-wide feature unification. This report does not call that Full row green.
Hosted platform parity was not checked.

## B. Security findings

No actual authorization, artifact substitution, freshness or policy defect was
observed. Scope's complete experimental library and all-feature library suites
passed, as did the real Sprout adapter tests. Existing tests exercised:

- Real registered documents, explicit dependency mapping, compiler/language
  identity and complete closure admission; fabricated sources rejected pre-policy.
- Exact binding and mutations at authorization boundaries; dependency, operation,
  policy, artifact/selection and catalog substitutions rejected.
- Root-only policy closure denial, single-use tickets and denial recovery.
- Six prepared-adapter tests: actual policy recipe/relative timeout, ABI failure
  and recovery, loaded A surviving path replacement, B invalidating selection even
  for identical bytes, test-owned policy substitution, stale dependency, denial
  and passive retained history after B.
- Retained evidence without rerun and existing semantic/single-document dispatch.
- Real Wasm execution and rejection: fuel/memory, deadlines/cancellation, ABI,
  payload and nominal output validation (not mock substitutes).

A host-supplied descriptor is still trusted metadata, not authenticity proof.
The policy-substitution fixture is explicitly test-owned; it does not establish
that arbitrary descriptors truthfully describe a real Host. No executable closure
provider was tested or introduced. These findings apply to inert admission plus
existing single-document execution, not future multi-module execution.

## C. Fixes and lint baseline

No source fixes or new tests: no demonstrated security acceptance gap warranted
architecture changes. Verification failures were retained, not suppressed.

Reproduced production Sprout/guest strict-Clippy diagnostics:

- `duplicate_mod`: `names.rs` loaded from `executable/adapter.rs:67`,
  `parser.rs:2`, and (when enabled) `compilation_link.rs:5`.
- `collapsible_if`: `executable.rs:461` and `static_check.rs:512`.
- `question_mark`: `static_check.rs:613`.

Root all-target lint additionally reports cloned-reference slice construction at
`executable/c1_workloads.rs:57` and `l4b_tests.rs:331`, and manual
`is_multiple_of` at `c2_tests.rs:91`.

Source ancestry inspection against Sprout HEAD shows the duplicate inclusion
structure and these unchanged lint sites predate the milestone. However, M0.4b
widens `compilation_link` from test-only to test-or-closure-discovery, exposing an
additional duplicate include in feature-enabled production lint. This is not
accurately described as wholly unrelated baseline debt. No clean-HEAD build was
run; ancestry is not proof of a passing/failing historical toolchain baseline.

Guest formatting fails before checking layout because shared `static_check.rs`
contains `#[cfg(test)] mod tests`, resolving an absent `sprout/src/tests.rs`.
Guest all-target Clippy also forces shared root test code needing `sha2` and
`crate::contained`, unavailable in the guest. The external shared-source inclusion
and guest `test = false` / `doctest = false` predate the milestone. The new cfg
recognition does not activate closure-discovery in the guest. Production guest
check and its two ABI integration tests pass; strict production lint still fails.
Do not add host runtime dependencies to the guest merely to silence test errors.

## D. Performance observations

The original M0.4b timeout transcript was not found among inspected preserved
logs. Its exact active test cannot be reconstructed from fingerprints. Old PTY
processes alone do not establish lock contention or deadlock.

Fresh bounds locate the expensive path: default, all-feature integration and TUI
all reached `source_proposal_launcher`; their Cargo build phases were respectively
0.52s, 25.76s and 0.17s, versus 240s, 300s and 180s total bounds. An isolated
all-feature source journey then passed in 148.30s, and the TUI variant in 148.38s.
The all-feature remainder advanced through Script tests to the start of
`workspace_semantic` before its cumulative 240s bound. That target passed alone.
No fresh timeout reported a failing assertion or disk-exhaustion error.

These are artifact-reusing/partially cached measurements, not cold builds. Sprout
root build phases were 11.32s default and 6.02s closure-enabled; total walls were
62.62s and 60.28s. Host build was 2.38s, test harness 5.65s. Real Scope execution
completed in 2.73s total. Free disk was about 8.8GiB at one observation: a resource
risk, not evidence of this timeout's cause. No competing Cargo builds were run.

Concrete follow-up: schedule long proposal and workbench PTYs independently,
retain target-level output/timing and child lifecycle evidence, and use a process
watchdog with child cleanup. Rust launcher wrappers buffer Python `.output()`,
limiting live progress visibility. Preserve rejection/stale-source/exit assertions
and existing runtime ceilings. For exact workspace-unified closure, partition by
explicit test selection while retaining the full workspace package/feature set,
or use CI with adequate aggregate runtime; do not replace PTYs with mocks or
repeat blind increasingly long local workspace runs.

## E. Remaining gaps and smallest corrective tasks

1. Obtain successful exact all-feature workspace coverage with identical workspace
   feature unification, including binaries, integrations and doctests. Current
   evidence has complete library/doctest passes and passing isolated remainder,
   but the broad test invocation timed out. Acceptance: retained successful
   command/transcript or demonstrably equivalent workspace-unified partitions.
2. Resolve Sprout duplicate-module production lint narrowly; address remaining
   pre-existing lint sites in a separately scoped cleanup or explicitly accept
   that debt under maintainer policy. Acceptance: required strict checks pass,
   without rule suppression or semantic changes.
3. Repair shared-source guest formatting/test traversal without importing host
   capabilities, removing tests or changing language semantics. Acceptance:
   standalone format and intended all-target lint can run, production guest
   check and ABI regressions remain green.
4. Preserve evidence against the actual dirty Sprout source revision when it is
   committed; this local report is not a cross-platform CI attestation.

After these acceptance conditions close, recommend **M0.5 — Versioned Contained
Multi-Module Execution**. No M0.5 implementation is included here.
