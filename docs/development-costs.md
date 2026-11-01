# DX2: artifact reclamation and heavy-path measurements

Local observations on 2026-11-01, macOS arm64, Rust/Cargo 1.97.1. These are
bounded development-loop measurements, not release benchmarks. Follow the
[verification contract](verification.md); this audit does not replace Full.

For the current workflow, consolidated historical measurements and a refreshed
artifact-presence inventory, see the [DX3 development-loop audit](development-loop.md).
The sizes and raw-output locations below are historical snapshots, not a claim
that those files still exist or that those bytes are reclaimable today.

## Method and limits

- Started with a clean tracked worktree, cached offline dependencies, 48 GiB
  filesystem availability and 36 GiB physical RAM. `memory_pressure` reported
  23% system-wide memory free with substantial compressor/wired usage.
- Used the existing shared root target, `--locked --offline -j 2`, no network,
  no clean, no artifact deletion, no live model calls and no new profiles.
- Measured warm execution first where binaries existed. The selected heavy root
  test binaries did not exist: the root contained check metadata and some build
  artifacts, but no Wasmtime, Cell or IDE rlibs. Bounded target-specific builds
  populated them; these are **partial-cache builds, not warm or cold baselines**.
  No full rebuild was performed merely for a benchmark.
- Wall times use Python `time.monotonic()` around subprocesses or
  `/usr/bin/time -p` / `-l`. Cargo unit durations come from ordinary `--timings`
  HTML. Parallel unit durations overlap: their sums are not wall times or CPU time.
  Test-harness duration excludes Cargo and some executable startup overhead.
- Initial builds used a 120-second command bound. Runtime selections were bounded
  individually at 110 seconds. Results are single samples unless repetitions are
  stated. Host load and filesystem cache effects are not controlled.
- One incremental probe changed only the mtime of workbench `src/main.rs`, then
  restored its original timestamps in `finally`. Source bytes were unchanged.
  This measures a no-content-change incremental compile/link, not a semantic edit.

## Storage inventory

`find . -type d -name target -prune` found exactly four target roots. Inventory
uses allocated KiB from `du -sk`, converted to GiB (1,048,576 KiB). Do not add
nested rows to parent totals. Hard links make independently measured subdirectory
sizes non-additive; logical file sizes can also exceed allocated size.

| Directory | Initial allocated KiB | GiB | Classification |
| --- | ---: | ---: | --- |
| `target` | 1,079,224 | 1.029 | Active shared builds, plus old audit artifacts |
| `examples/downstream/target` | 2,838,208 | 2.707 | Historical independent consumer artifacts |
| `examples/harness/target` | 2,080,556 | 1.984 | Historical independent consumer artifacts |
| `examples/h1-path-agent/target` | 3,265,232 | 3.114 | Historical experiment builds **and result data** |

Initial root breakdown: `debug` 957,520 KiB; within it, `deps` 480,356,
`build` 120,880, `incremental` 349,156. `target/dx-compact` was 121,376 KiB,
`target/flycheck0` 320 KiB, and `target/tmp` empty. No release directory was found
in any of the four targets. No target directory existed for `ce4-composition`.
Hidden fingerprints and root binaries are not all represented by those subrows.

Historical debug breakdowns (each subdirectory measured separately):

| Example | deps KiB | build KiB | incremental KiB |
| --- | ---: | ---: | ---: |
| downstream | 2,384,548 | 120,956 | 207,976 |
| harness | 1,793,392 | 122,228 | 102,876 |
| h1-path-agent | 2,656,532 | 174,508 | 348,224 |

After measurements, root `target` occupied **2,751,608 KiB (2.624 GiB)**, an
increase of **1.595 GiB**. `debug` was 2,628,852 KiB, `deps` 1,858,388,
`build` 123,104, `incremental` 520,216. Timing HTML occupied 872 KiB; raw DX2
logs/JSON/cProfile data are also retained under root `target`. Filesystem
availability fell to 44 GiB; concurrent system activity/APFS accounting means that
4 GiB change must not be attributed entirely to these builds. No disk was reclaimed.

