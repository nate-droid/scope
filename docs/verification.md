# Verification policy

Run from the repository root with the pinned toolchain. Choose by changed contract,
not by available patience. These modes schedule existing checks; they do not remove
tests or make a Fast result evidence of repository-wide correctness.

## Choose your next check

This is the authoritative policy for contributors and agents. Verification must
be proportional to the risk and scope of the change; correctness and regression
assurance remain non-negotiable.

- **Direct verification** establishes the changed behavior (for example, Quick
  Open Home/End navigation tests).
- **Adjacent verification** covers nearby behavior and invariants (for example,
  Quick Open filtering, selection, opening files and unsaved-document protection).
- **System verification** covers broader integration, PTYs, reverse dependents,
  cross-package contracts, feature matrices and the full workspace. Select it by
  changed boundaries and risk, not automatically for every edit.

A low-risk isolated change can finish with direct and adjacent checks. Shared
contracts, authorization, terminal infrastructure, source lifecycle and public
interfaces require broader checks as specified below. Full remains required for
high-risk changes, release and broad regression assurance. The existing Fast,
Focused and Full command groups below are execution guidance, not three mandatory
steps for every change.

### Ordinary development workflow

1. Identify the smallest coherent implementation and its changed boundaries.
2. Select direct and adjacent tests before editing; prefer a focused red/green
   loop where appropriate. Select package and target before filtering names;
   confirm a nonzero test count.
3. Reuse existing root Cargo artifacts; batch compatible targets where useful.
4. Run system checks when the changed boundaries or escalation rules justify them.
   Documentation-only edits normally need Python checks, not a Rust build.
5. After a timeout, diagnose the specific remaining question before another broad
   run. A timeout leaves that run incomplete; it does not invalidate earlier
   passing targeted checks.
6. Report exact commands, features, results, selected scope and remaining gaps.

Do not run the whole workspace after every small edit, repeat equivalent Cargo
commands without a new reason, or require all downstream PTYs for isolated UI
changes. Do not add profiling scripts or large benchmark campaigns without a
concrete performance question, or expand a narrow change into unrelated cleanup.
These limits never excuse omitting tests necessary for the changed behavior.

| Change | Smallest useful starting point | Completion / escalation |
| --- | --- | --- |
| Markdown prose or links | Python unit and link checks below | Same checks; code examples may require consumer/doctests |
| Internal library logic | `cargo test --locked -p scope-script --lib` (example) | Direct + adjacent; affected integration/consumers if boundaries change; Full for public/security contracts |
| Isolated UI navigation | Relevant navigation tests with package + target selected | Adjacent filtering/selection/opening/protection tests; no automatic downstream PTY closure |
| One integration regression | `cargo test --locked -p scope-types --test manifest` (example) | Adjacent tests + affected consumers; type/serialization contracts require Full |
| Execution implementation | Both `execution` and `policy` targets below | All-feature execution suite + Full for policy/kernel/contracts |
| PTY synchronization helper | Python helper tests below | Real launcher/package + workbench and affected terminal/Script consumers; retain negative waits |
| Feature wiring, dependency, profile, toolchain, public API, authorization | Affected regression first | Full matrix + relevant standalone/scaffold checks |

The examples are selections, not a checklist to run in order. A small edit to a
high-risk contract is still high-risk. See the [DX3 loop audit](development-loop.md)
for measured scope-selection costs, artifact inventory and evidence limits.

## Fast — ordinary iteration

Select a package **and target first**, then optionally a test name. A name filter
alone still compiles the selected test binaries. Bare `cargo test` selects the
workbench default member, not the cheapest library or the whole workspace.

```sh
# Manifest change: compile only this integration target and its dependencies.
cargo test --locked -p scope-types --test manifest
# One named regression (confirm the summary reports a nonzero test count).
cargo test --locked -p scope-types --test manifest schema_versions_and_malformed_json_are_rejected -- --exact
# Interpreter-only change: avoid building all integration binaries.
cargo test --locked -p scope-script --lib
# Execution iteration: retain both real-Wasm targets in one Cargo invocation.
# This is not a replacement for the Focused all-features package suite below.
cargo test --locked -p scope-execution --test execution --test policy
# Markdown-only change: no Rust build needed.
python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v
python3 -B .github/scripts/check_markdown_links.py
```

