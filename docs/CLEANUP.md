# Repository consolidation — 2026-10-28

## Inventory and scope

The nine workspace crates retain their existing responsibilities and public APIs;
see [Development](development.md#workspace-inventory). The default `scope` binary
composes the IDE host, terminal/editor, and a pure Script runner. Language services
supply analysis, semantic operations and optional interactive evaluation. Discovery
is not authorization. Cell supplies capability-backed execution/filesystem
boundaries; native extensions remain trusted host code.

Before cleanup the working tree was clean. Inspection covered Cargo targets,
exports, source ownership, CLI dispatch, Script hosts, semantic registration,
proposal/revision/evidence tests, example dependencies, and local downstream
manifests. No tracked orphan Rust programs or disposable JSONL traces were
established: unusually named examples generally have active tests or reproduction
ownership. They were not deleted merely for looking experimental.

## Removed and consolidated

- Removed root `BUILDING_WITH_CELL.md`, `DOWNSTREAM_GUIDE.md`,
  `HARNESS_GUIDE.md`, `INTERACTIVE_SESSIONS.md`, and
  `LANGUAGE_DESIGNER_GUIDE.md`. Current material is consolidated in
  [Development](development.md), [Integration](integration.md), and
  [Agents](agents.md), with obsolete setup and repeated walkthroughs removed.
- Replaced `docs/getting-started.md` and `docs/scratch-editing.md` with the
  root [Tutorial](../TUTORIAL.md); migrated internal links, without redirect pages.
- Removed `docs/design/SC1_RESULTS.md` and `docs/INTERACTIVE_WORKBENCH.md`.
  Current contracts are in Integration; distinct historical coverage and
  limitations are retained in the [design index](design/README.md) and
  [S3 evidence](design/S3_RESULTS.md).
- Condensed S2/S3 reports, labelled UX1 historical, and clarified research-host
  ownership in the [example catalog](../examples/README.md).
- Updated the README front door, architecture and Script guides; corrected the
  security and downstream example references to Cell `v0.0.2`.
- Removed CI's redundant sibling-Cell checkout/configuration gate. Cargo already
  fetches the tagged Git dependency recorded in the lockfile. No dependency,
  capability, implementation, test, or public API was changed.

## Retained deliberately

All implementation, regression tests, example source, Cargo manifests/lockfiles,
and deterministic fixtures remain. CE1–CE4 reports and `deterministic.json`
artifacts substantiate controls, negative results and replay-versus-live limits.
Q3 capture tooling, bounded trial hosts, PTY drivers, downstream Wasm fixtures,
and shared example providers have explicit research/test ownership. Specialized
source, authorization, interoperability and execution contracts remain linked
from current guides or the design index. The longer Script tutorial is a separate
project-based course, not a competing setup guide.

## Validation actually executed

Commands used cached dependencies with `--locked --offline` where applicable.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo check --workspace --all-targets` | Passed, including example targets |
| `cargo check --workspace --all-targets --all-features` | Passed after clearing generated incremental cache following disk exhaustion |
| `cargo test --workspace --lib` | Passed |
| `cargo test -p scope-ide --all-features --lib` | Passed: 662 tests |
| `cargo test -p scope-language -p scope-types -p scope-meta -p scope-execution` | Passed |
| `cargo test -p scope-script -p scope-workbench -p scope-terminal --tests` | Passed, including three workbench PTY tests and Script PTY coverage |
| Focused retained-workspace, downstream-shell, buffered-shell and workspace-command tests | Passed |
| Harness standalone tests and runner | Passed: seven tests and approved/denied scenarios |
| Standalone downstream Wasm-binding test | Passed |
| `cargo run -p xtask -- check-downstream --offline` | Consumer/harness stages passed; generated consumer stage failed when disk filled; aggregate check incomplete |
| `cargo test --workspace` | Timed out at 200 seconds in `source_proposals_through_real_launcher`; not a complete pass |
| `cargo doc --workspace --all-features --no-deps` | Passed; existing compile warnings remain |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Failed on existing `clippy::reversed_empty_ranges` in `crates/scope-terminal/tests.rs:556`; rejection fixture left intact |
| Markdown checker unit tests and repository local-target check | Passed: five checker tests; local links resolve |
| `git diff --check` | Passed |

The normal executable's piped scratch workflow produced `42`, then `56` after a
line edit at the next revision. README's `print(2 + 3)` example, the tutorial
command-mode revision transcript, saved daily/workbench scripts, CLI help and
bounded metadata invocation were exercised. Workbench PTY tests cover keyboard
workflows; no claim is made of manually testing every terminal emulator. No live
model calls were made. Hosted CI and a complete all-features test run were not
verified. Initial disk exhaustion also interrupted an all-features compilation;
the later all-target/all-features check passed.

## Downstream compatibility

`path-scope` passed a focused library check against the current `scope-language`:

```sh
CARGO_INCREMENTAL=0 cargo check --lib --locked --offline \
  --manifest-path ../path/Cargo.toml -p path-scope --target-dir target
```

Hilb's default-feature library check was stopped as free disk space fell below
1 GB; Forth and Cross checks were then skipped. Their manifests and integration
contracts were inspected read-only, not changed. Forth's absolute paths and
path-sourced Cell logging dependency, and Cross's inherited local dependencies,
remain portability/type-identity risks. See [Integration](integration.md) for the
contract boundaries. No API changes were made, but this is not full downstream
runtime or semantic compatibility certification.

## Follow-ups intentionally deferred

1. Make PTY launcher failures reliably bounded and diagnostic, especially the
   source-proposal launcher that blocked the aggregate test run.
2. Resolve strict-Clippy compatibility of deliberate malformed-input fixtures
   without weakening their rejection coverage.
3. Make research/downstream build checks less storage-intensive and remove
   machine-specific sibling dependency paths in their owning repositories.