### Ranked owner-approved reclamation plan

Stop Cargo, rust-analyzer checks and tests before removing any build directory.
These are removal candidates, **not instructions to run automatically**. No source,
lockfile, fixture, Cargo registry or Git cache needs deletion.

1. **Old isolated audit target:** `target/dx-compact`, **115.75 MiB**. The previous
   audit explicitly selected it; ordinary commands use `target/debug`. Removing
   it loses only that isolated cache; repeating its `--target-dir` commands rebuilds
   it. `target/flycheck0` adds 0.31 MiB of temporary check output and can be recreated
   by the editor; inspect/stop active checks first. DX2 logs and timing reports are
   tiny and worth retaining until the audit is reviewed.
2. **Historical downstream and harness targets:** **4.69 GiB combined**. All five
   manifests (root, downstream, harness, h1, ce4) now report root `target` using
   `cargo metadata --locked --offline --no-deps --format-version 1` from this
   checkout. No nested Cargo config overrides were found. After validating each
   consumer with the shared target, the owner can remove
   `examples/downstream/target` and `examples/harness/target`. Their standalone
   dependencies/profiles may still need compilation into the shared cache; this
   audit did **not** do those rebuilds or prove equivalent cache contents.
3. **Historical h1 builds:** `examples/h1-path-agent/target/debug`, about
   **3.11 GiB**. Preserve `target/results` there: its six JSON snapshots/summaries
   occupy 156 KiB and are experiment evidence, not disposable compiler output.
   Do not remove the whole h1 target indiscriminately. Rebuilding needs its sibling
   `path` checkout; ce4 additionally needs sibling `scope-forth`. Metadata with
   `--no-deps` does not establish those dependencies build. Retain old executables
   if needed until a shared rebuild succeeds.
4. **Active root incremental cache:** optionally **508.02 MiB** at the final
   snapshot. Removing only `target/debug/incremental` retains ordinary compiled
   dependencies, but sacrifices incremental compiler state; future local crate
   edits may be slower. This is lower priority than historical targets, not a
   routine cleanup recommendation. Removing active deps/build instead would
   recreate precisely the heavy work measured below.

**Conditional historical/audit recovery is approximately 7.92 GiB**, excluding h1
results and without double-counting example incremental caches. Only about
116 MiB is the clearly isolated previous audit/check cache; the larger amount
is removable compiler output but not yet demonstrated superseded by successful
shared consumer builds. Active root incremental adds about 0.50 GiB if the owner
accepts that latency tradeoff. Physical free-space recovery can differ under APFS,
snapshots and shared extents. Age or duplicate filenames alone do not prove a
fingerprint obsolete; do not surgically delete hashed dependency files.

## Heavy build and warm-runtime results

All Cargo commands below ran from the repository root. Common prefix:
`cargo test --locked --offline -j 2`.

| Selection | Partial-cache no-run wall / Cargo | Warm wall / Cargo / harness | Tests |
| --- | --- | --- | ---: |
| `-p scope-ide --features tui --test downstream_launcher` | 42.93 / 42.89 s | 57.521 / 0.43 / 57.01 s | 1 exact parent |
| `-p scope-workbench --test terminal_pty` | 3.04 / 2.98 s | 72.094 / 0.41 / 71.31 s | 3 |
| `-p scope-execution --test execution` | Combined build below | 1.818 / 0.35 / 0.17 s | 14 |
| `-p scope-execution --test policy` | Combined build below | 2.628 / 0.12 / 1.28 s | 13 |

No-run selections appended `--no-run --timings`. Execution and policy were built
in one invocation (32.07 s wall / 32.03 s Cargo). Runtime commands appended
`-- --test-threads=1`, except IDE, which selected
`downstream_actions_through_real_launcher -- --exact --nocapture`.
Workbench also used `--nocapture`. All selections passed, with nonzero counts.
The first IDE invocation also passed; the table uses the later logged warm sample.

### Compilation versus test-binary generation