For terminal synchronization-helper iteration, the smallest deterministic check is:

```sh
python3 -B crates/scope-ide/tests/test_pty_sync.py -v
# Real ordinary launcher (not the complete integration closure):
cargo test --locked -p scope-ide --features tui --test downstream_launcher
```

Confirm all nine Python tests ran. Transport-only checks do not establish real
PTY, rendering, lifecycle or authorization correctness. See the
[terminal performance audit](terminal-verification-performance.md) for measurements,
reuse constraints and remaining bottlenecks.

For Rust edits also run `cargo fmt --all -- --check`. Add `--offline` after caches
are populated to prevent network access; missing cached dependencies are not test
failures. Documentation changes involving code examples may need the relevant
doctests/consumer checks, not just link checks.

### Discover targets and reuse builds

Use `cargo metadata --locked --no-deps --format-version 1` to find each package's
`targets` (`name`, `kind`, `src_path`). For ordinary Rust integration tests,
`tests/manifest.rs` selects `--test manifest`; inline unit tests select `--lib`.
Manifest declarations may rename targets or require features. Discover test names
with the same package, target and features you will execute:

```sh
cargo test --locked -p scope-types --test manifest -- --list
cargo test --locked -p scope-types --test manifest schema_versions_and_malformed_json_are_rejected -- --exact
# Build without execution only when separating build cost is useful:
cargo test --locked -p scope-types --test manifest --no-run
```

Listing and `--no-run` still compile the selected target. They are not mandatory
preflight steps before every test. A normal repeat of the exact Cargo command
already reuses the up-to-date executable and checks source freshness. Batch
same-configuration targets with repeated `--test` arguments (execution example
above); do not run them separately and then repeat them together without a reason.

For a bounded runtime investigation only, `--no-run` prints the executable path;
that exact binary can accept `--list` or the test filter and `--exact` directly.
Do not guess its hash or use wildcard execution. Direct execution skips Cargo's
freshness check and environment setup; rebuild after edits and preserve required
working directory, features, fixtures and environment. It is not the default
contributor workflow or proof the current source passes. Ordinary launcher PTYs
already use Cargo-built binaries; replacing their launcher is not a new saving.

## Focused / Integration — complete logical change

Complete direct and adjacent verification for the logical change. For low-risk
internal changes, selected targets or a subsystem filter can suffice; complete
package tests are not automatic. Add integration tests, doctests, reverse dependents
and feature configurations when their boundaries are affected. Examples below
are system-check selections by subsystem, not a sequence for every change.

```sh
# Type/metadata contracts (include execution/IDE consumers if their contracts change).
cargo test --locked -p scope-types -p scope-meta
cargo clippy --locked -p scope-types -p scope-meta --all-targets -- -D warnings
# Execution: include both legacy execution and managed-policy rejection tests.
cargo test --locked -p scope-execution --all-features
# Interpreter plus the composed public workbench.
cargo test --locked -p scope-script -p scope-workbench
# IDE feature absence, default tools, retained TUI, or optional model integration.
cargo test --locked -p scope-ide --no-default-features
cargo test --locked -p scope-ide
cargo test --locked -p scope-ide --features tui
cargo test --locked -p scope-ide --features agent
# A launcher regression when the launcher boundary is affected.
cargo test --locked -p scope-ide --features tui --test downstream_launcher
```

For shared IDE terminal harness changes, Integration means the affected packages
and real consumers, not just socket-transport regressions:

```sh
cargo test --locked -p scope-ide --features tui -p scope-workbench
cargo test --locked -p scope-ide --features tui,agent --test configured_proposal_launcher --test source_proposal_launcher
# Add these when shared terminal/input or Script behavior changes:
cargo test --locked -p scope-terminal -p scope-script
```

Local HTTP fixtures may need permission to bind loopback sockets. Do not replace
them with paid/live model calls. Preserve negative, stale-source, execution-policy
and terminal-exit assertions. Use `--no-fail-fast` when collecting broad failure
inventory; a failed or timed-out invocation is not a pass.

Apply the same package/features to `cargo clippy --all-targets -- -D warnings`.
The IDE default is already `tools`; `tui` and `agent` include tools. An
all-features run cannot establish that feature absence works. Preserve isolated
feature checks when changing feature boundaries.

