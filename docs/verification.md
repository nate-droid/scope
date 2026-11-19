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
- **Full verification** establishes explicit integration/release readiness with
  the complete matrix below. Run it for the listed high-risk contract changes or
  an integration milestone/explicit request, not automatically after a narrow edit.
- **System checks** are selected broader integration, PTY, reverse-dependent and
  feature checks. A real-launcher test can be Adjacent for a harness change; it
  does not by itself require Full.

A low-risk isolated change can finish with direct and adjacent checks. Shared
contracts, authorization, terminal infrastructure, source lifecycle and public
interfaces require broader checks as specified below. Full remains required for
high-risk changes, release and broad regression assurance. The existing Fast,
Focused and Full command groups below are execution guidance, not three mandatory
steps for every change.

### Ordinary development workflow

1. Identify the smallest coherent implementation and its changed boundaries.
2. **During implementation:** run the narrowest direct compilation/regression
   target. Select package and target before filtering names; confirm a nonzero
   test count. A passing test command already compiles its target; a separate
   check/build/list/no-run is not a mandatory preflight.
3. **At feature completion:** add targeted adjacent regressions and one meaningful
   end-to-end acceptance journey for the changed user workflow (reuse applicable
   current-source evidence). Library-only or prose changes need the corresponding
   consumer or documentation checks, not an invented interactive journey.
4. **System verification:** run wider compatibility, feature-matrix and artifact
   checks only for a genuinely changed boundary under the escalation rules, or
   an explicit system/release request. Reuse root Cargo artifacts and batch
   compatible targets. Documentation-only edits normally need Python checks.
5. Bound every command. Use 120 seconds as the ordinary initial command budget;
   choose a documented longer bound for a known slow required journey. If a broad
   run is unexpectedly expensive, stop and record completed targets. After a
   timeout, require a new diagnostic hypothesis before retrying or launching an
   overlapping broad run. Earlier targeted passes remain valid within their scope.
6. Reuse passing evidence when relevant source, target, features and environment
   are unchanged. Do not rerun a successful check merely to advance tiers; run
   uncovered required coverage. Repeat only for a relevant modification, evidence
   of flakiness/environment change, or a specific bounded diagnostic question.
7. Report exact commands, features, results, selected scope and omissions with
   reasons. A known unrelated failure is a blocker, not a pass or an obligation
   to repair unrelated code.

Do not run the whole workspace after every small edit, repeat equivalent Cargo
commands without a new reason, or require all downstream PTYs for isolated UI
changes. Do not add profiling scripts or large benchmark campaigns without a
concrete performance question, or expand a narrow change into unrelated cleanup.
Do not automatically run workspace Clippy, Rustdoc, all-feature tests or multiple
interactive launcher suites after an isolated UI edit. Clippy target scope should
match the changed Rust target unless broader lint coverage is justified.
These limits never excuse omitting tests necessary for the changed behavior.

| Change | Smallest useful starting point | Completion / escalation |
| --- | --- | --- |
| Markdown prose or links | Python unit and explicit-source link checks below | Same checks; moves/deletions or changed link targets need the full link scan; code examples may require consumer/doctests |
| Internal library logic | `cargo test --locked -p scope-script --lib` (example) | Direct + adjacent; affected integration/consumers if boundaries change; Full for public/security contracts |
| Isolated UI navigation | Relevant navigation tests with package + target selected | Adjacent filtering/selection/opening/protection tests; no automatic downstream PTY closure |
| One integration regression | `cargo test --locked -p scope-types --test manifest` (example) | Adjacent tests + affected consumers; type/serialization contracts require Full |
| Execution implementation | Both `execution` and `policy` targets below | All-feature execution suite + Full for policy/kernel/contracts |
| PTY synchronization helper | Python helper tests below | Real launcher/package + workbench and affected terminal/Script consumers; retain negative waits |
| Feature wiring, dependency, profile, toolchain, public API, authorization | Affected regression first | Full matrix + relevant standalone/scaffold checks |

