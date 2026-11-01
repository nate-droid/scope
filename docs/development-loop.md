# DX3: fast agent development loop

## Status: historical evidence

Ordinary development follows the authoritative [verification policy and
workflow](verification.md#ordinary-development-workflow), not this audit's
measurement procedure. The DX3 observations below are retained historical
evidence, not additional completion requirements. No new runner or permanent
profiling infrastructure is needed.

## Diagnosis

- **Scope selection dominates avoidable work.** Bare `cargo test` selects
  `scope-workbench`, not the cheapest target. A name filter without `--lib` or
  `--test` can still compile unrelated test binaries. Script and IDE each have
  many integration targets. The nine-package workspace and standalone examples
  are different verification scopes.
- **Broad verification is valuable but expensive.** Previous contributor guidance
  foregrounded workspace/default and all-feature runs. Current guidance already
  distinguishes three levels; this change adds an upfront decision table,
  discovery/reuse instructions and an explicit profiling budget. Development's
  ordinary build example now builds the default application, not the workspace.
  There is no agent-command telemetry proving how often broad commands are used.
- **Compile/link cost depends on the selected dependency graph.** Types/meta use
  Serde without the Cell runtime. Execution pulls Cell/Wasmtime; historical
  partial-cache IDE launcher and execution/policy builds took 42.93 s and 32.07 s
  wall respectively. Cranelift/Wasmtime were measured compile hotspots. A workbench
  timestamp-only incremental probe took 1.837 s, not a representative semantic
  edit bound. Linker-only time has not been isolated; no new campaign is needed
  to choose a narrow target.
- **Runtime cost is not all compilation.** Historical broad all-feature tests took
  455.17 s wall despite only 0.71 s Cargo preparation. PTY collection and explicit
  observation windows matter. Individual PTY improvements do not eliminate the
  need to select appropriate verification scope.
- **Some repeated execution is avoidable, some is assurance.** Repeating separate
  same-feature execution/policy commands and then a combined command is unnecessary
  during iteration. Batch the targets once. Full deliberately retains overlapping
  feature configurations: feature unification and absence coverage differ, so
  neither all-features nor direct-binary runs replace that matrix. Script and
  workbench PTYs test distinct public entry points, not interchangeable duplicates.
- **Reuse the existing target directory.** Root Cargo config already shares
  `target` with independent examples invoked from this checkout. Matching features,
  versions, flags and profiles are required for reuse. Standalone validation still
  matters. Do not clean caches or copy a stale executable to make iteration fast.

## Representative demonstration (2026-11-02)

Selected an **existing** regression, not a production change:
`schema_versions_and_malformed_json_are_rejected` in
`crates/scope-types/tests/manifest.rs`. macOS arm64, Rust/Cargo 1.97.1, cached
offline dependencies, unchanged profiles, repository-root working directory.
Root `target` was absent at inventory time; initial target execution populated it.
No cache was cleaned. Commands were bounded to at most 120 seconds per tool call.

```sh
# Fast: exactly one regression, not a zero-match success.
/usr/bin/time -p cargo test --locked --offline -p scope-types --test manifest schema_versions_and_malformed_json_are_rejected -- --exact
# Relevant integration: complete types package plus metadata consumer/doctest.
/usr/bin/time -p cargo test --locked --offline -p scope-types -p scope-meta
```

| Observation | Cargo preparation | Harness execution | Total wall |
| --- | ---: | ---: | ---: |
| Final warm exact regression, 1 passed / 9 filtered | 0.07 s | 0.00 s | 0.10 s |
| First types/meta closure after manifest build, 43 integration tests + 1 doctest passed | 0.81 s | Rust tests 0.00 s per binary; doctests 0.43 s | 2.62 s |
| Warm types/meta closure, same tests passed | Warm cache; not separately transcribed | Rust tests 0.00 s per binary; doctests 0.44 s | 0.61 s |

The first closure is **partial-cache**, not cold. `0.00 s` is harness reporting
precision, not literally zero runtime. Initial manifest execution and warm repeats
also passed; their timings are not used as a cold-build comparison. The focused
command ran four times total (initial build plus three warm executions); the
closure ran twice. No source edit, synthetic rebuild, new profile or profiler was
needed. Results are individual observations, not statistical estimates. Cargo
reported duplicate Cell-package source warnings, without test failures.

### Comparison with the documented broad path

The [terminal verification report](terminal-verification-performance.md) recorded:

| Historical broad check | Tests passed | Cargo preparation | Wall |
| --- | ---: | ---: | ---: |
| Workspace all-features | 1,194 | 0.71 s, warm | 455.17 s |
| Workspace defaults | 902 | 25.19 s, partial-cache | 391.44 s |

Those two invocations alone sum to **846.61 s (14.11 minutes)**; this is historical
arithmetic, not a newly measured Full run. They do not include the remaining
isolated configurations, Clippy, docs or standalone matrix. The narrow warm
regression plus warm integration closure sum to **0.71 s of command wall time**,
not end-to-end task time. Different scope/cache conditions mean this is **not an
equivalent-assurance speedup ratio**. It demonstrates why an ordinary narrow task
should not repeatedly pay the broad-path cost. No broad suite was rerun merely
to produce a comparison.

**Assurance:** the selected rejection regression and types/meta package tests,
including their integration tests and one doctest, passed locally. No manifest,
type identity or serialization behavior was changed. A real change to those
contracts would still require affected execution/IDE consumers and Full under
the escalation policy. This demonstration does not establish PTY, execution,
authorization, feature-matrix or cross-platform correctness.

## Existing evidence: reuse before measuring

| Report | Useful retained conclusion |
| --- | --- |
| [Verification audit](verification.md#audit-and-bounded-evaluation-2026-11-01) | Historical manifest cold compilation/wall 4.60/4.94 s; small-target artifact reduction 3.7%, no demonstrated warm latency gain |
| [DX2 development costs](development-costs.md) | Same-feature execution/policy batching retained 27 tests; median 1.746 → 1.591 s; partial-cache builds and conditional reclamation plan |
| [Terminal verification performance](terminal-verification-performance.md) | Downstream scenario approximately 24% faster; broad default/all-feature costs above; binary reuse already present |
| [Terminal synchronization](terminal-test-synchronization.md) | Warm launcher median 57.755 → 54.774 s; no demonstrated workbench speedup; retained negative waits |
| [Source-proposal PTY performance](source-proposal-pty-performance.md) | Scenario approximately 225 → 144 s; removed transition overhead, not authorization/lifecycle assertions |

These are different workloads and revisions. Do not add their savings or describe
them as whole-loop improvements. Raw profiles are optional local evidence, not
required fixtures for ordinary development. New investigations use the
[bounded profiling convention](verification.md#bounded-performance-investigations):
one question, ten-minute default budget, at most three runs per variant, stop
when the decision is clear. Required verification is not waived by that budget.

## Temporary tooling and artifact inventory

Read-only inventory at task start: tracked worktree clean, no non-ignored untracked
files. Root `target/` was **absent**, unlike historical reports. The demonstration
then created normal root build artifacts. No pre-existing file was removed or
overwritten. No sibling repository or live model was used.

| Artifact group | Presence at inventory | Safe recommendation |
| --- | --- | --- |
| `target/dx-compact/`, `target/flycheck0/` | Absent | Historical isolated/editor caches; no current recovery claim |
| `target/dx2_profile.py` | Absent | Historical checkout/hash-specific profiler; do not recreate or promote as a general runner |
| `target/dx2-*.log`, `target/dx2-*.prof`, source phase/repetition JSON, `target/cargo-timings/` | Absent | Historical optional measurements; concise conclusions remain in linked reports |
| `target/dx-terminal-full-unrestricted.log`, `target/dx-terminal-default.log` | Absent | Do not present these historical logs as currently available |
| `examples/downstream/target/`, `examples/harness/target/` | Present, ignored | Preserve until owner approves reclamation after successful shared-target consumer builds |
| `examples/h1-path-agent/target/` | Present, ignored; includes result JSON | Preserve evidence; never delete whole directory as build-only output |
| Python `__pycache__/`, `.DS_Store` | Present, ignored | Not DX framework code; unrelated cleanup unnecessary |
| Tracked PTY drivers, `tests/test_pty_sync.py`, milestone scripts | Present | Maintained regressions/scenario coverage, not disposable profiling tools |

The six surviving H1 result files under
`examples/h1-path-agent/target/results/` are `fake-snapshot.json`,
`fake-h2-snapshot.json`, `h3-control-summary.json`, `fake-h2-summary.json`,
`h4-summary.json`, and `fake-summary.json`. Similar names exist under its
`evidence/`; content/provenance equivalence has **not** been established. Archive
unique evidence outside ignored build output before any owner-approved removal.
No H1 rebuild or sibling dependency work is part of this task.

Historical standalone sizes (2.707 / 1.984 / 3.114 GiB) and the previous approximate
7.92 GiB recovery estimate are **not current reclaimable-byte measurements**.
No recursive compiler-artifact size scan or deletion was performed. The tracked
empty root file `,` is not profiling debris and was left alone.

Consolidation means this index plus the existing detailed reports, not rewriting
historical evidence or introducing permanent profiling machinery. Future raw
measurements belong in ignored task-owned `target/<task>/`; retain only commands,
cache state, results, limitations and the engineering decision in tracked docs.

## Completion reporting

For current reporting requirements, see the [verification policy](verification.md).
The Rust commands above were a bounded existing-target demonstration; the DX3
edits were documentation only. Historical validation performed:

- `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`:
  **5 passed**.
- `python3 -B .github/scripts/check_markdown_links.py`: **failed**, 35 existing
  missing-link occurrences (34 distinct source/target pairs). A comparison using
  the same parser against `git show HEAD:<path>` found **zero new broken pairs**.
  The unchanged missing entry-point link in `docs/development.md` remains; unrelated
  missing milestone documents were not repaired or suppressed. The checker does
  not validate heading anchors or remote URLs.
- `git --no-pager diff --check`: **passed**.
- Rust demonstration: exact commands/results above; no source behavior changed.

Full, PTY suites, Clippy, Rustdoc and standalone builds were not run: these are
prose/workflow edits with no dependency/profile/public behavior change. Their
coverage is not claimed. Historical Full results are context, not validation of
this checkout. No tests or assurance levels were weakened.
