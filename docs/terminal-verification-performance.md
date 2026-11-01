# Terminal verification performance — 2026-11-02

## Result and limits

A narrow harness change removes about 13 seconds (24%) from the real downstream
launcher scenario without removing assertions, processes, or isolation. This is
not a whole-suite speedup claim. The [verification contract](verification.md)
remains authoritative; Fast is not Full assurance.

Environment: macOS arm64, cached offline dependencies, existing root `target`.
No clean, dependency changes, sibling edits, or artifact deletion. No genuinely
cold build was measured: preserving existing artifacts took priority. Cargo's
reported preparation time combines freshness checking, compilation and linking;
linker-only and test-binary startup costs were not separately instrumented.

## Measurements

Commands (from repository root):

```sh
cargo test --locked --offline -p scope-ide --features tui --test downstream_launcher
cargo test --locked --offline -p scope-workbench --test terminal_pty
cargo test --workspace --all-features --locked --offline --no-fail-fast
cargo test --workspace --locked --offline --no-fail-fast
```

| Observation | Before | After |
| --- | --- | --- |
| Launcher Cargo preparation, first observation | 2.37 s (partial-cache rebuild) | warm subsequent observations about 0.5 s |
| Launcher Rust harness execution | 54.14 s | 41.45, 41.15, 41.16 s |
| Launcher command wall | 57.32 s (includes rebuild) | 42.12, 41.70, 41.73 s (warm) |
| Fixed-executable Python scenario, cProfile | 54.401 s | 41.414 s |
| Workbench target, 3 real PTY tests | 53.31 s execution; 57.87 s wall; 4.21 s preparation | all-feature run: 53.49 s execution (different feature build) |

The pre-edit Cargo observation is **not** a warm build comparison. To isolate
runtime, the pre-edit driver was also compiled in memory with the two original
`expect(..., send(...))` calls restored and executed against the same existing
`downstream_launcher-15910a9d17f4c218` executable as the edited driver. No source
files were swapped. These two single profiles are attribution evidence, not
statistically robust medians or a compilation improvement.

Baseline profile: 54.295 s in select, three `resume` calls 12.009 s, three shell
transitions 0.960 s, three picker calls 12.009 s. Edited: 41.367 s in select,
six `send_until` calls 0.028 s, picker calls still 12.005 s. Thus most remaining
runtime is polling/settling, not Python CPU or process creation. Earlier
[DX3 profiles](terminal-test-synchronization.md) measured construction about
1 ms and cleanup below 1 ms; those are historical, not new startup measurements.
Readiness remains included in reads, not fork duration.

## Implementation and safety

`Workspace.send_until(keys, needle, timeout=10)` reuses the existing reader,
scenario deadline, fragmented CSI/UTF-8 normalization, cursor-query replies,
transcript and process-group cleanup. It stops on fresh received transition text,
returns the complete final OS read and reports marker/input/output on failure.
Empty markers are rejected before writing.

Only the ordinary downstream launcher's REPL entry and TUI return use it.
These call sites need the next input mode, not trailing frame content. It is not
an arbitrary-command completion protocol: stale/unconsumed matching output or
output forging a marker can invalidate that assumption. Use `command` for its
restricted synchronous REPL contract. Generic `send/read` remains necessary for
frame-content assertions and negative observation windows. Do not use title
recognition to claim a complete rendered frame or that a forbidden action never
occurred. No configured/source-proposal synchronization was migrated.

Four added transport tests cover delayed/fragmented markers, cursor replies,
EOF diagnostics, both deadline bounds and empty-marker rejection. Nine total
transport regressions passed once and ten additional repetitions (99 checks).
These complement real PTYs, not replace them. Three repeated real launcher runs
passed: two Rust tests per run, including all nine Python synchronization tests.
The existing ignored launcher child remains a protocol entry point, not a newly
ignored regression. An initial validation attempt caught string rather than byte
inputs in the new tests; corrected tests then passed. No application assertion
was weakened. Process isolation, Cargo scheduling and concurrency are unchanged.

## Broad correctness evidence