The examples are selections, not a checklist to run in order. A small edit to a
high-risk contract is still high-risk.

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
# Explicit sources for an ordinary prose/link edit (repository-relative paths):
python3 -B .github/scripts/check_markdown_links.py docs/verification.md
# Full scan for moves/deletions, changed link targets, or repository assurance:
python3 -B .github/scripts/check_markdown_links.py
```

Explicit-source link checks validate outgoing links only in the named files; they
are not a repository-wide pass. Include every edited Markdown source. Use the
no-argument full scan when moving/deleting files or changing link targets, since
unchanged sources may link to them. Both modes omit remote URLs and heading anchors.

Direct is the normal editing loop; the Fast examples above select its cheapest
relevant target. For IDE Rust logic, start with a named regression under
`cargo test --locked -p scope-ide --features tui --lib` rather than an unfiltered
package. For terminal synchronization-helper iteration, Direct is:

```sh
python3 -B crates/scope-ide/tests/test_pty_sync.py -v
# Adjacent: select the affected real ordinary-launcher journey (not all consumers).
cargo test --locked -p scope-ide --features tui --test downstream_launcher
```

Confirm all fourteen Python tests ran (transport and real-PTY lifecycle regressions).
Transport-only checks do not establish real
PTY, rendering, lifecycle or authorization correctness.

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

### Launcher budgets and entry points

Use existing Cargo targets; no separate runner or blanket `--include-ignored` is
needed. Ordinary iteration selects `--lib` or one `--test` target. Explicit real
launcher acceptance is `--test downstream_launcher` (both downstream discovery
and H4 keyboard journeys); these remain enabled in normal workspace tests.

Budget a warm downstream target at 120 seconds initially. H4 retains deliberate
negative-observation windows for non-dispatch, no-save and terminal-exit behavior;
positive screen assertions wait for visible state plus a complete frame rather
than terminal silence. On macOS H4 measured 93.99 seconds before six positive transitions were changed
from silence-based reads to existing complete-frame predicates, then 69.75 seconds
afterward (same fresh Cargo executable, one run each). All negative windows remain.
These are scenario timings, not compilation or whole-target timings. A cold build
can require additional budget.

The agent-feature source-proposal journey has a 300-second internal scenario
bound and measured 148 seconds warm: use a 330-second outer command budget for
`cargo test --locked -p scope-ide --features tui,agent --test source_proposal_launcher`.
It exercises separate real F4 commands and repeated full redraws for propose,
list, diff, acceptance, duplicate acceptance and rejection. Do not shorten its
negative assertions or mistake an external command cutoff for its internal timeout.
For a complete workspace run, use an explicit 420-second initial outer budget
rather than the ordinary 120-second iteration budget; compilation and host load
can still exhaust it. A passing default workspace run is not the Full matrix.

The PTY supervisor detects driver death and terminates/reaps its worker, including
when an outer command cutoff prevents Python `finally` cleanup. Lifecycle tests
cover normal/failing exits, assertion/timeout cleanup, TERM resistance and abrupt
driver death. This does not promise cleanup after an OS kills the entire process
group with SIGKILL or after host failure.

Adjacent selects only affected contracts: `downstream_launcher` for ordinary
launch/discovery/H4; `configured_proposal_launcher` for configured-agent editing;
`source_proposal_launcher` for proposal lifecycle. The latter two require
`--features tui,agent`. Do not run all three merely because they exist. A change
confined to one Python scenario using unchanged transport needs that journey and
relevant deterministic regressions, not every transport consumer.

For shared IDE terminal transport/input changes, Integration means the affected
packages and real consumers, not just socket-transport regressions:

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

Escalate for a **change to the contract**, not merely a call to an unchanged
boundary. For example, changing public local-Apply/history or stale-source
acceptance requires Full; changing Format preview wording using those existing
contracts does not. Preserve relevant stale/denial regressions in either case.
Historical incomplete Full obligations must be reported separately, not silently
inherited as the implementation checklist for every subsequent UI edit.

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
cargo fmt --manifest-path examples/managed-transfer/Cargo.toml -- --check
cargo test --locked --manifest-path examples/managed-transfer/Cargo.toml
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

- Ordinary changes do not require profiling or a timing report.
- Before measuring, state one question, the command/features, cache state and a
  stopping rule. Default local budget: **10 minutes total, at most three comparable
  runs per variant**. Stop sooner once the engineering decision is clear. This is
  an investigation budget, not a timeout or waiver for required verification.
- Before the first expensive run, map each required measurement to an observable
  event/counter in the planned probe (or mark it unavailable). Inspect the probe
  against that list before running it; for wait investigations, elapsed wait alone
  does not establish when success became observable. Reuse one run for compatible
  phase and total measurements rather than discovering missing fields after a
  before/after pair. This is a short task-local check, not a new report or runner;
  it never replaces required regression or acceptance verification.
- Time useful checks with `/usr/bin/time -p`; distinguish Cargo preparation,
  harness/scenario time, actual application execution and total wall time; do not
  attribute transport waits to application work. Label cold, partial-cache and warm
  runs. For known 100–150-second launchers, use historical evidence plus at most
  one instrumented baseline and one changed run; another needs a concrete anomaly.
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

Ordinary PTY targets launch existing binaries, not nested Cargo. Do not weaken
wait, lifecycle or authorization assertions based on startup-only benchmarks.
When reclaiming compiler output, distinguish it from experiment result data;
preserve results unless their owner approves removal.