### Escalation is mandatory

- **Kernel/mathematical correctness, graph validation, type identity, capability
  propagation, serialization/versioning:** include affected consumers, positive
  and rejection tests; run Full before claiming complete confidence.
- **Authorization, confinement, execution limits/cancellation, write approval,
  stale-source/proposal acceptance, host adapters, credentials:** run complete
  affected security/policy suites plus Full. A single passing denial case is not
  sufficient. Do not replace real Wasm policy tests with mocks.
- **Public/cross-component contracts, scaffolding, feature wiring, dependency or
  toolchain/profile changes:** Full plus applicable standalone/scaffold checks.
- Purely internal low-risk changes can finish at Focused; report the chosen scope
  and what it does not establish. Storage limits can prevent escalation locally,
  but do not waive it: report the gap and require successful CI/maintainer checks.

## Full — repository-wide / high-risk / publication

This retains all current CI Rust configurations, including overlapping isolated
IDE configurations. Python 3 and a host supporting PTYs are needed for terminal
integration tests. Model tests must use deterministic fixtures, not paid/live calls.

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo test -p scope-ide --locked --features tools
cargo test -p scope-ide --locked --features tui
# Additional feature-absence coverage, not substituted by all-features.
cargo test -p scope-ide --locked --no-default-features
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p scope-ide --all-targets --features tools --locked -- -D warnings
cargo clippy -p scope-ide --all-targets --features tui --locked -- -D warnings
cargo clippy -p scope-ide --all-targets --no-default-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
cargo test --locked --manifest-path examples/downstream/Cargo.toml
cargo test --locked --manifest-path examples/harness/Cargo.toml
cargo run --locked --manifest-path examples/harness/Cargo.toml
python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v
python3 -B .github/scripts/check_markdown_links.py
```

For public API, scaffolding, or publication changes also run:

```sh
cargo run --locked -p xtask -- check-downstream --offline
```

This checks downstream/harness binaries and a freshly generated language. It
repeats some standalone coverage, but adds genuine launcher/scaffold checks.
The outer `--locked` does **not** lock its nested Cargo commands: xtask seeds the
generated workspace lockfile and permits local resolution updates; `--offline`
only prohibits fetching. Review resulting tracked lockfile changes.

Root `--workspace` does not cover independent experiment workspaces such as
`examples/ce4-composition` or `examples/h1-path-agent`; validate these when changing
their contracts and report unavailable sibling dependencies. Do not use blanket
`--include-ignored`: some ignored tests are PTY protocol child entry points.
Milestone/live-model scripts are opt-in evidence, not substitutes for deterministic
regression tests. Full local checks do not prove Linux/macOS parity; hosted CI
still matters.

## Artifact policy

- `.cargo/config.toml` shares the root `target` directory with standalone Cargo
  workspaces invoked inside this checkout. Cargo discovers config from the
  **current working directory**, not from `--manifest-path`; invoke from this
  repository. `CARGO_TARGET_DIR` or `--target-dir` overrides this location.
- Reuse that directory across iterations. Do not routinely run `cargo clean`.
  Existing example target directories are not moved or deleted automatically.
  They remain usable with explicit overrides; after a successful shared rebuild,
  an owner may choose to reclaim the obsolete directories.
- Root dev/test builds retain Scope line tables, incremental compilation, debug
  assertions and overflow checks. Only non-workspace dependency debug information
  is omitted. Dependency source-level debugging loses line information; use an explicit local
  `[profile.dev.package.<crate>]` table with `debug = "line-tables-only"` when needed.
- Standalone examples deliberately do not inherit root profiles. Shared location
  enables reuse only for matching versions, features, flags and profiles; it does
  not make standalone dependency/profile validation identical to root builds.
- Profile changes cause a one-time rebuild and may leave old fingerprints on disk.
  No release profile or production optimization settings change. Do not add
  profile/feature permutations to every Fast run.

### Bounded performance investigations

- Read the [DX3 index](development-loop.md) and existing DX reports first. Ordinary
  changes do not require profiling or a timing report.
- Before measuring, state one question, the command/features, cache state and a
  stopping rule. Default local budget: **10 minutes total, at most three comparable
  runs per variant**. Stop sooner once the engineering decision is clear. This is
  an investigation budget, not a timeout or waiver for required verification.
- Time useful checks with `/usr/bin/time -p`; distinguish Cargo preparation,
  harness execution and total wall time. Label cold, partial-cache and warm runs.
  Do not clean caches or add profile permutations just to create a benchmark.
- If the budget cannot answer the question, report it as inconclusive and propose
  a specific follow-up. Do not automatically expand into a Full benchmark campaign.
- Put optional raw output in ignored `target/dx3/` (or a task-named subdirectory).
  Keep only a concise final result, commands, limitations and decision in docs.
  Remove only your own temporary output once no longer needed; never reclaim
  pre-existing caches, experiment results or another contributor's traces implicitly.
- Use existing Cargo/Python tools. A one-off profiling driver stays temporary unless
  repeated measured need justifies maintained tooling. Never collect credentials
  or use live/paid model calls for deterministic verification.

## Heavy-path follow-up and reclamation (DX2)

See the [measured development-cost report](development-costs.md) for the current
storage inventory, owner-approved reclamation plan, warm PTY/execution timings,
incremental probe and artifact-reuse limits. No artifacts were deleted. Historical
example targets are potential recovery, not proven duplicate shared artifacts;
preserve the h1 experiment's `target/results` when reclaiming its compiler output.

For same-package, same-feature targets, repeat `--test` in one invocation rather
than launching Cargo separately. Three alternating warm measurements of execution
and policy retained all 27 tests and reduced median wall time from 1.746 to 1.591 s.
This is a small local observation, not permission to omit Focused/Full checks.
PTY runtime dominated the selected warm launcher workloads; ordinary PTY targets
already launch existing binaries, not nested Cargo. Do not weaken their wait,
lifecycle or authorization assertions based on startup-only benchmarks.

The [DX3 terminal synchronization report](terminal-test-synchronization.md) records
bounded REPL completion, repeated PTY timings, retained waits and validation gaps.

## Audit and bounded evaluation (2026-11-01)

Measured on macOS arm64, Cargo/Rust 1.97.1, offline dependencies already cached.
No `cargo clean`, network fetch, full runtime build, or deletion of existing
artifacts was used. Filesystem reported 17 GiB available on a 460 GiB volume.
Measurements are single observations, not statistically reliable benchmarks.

### Existing costs

- Nine root packages; default member `scope-workbench`. Local edges:
  workbench → IDE + Script; Script → IDE + language + terminal;
  IDE → language + meta + terminal (types in dev dependencies);
  meta → types; execution → types. Language/types/terminal/xtask do not pull the
  Cell execution runtime through local normal dependencies.
- Root target was absent. Existing standalone targets: downstream **2.7 GiB**,
  harness **2.0 GiB**, h1-path-agent **3.1 GiB** (`du -sh`, rounded).
  Downstream debug deps/build/incremental: **2.3 GiB / 118 MiB / 203 MiB**;
  harness: **1.7 GiB / 119 MiB / 100 MiB**. Cargo caches were separately
  **868 MiB registry / 2.7 MiB Git**. These are occupied bytes, not proven reclaimable
  duplicates; features and profiles can legitimately differ.
- Root dev/test already used `debug = "line-tables-only"`, incremental enabled.
  Standalone consumers use their own defaults. No repository AGENTS.md previously
  supplied a narrower verification contract.
- CONTRIBUTING listed workspace default + all-features tests and all-feature
  Clippy as the principal workflow. CI on both Linux/macOS runs four test and four
  Clippy configurations, docs and standalone consumers. These are broad compile
  workloads; CI's standalone defaults formerly put artifacts in separate roots.
  No representative agent command history was available: identifying these as
  typical dominant commands is an inference from instructions, not telemetry.

### Measured small workflow

Command: `cargo test --locked --offline -p scope-types --test manifest` (10 tests).
Baseline used the initially absent root target. Post-change used a fresh isolated
`--target-dir target/dx-compact` so old artifacts did not inflate the comparison.

| Observation | Before | Dependency debug omitted |
| --- | ---: | ---: |
| Cold baseline Cargo compilation / wall time | 4.60 s / 4.94 s | not compared |
| Warm wall time | 0.24 s | 0.36 s |
| Test execution reported by harness | 0.00 s | 0.00 s |
| deps (`du -sk`, KiB) | 49,464 | 46,364 |
| build (`du -sk`, KiB) | 6,816 | 6,376 |
| incremental (`du -sk`, KiB) | 13,852 | 14,780 |
| Sum of these three artifact directories | 68.49 MiB | 65.94 MiB |

This is **3.7% less artifact storage** for this small target, not a whole-workspace
savings estimate. Incremental storage increased. Warm latency did not improve;
0.12 s difference is too small and insufficiently sampled to attribute. Larger
Cell/Wasmtime savings and cold compilation speedups are **unmeasured**.
The modified cold build passed; its wall time is not used to claim improvement.
A subsequent complete `scope-types` + `scope-meta` test run, including doctests,
passed in **4.55 s wall time** using the compact target. That is a different
workload, not a comparison with the manifest-only baseline.
The same complete types/meta suite then passed warm in **1.24 s wall time**
(Cargo reported 0.16 s): 43 integration tests and one doctest.
Metadata confirmed the independent harness resolves its target to root `target`,
both from the repository root and from the harness directory.
No shared-consumer rebuild was attempted, so reuse savings are not quantified.
The root target grew during the audit beyond the initial baseline; only the
immediate per-directory snapshots above are used for the comparison. At the final
snapshot it occupied 1.0 GiB, including the 119 MiB compact measurement target.
The audit did not reclaim disk space: existing artifacts and measurement builds
were retained, and total root growth cannot be attributed solely to these checks.

### Compile versus runtime evidence (static unless noted)

- Types manifest tests execute below the harness's 0.01 s reporting precision;
  measured cold compilation dominates this target.
- Execution directly pulls Wasmtime 47 with default features and Cell. IDE,
  Script and workbench have many integration targets. These are inferred compile
  hotspots, not measured whole-suite timings.
- `scope-execution/tests/support/mod.rs` constructs/parses real WAT fixtures and
  creates bindings repeatedly; preparation compiles/preflights through Cell.
  `tests/policy.rs` deliberately warms real components and tests four combinations
  of loop phase and deadline/cancellation with 250 ms timers. This is meaningful
  runtime work, not dead scaffolding.
- Script/workbench PTY suites start Python and application processes repeatedly.
  Workbench milestone scripts share launcher/loopback setup but test different
  contracts; live UX scripts are not routine deterministic checks. No scaffolding
  was demonstrated obsolete, so none was deleted or disabled.

### Remaining opportunities, ordered by likely benefit

1. Measure a warm Full run on a machine with headroom, then retain its artifacts;
   profile Cell/Wasmtime compile/link costs and CI feature overlap before changing
   the matrix. Default IDE tools and workbench TUI overlap, but feature unification
   can differ. CI coverage is unchanged here.
2. Measure standalone artifact reuse after the shared target has been populated;
   only then consider aligning local profiles without losing independent-consumer
   validation. Existing directories still occupy disk until the owner reclaims them.
3. Profile execution fixture setup. Cache immutable WAT bytes if worthwhile;
   do not share mutable Cells, cancellation tokens or policy state across tests.
4. Profile PTY startup and Cargo-in-PTY launcher overhead before consolidating
   helpers. Preserve separate binary/lifecycle/authorization assertions.
5. Consider opt-in lower-parallelism builds for memory pressure or a compact
   non-incremental profile only after measuring the storage/iteration tradeoff.

### Final validation and omissions

Passed:

```sh
cargo test --locked --offline --target-dir target/dx-compact -p scope-types -p scope-meta
cargo fmt --all -- --check
git --no-pager diff --check
python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v
```

The Python suite ran five tests. The repository-wide
`python3 -B .github/scripts/check_markdown_links.py` failed with **35 existing
missing-link occurrences** (34 distinct source/target pairs). Comparing with
`git show HEAD:<path>` through the same link parser found **zero new broken links**.
These unrelated missing milestone documents were not repaired or suppressed.

Full, heavy integration/PTY suites, Clippy and Rustdoc were not run during this
bounded audit: the profile change warrants those checks before repository-wide
assurance, but available disk and avoiding an expensive baseline build constrained
local validation. No tests, feature checks or CI configurations were removed.
Dependency debug information is reduced; incremental compilation, Scope line
tables, assertions and overflow behavior are retained. No demonstrated latency
improvement or whole-workspace storage reduction is claimed.