The first sandboxed all-feature attempt failed at loopback HTTP binding, not at
a terminal assertion (40.06 s wall). Both workspace configurations were then run
outside the sandbox, allowing deterministic local HTTP fixtures:

| Suite | Cargo preparation | Wall | Passed | Existing ignored child entries |
| --- | ---: | ---: | ---: | ---: |
| workspace all features | 0.71 s (warm) | 455.17 s | 1,194 | 3 |
| workspace default | 25.19 s (partial-cache configuration rebuild) | 391.44 s | 902 | 2 |

Counts sum Rust harness summaries, including doctests; Python assertions inside
Rust integration tests are not counted a second time. All 69 harness summaries
in each run succeeded. Logs are local artifacts at
`target/dx-terminal-full-unrestricted.log` and `target/dx-terminal-default.log`.
No pre-change full run was measured, so no Full before/after claim is possible.

This is not completion of the entire Full verification matrix: isolated tools,
tui and feature-absence runs, Clippy, Rustdoc and standalone consumers were not
all run in this milestone. Linux and contended-host evidence remain outstanding.
No live models, credentials or paid calls were used. The full all-feature test
execution alone exceeds the aspirational 2–3 minutes.

Additional checks: `cargo fmt --all -- --check`, `git diff --check` and
`python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`
passed (five documentation-checker tests). The repository-wide
`python3 -B .github/scripts/check_markdown_links.py` still reports 35 missing
link occurrences in unrelated existing documents; neither changed document is
reported. Those missing milestone files were not repaired or suppressed.

## Remaining bottlenecks, ranked

Follow-up: the [DX2 source-proposal measurements](source-proposal-pty-performance.md)
reduce the first workload below from about 224 s to 144 s using scenario-local
F4 entry synchronization. The figures below retain this audit's original baseline.

1. **Source-proposal launcher: 223.83 s.** Highest-impact follow-up: profile its
   repeated F4/pane/redraw transitions and add fresh, bounded mode-specific waits.
   Preserve exact revisions, stale-source rejection, duplicate acceptance,
   unchanged disk and exit assertions. Do not infer full frames from title text.
2. **Configured-agent launcher: 86.41 s.** Separate model-fixture delay, mode
   switching and deliberate pending/rejection windows before optimizing. Loopback
   permissions are required; no external endpoint is needed.
3. **Workbench PTY binary: 53.49 s; ordinary launcher: 41.15 s.** Real tests
   already run concurrently within each binary. Three ordinary picker reads still
   cost 12 seconds; safe optimization needs a complete/fresh frame boundary,
   not just `Actions:` followed by possibly incomplete or negative content.
4. **Sequential integration binaries / overlapping feature builds.** Cargo runs
   binaries sequentially even though each binary uses parallel libtest tests.
   Default and all-feature coverage overlap but establish different contracts;
   no scenarios were proved redundant and none were removed. nextest is not
   installed. Its cross-binary scheduling may help, but measure resource pressure,
   nested ignored child entry points and PTY isolation first; no dependency or
   framework was added for theoretical gains.
5. **Build/link and startup attribution.** Measure a genuinely cold build on a
   separate adequately provisioned machine, plus incremental linking and binary
   startup, before changing profiles or fixture reuse. Current evidence supports
   wait optimization, not production architecture or shared mutable fixtures.

## Ownership and downstream reuse

Scope should own shared input semantics (editing, history, paste, cancellation,
EOF), terminal restoration/cleanup, and workspace synchronization regression
coverage. Path, Hilb and Lisp should retain independent launcher/registration,
language routing, focus/key bindings, host authorization and source/revision
integration tests. A passing Scope input test cannot prove a downstream host
wired its editor, command or write policy correctly.

Downstream maintainers can adopt the existing stdlib Python Workspace transport
and bounded transition pattern, with explicit local markers and their own real
PTY tests. This is repository test infrastructure, not a new stable public Rust
API or packaged terminal framework. Keep fresh processes/workspaces where history,
terminal state or authorization isolation matters. No sibling repository changed.