| Build | Dependency unit seconds (overlapping) | Local unit observations |
| --- | ---: | --- |
| IDE launcher | 75.49 | IDE library 4.11 s; other Scope libraries 2.00 s total; IDE binary 0.98 s; launcher test binary 0.98 s |
| Workbench PTY after IDE | 0 | Script library 1.58 s; workbench binary 0.96 s; test binary 0.51 s |
| Execution/policy | 53.34 | types library 0.75 s; execution library 1.06 s; execution/policy test binaries 1.16 / 1.12 s |

IDE dependency units included Cranelift codegen **10.74 s**, Wasmtime **9.15 s**,
Wasmtime environ **4.83 s**, wasmparser **3.96 s**. Execution rebuilt Cranelift
codegen **10.69 s**, Wasmtime **8.43 s**, environ **4.85 s**, wasmparser **3.82 s**.
Those are observed compile hotspots, not predicted savings from changing features.
`time -l` reported maximum resident set size 1,203,683,328 bytes for the IDE
build and 499,744,768 for workbench; these are not summed concurrent process memory.

Cargo timing sections split library frontend/codegen: IDE 1.87 / 2.24 s;
execution 0.28 / 0.78 s. Binary-unit durations above include compile, codegen and
linking; **linker-only time is not isolated** by these stable Cargo reports.
The timestamp-only incremental workbench probe took **1.837 s wall**, Cargo
1.79 s, with only the workbench binary rebuilt (1.47 s unit), no dependencies or
PTY test target rebuilt. Do not treat it as a representative semantic-edit bound.

### PTY runtime, process startup and fixtures

Ordinary IDE launcher tests use Python plus the current integration executable's
ignored `launcher_child`. Workbench `terminal_pty` passes `CARGO_BIN_EXE_scope`
to Python. Neither runs nested Cargo. The ignored child is a protocol entry point,
not missing coverage; do not use blanket `--include-ignored`.

A direct, profiled workbench scenario passed:

```sh
python3 -B -m cProfile -o target/dx2-pty-workbench.prof crates/scope-workbench/tests/pty_workbench.py --binary target/debug/scope -v
```

One test ran in 7.675 s, Python total 7.725 s. `select.select` accounted for
**7.666 s in 450 calls**; 26 shared `pty_shell.py::read` calls took 7.670 s.
Fixture `setUp` took 1.275 s including synchronization/read waits, not just spawn.
This identifies waiting/synchronization as a concrete optimization candidate,
but does not prove every wait removable or distinguish child computation during
those waits. A 0.3-second quiet interval is part of the transport behavior.
Changing it needs repeatable slow-host and Linux/macOS lifecycle coverage.

Direct `scope --help` startup took 0.006 / 0.004 / 0.004 s; the integration
executable's `--list` rounded to 0.00 s under `time -p`. These are minimal startup
probes, not interactive-render readiness benchmarks. The analogous warmed
`cargo run --locked --offline -j 2 -p scope-workbench --bin scope -- --help`
took **1.266 / 1.067 / 1.070 s**. The separate UX3/UX3b/UX5 milestone scripts
really do use `cargo run` inside a PTY and pay repeated Cargo overhead; that
measurement is not a timing of those full scenarios. Keep their root-launcher
assertions; use the existing direct-binary suites when measuring runtime alone.
No live/paid UX4 script was invoked.

Real execution fixture preparation (WAT parsing, Cell compilation/preflight and
execution combined) fit in the 0.17-second execution harness sample. No isolated
WAT-only timing was taken. Policy's four deliberate 250 ms deadline/cancellation
windows explain about a second of its runtime. Caching mutable Cells or removing
policy cases would be inappropriate. Immutable WAT caching is not justified by
these aggregate costs relative to PTY waits and dependency compilation.

## Improvements and decisions

### Implemented: retain targets, batch same-configuration selections

The verification guide now gives a target-first Fast command for both execution
integration targets together. It retains every test while avoiding a second
Cargo startup/freshness walk. This is a workflow/documentation change only.

