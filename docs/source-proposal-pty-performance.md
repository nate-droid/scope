# DX2 — source-proposal PTY performance

## Result and scope

On macOS, the existing source-proposal launcher scenario improved from
**224.10–225.15 s to 143.92–144.28 s**, approximately **36%** (about 80 s).
These are measured warm-artifact runtime results, not build-time estimates.
The final normal Cargo invocation passed in 144.27 s test time, 145.05 s wall
including 0.68 s Cargo preparation. No production behavior, dependency, timeout,
assertion, process-isolation or authorization contract changed.

The measured family is one parent test,
`source_proposals_through_real_launcher`, with one deliberately ignored PTY child
entry point invoked by that parent. Every execution retains 13 separate lifecycle
F4 operations, seven additional output-pane F4 operations with full-redraw
inspections, and four public consumer
callbacks. Those callbacks check exact committed Unicode text and revisions
0/1 before acceptance, after acceptance, after duplicate acceptance and after
rejection. No scenarios were combined, removed or moved to mocks.

## Method and baseline

Build once, then use the same executable for before/after measurements:

```sh
cargo test --locked --offline -p scope-ide --features tui,agent --test source_proposal_launcher --no-run
python3 -B -m cProfile -o target/dx2-source-before.prof crates/scope-ide/tests/pty_source_proposal_launcher.py /absolute/path/to/source_proposal_launcher-executable
python3 -c 'import pstats; pstats.Stats("target/dx2-source-before.prof").strip_dirs().sort_stats("cumulative").print_stats(25)'
```

Use the executable printed by Cargo, not an assumed hash. The measured executable
here was `target/debug/deps/source_proposal_launcher-82a9bccb692e0650`.
The initial build reused cached dependencies and took 3.55 s; this was not a cold
build. The Python script is loaded at runtime, so changing it does not rebuild
the executable.

The first baseline was profiled without edits. A second baseline executed the
original script from Git HEAD in memory, without reverting working-tree files.
A temporary `sys.setprofile` driver recorded monotonic call/return durations for
`Workspace` initialization, transport methods, command wrappers, exit and cleanup.
This separated cumulative command cost from transport phases without adding
sleeps or changing PTY input. Runs were sequential, not concurrent with each
other or with supporting tests. No subprocess-contention experiment was run.

Local raw artifacts (untracked, under the reusable `target` directory):

- `dx2-source-before.prof`, `dx2-source-after-1.prof`: cProfile results.
- `dx2-source-before-phases.json`, `dx2-source-after-2.json`,
  `dx2-source-after-3.json`: per-call phase traces.
- `dx2_profile.py`: temporary profiling driver, accepting `before|after` and an
  output JSON path; it contains this checkout's executable path/hash.

These local artifacts are not required by the tests. The tables below retain
the measured results independently of those artifacts.

### Runtime and repeatability

| Execution | Runtime (s) | Result |
| --- | ---: | --- |
| Original, cProfile, whole script | 225.145 | Pass |
| Original, phase trace, `main` | 224.103 | Pass |
| Optimized 1, cProfile, whole script | 143.918 | Pass |
| Optimized 2, phase trace, `main` | 144.282 | Pass |
| Optimized 3, phase trace, `main` | 143.965 | Pass |
| Optimized 4, normal Cargo parent test | 144.270 | Pass |

The first three optimized profiling runs span **0.364 s**, about 0.25% of
runtime; median 143.965 s. cProfile includes approximately 0.04 s of module
imports outside `main`; phase traces measure `main`. This small measurement
boundary difference is not the source of the approximately 80 s improvement.
Four completed optimized real-PTY executions passed, not merely transport mocks.

### Per-phase breakdown

Before and after columns below use the phase-traced baseline and optimized run 2.
Rows are transport phases, not additional application scenarios.

| Phase | Count | Before (s) | After (s) |
| --- | ---: | ---: | ---: |
| PTY construction | 1 | 0.0015 | 0.0014 |
| Initial inspector output | 1 | 1.323 | 1.328 |
| F4 command-bar entry | 20 | 80.066 | 0.222 |
| F4 proposal/lifecycle/callback submissions | 13 | 52.045 | 52.037 |
| F4 `pane output` submissions | 7 | 28.025 | 28.026 |
| Output scrolling | 7 | 28.016 | 28.029 |
| F2 shell entry, including setup | 8 | 2.574 | 2.590 |
| Shell quit/full TUI redraw, including setup | 8 | 32.026 | 32.030 |
| Setup shell commands (open and semantic target) | 2 | 0.008 | 0.005 |
| Ctrl-Q collection | 1 | 0.009 | 0.003 |
| `wait_exit` | 1 | <0.001 | <0.001 |
| Process/fixture cleanup | 1 | 0.002 | 0.002 |

Other fixture/orchestration overhead is below 0.01 s at this precision. Timings
include terminal collection, not just application execution. In particular,
52 s for submissions is **not** evidence that proposal generation takes 52 s.
Generation here is deterministic local JSON, with no model/network call.

The initial cProfile run spent **224.951 of 225.145 s in `select()`**, with 71
`read` calls and 64 `send` calls. Twenty command-bar reads hit the existing
four-second collection cap. Periodic terminal output prevents the transport's
0.3 s quiet condition from ending those reads early. This is bounded collection
waiting, not a CPU, filesystem or process-startup bottleneck. Successful baseline
runs did not enter assertion-timeout failure paths.

### Per-operation breakdown

These timings include each operation's review navigation where present. The
13 wrappers total 218.429 s before and 138.608 s after. The remaining runtime is
initial setup, terminal startup/exit and cleanup.