Alternating three warm repetitions, serial test execution:

| Invocation shape | Wall seconds | Median |
| --- | --- | ---: |
| Two Cargo invocations, one per target | 2.070, 1.746, 1.744 | 1.746 s |
| One Cargo invocation, both `--test` flags | 1.615, 1.567, 1.591 | 1.591 s |

Observed median improvement: **0.155 s (8.9%)**, preserving all **27 tests per
repetition**. This is small and load-sensitive, not a general Cargo speedup claim.
No new orchestration, dependencies, feature/profile changes or test modifications
were needed. The Focused all-features execution suite remains mandatory for
completing an execution change.

### Evaluated, not changed

- **Cross-workspace reuse:** all independent manifests resolve to the shared
  target. Workbench's build after IDE compiled only three local units, directly
  demonstrating dependency reuse inside the root workspace. Standalone
  consumer reuse remains unquantified: independent default profiles differ from
  the root dependency-debug override, and versions/features may differ too.
  Do not align those profiles merely to improve a benchmark.
- **Feature/graph fragmentation:** IDE then execution rebuilt substantial runtime
  dependencies. Root Wasmtime fingerprints include identical printed Wasmtime
  feature lists with different dependency fingerprints (and a separate check
  profile); Cell likewise has multiple dependency fingerprints with empty direct
  feature lists. Direct features alone do not determine artifact compatibility.
  Shared target location cannot prevent different transitive graphs producing
  distinct units. Do not infer these are obsolete or disable default Wasmtime
  functionality without dependency/consumer validation and Full.
- **Redundant targets:** selected integration builds also generated application
  binaries needed by integration tests. Their costs are explicitly measured;
  no target was shown safe to remove. Select package/target before name filters.
- **Compiler caching:** may help repeated matching non-incremental dependency
  builds across clean machines; it would not remove PTY waits, Cargo checks,
  linker costs or the policy timer windows. Differing fingerprints still miss;
  incremental local builds are not a demonstrated caching win here. No cache
  installed or performance claim made without an A/B measurement.
- **Scheduling:** run narrow targets during iteration, then existing Focused and
  Full at their documented boundaries. Keep isolated feature absence, all CI
  configurations, authorization, rejection, stale-source, real-Wasm and PTY
  coverage. No configuration was moved out of Full or scheduled less frequently
  than that contract requires.

## Verification and remaining assurance gaps

Executed commands are recorded above, with raw logs, JSON repetitions, four
Cargo timing reports and the cProfile file retained in ignored root `target`.
The raw IDE/workbench warm logs are `dx2-ide-warm.log` and
`dx2-workbench-pty.log`; execution logs are `dx2-execution-warm.log`,
`dx2-policy-warm.log` and `dx2-execution-combined.log`. Build reports and
`dx2-incremental.log` distinguish compilation from runtime. Metadata succeeded
for all five manifests with `--no-deps`; this does not validate sibling sources.

Documentation validation:

```sh
python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v
python3 -B .github/scripts/check_markdown_links.py
git --no-pager diff --check
```

The Python suite passed all five tests; diff whitespace checks passed. The global
link checker failed with 35 missing-link occurrences. Comparing every working-tree
Markdown source with `git show HEAD:<path>` through the same parser confirmed
35 baseline occurrences and **zero new failures**. Both changed/new documents
have no missing local link targets. Unrelated missing documents were not repaired.
No Rust source, config, dependency, public contract or coverage changed,
so this documentation-only delivery does not introduce a new Full requirement.
The prior profile-change Full assurance gap remains: workspace/all-features,
feature absence, Clippy, Rustdoc, standalone shared consumer rebuilds and hosted
Linux/macOS parity were **not** established by these selected measurements.
The IDE build emitted the existing unused `submit_command` warning. Pure linker
profiling, semantic-edit distributions, complete PTY wait attribution and a
cold-build/cache A/B remain unmeasured; none is necessary to reclaim old compiler
output with owner approval or use the demonstrated low-risk command grouping.