| F4 operation, in execution order | Before (s) | After (s) |
| --- | ---: | ---: |
| Propose committed Unicode replacement; inspect creation | 24.347 | 16.360 |
| List pending proposal | 24.344 | 16.350 |
| Review diff | 24.333 | 16.354 |
| Inspect unchanged pending source | 8.005 | 4.015 |
| Explicitly accept proposal 1 | 24.324 | 16.362 |
| Inspect accepted source/revision | 8.011 | 4.018 |
| Duplicate acceptance | 8.007 | 4.016 |
| Inspect unchanged duplicate result | 8.008 | 4.011 |
| Propose rejected replacement; inspect creation | 24.339 | 16.364 |
| Explicitly reject proposal 2 | 24.349 | 16.358 |
| List zero pending proposals | 24.345 | 16.358 |
| Attempt acceptance after rejection | 8.009 | 4.026 |
| Inspect unchanged rejected result | 8.009 | 4.016 |

## Change and safety boundaries

Only `crates/scope-ide/tests/pty_source_proposal_launcher.py` changes execution.
Its 20 F4 entry calls now use the existing
`Workspace.send_until(b'\x1bOS', '>')`. The freshly emitted empty command-bar
prefix establishes command input focus for this fixed fixture. It is not a
complete-frame, command-completion or authorization assertion, and should not be
copied as a universal marker for arbitrary source text. The existing helper
accumulates fragmented output, answers cursor queries and reports missing marker,
input bytes and received output under its unchanged bounded timeout.

All submissions, pane selection, scrolling, full redraws, callback checks,
terminal-mode checks and disk-unchanged checks remain unchanged. Every operation
still opens its own F4 command line. The parent still checks child exit status,
and the child still requires exactly four callbacks. Process groups and mutable
fixtures remain independent per execution. No shared mutable state or immutable
fixture pooling was introduced: setup measured in milliseconds offers no useful
payoff for complicating isolation.

An attempted F2 `send_until(..., 'scope> ')` migration **failed** during the next
scratch-open command: the prompt was emitted before shell input setup completed,
and the expected post-submission prompt never appeared. Diagnostics showed the
command echo and partial prompt rather than completed command output. The attempt
was reverted. Existing F2 collection is retained everywhere in this scenario;
a visible prompt alone is not proven readiness here. No timeout budget was
reduced to hide this failure. No final-version timing failures occurred.

The scenario does not independently prove every stale-source, target-retargeting
or draft-authorization case; those remain in existing lower-level suites. No such
checks were moved out of PTY coverage or weakened. The supporting controller,
retained bridge and full feature-enabled IDE library suites were retained.

## Validation and limits

All commands below reused warm artifacts and ran offline. No paid/live model
calls or credentials were used.

| Exact command | Result |
| --- | --- |
| `python3 -B crates/scope-ide/tests/test_pty_sync.py -v` | 9 passed; 1.42 s wall |
| `cargo test --locked --offline -p scope-ide --features tui,agent --lib` | 742 passed; 1.06 s wall |
| `cargo test --locked --offline -p scope-ide --features tui,agent --test source_edit_controller --test retained_proposal_bridge --test downstream_launcher --test configured_proposal_launcher` | 16 passed; 2 existing child entry points ignored; 130.08 s wall |
| `cargo test --locked --offline -p scope-ide --features tui,agent --test source_proposal_launcher` | 1 parent passed; 1 existing child entry point ignored; 144.27 s test / 145.05 s wall |

Supporting integration test times: configured launcher 85.93 s, downstream
launcher 41.19 s, retained bridge 0.04 s, controller <0.01 s. The first combined
supporting invocation failed at the configured fixture's loopback bind with
`PermissionError`; the exact command passed on an approved unsandboxed retry.
This was an environment failure, not a replaced fixture or live-service fallback.

Documentation checks also ran:

- `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`:
  all five tests passed.
- `python3 -B .github/scripts/check_markdown_links.py`: failed with 35
  pre-existing missing-link occurrences in unrelated documents. A direct call to
  its `check` function confirmed zero errors in either changed document. No
  unrelated links were repaired or suppressed.
- `git --no-pager diff --check`: passed.

This is targeted assurance, **not** a complete workspace pass. The shared
`Workspace` helper, Rust/production code, feature boundaries and authorization
contracts did not change. The complete IDE/workbench package closure, isolated
feature-absence checks and Full workspace matrix were not rerun for this
scenario-local synchronization change. Existing compiler warnings about duplicate
cached `cell` packages and unused `submit_command` were not changed. Cross-platform
PTY parity and loaded-machine stress remain CI/follow-up work.

## Ranked next steps

1. **Configured-agent launcher (~86 s in this validation):** profile F4/picker
   entry and fresh request-completion markers separately from model-fixture
   response, diff inspection and acceptance. Reuse `Workspace.send_until` only
   for proven input-mode transitions. Keep deterministic HTTP fixtures, stale
   source, draft authorization, cancellation/rejection and disk checks intact.
2. **Workbench PTYs:** measure each existing executable's command-entry, output
   scrolling and full-redraw phases before sharing any changes. Promote proven
   mode-specific transitions, not generic title/prompt shortcuts. Preserve focus,
   execution-policy denials, negative observation windows and restored-terminal
   assertions. No new workbench timing was collected in this task.
3. **Remaining source-proposal collection (~140 s):** submissions, pane switches,
   scrolling and full redraws now dominate. Investigate a genuine observable
   completion boundary that also delivers the entire asserted frame; stopping at
   the inspector title would not establish that. Do not shorten read caps or
   delete the seven full-redraw inspections to obtain a faster number.
4. **Fixture/setup reuse:** lowest priority here. PTY construction and cleanup
   total milliseconds. Do not merge lifecycle tests, share mutable processes or
   redesign the test framework to recover negligible setup cost.
