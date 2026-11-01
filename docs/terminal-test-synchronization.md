# DX3: bounded event-driven REPL synchronization

## Scope and contract

This is a test-infrastructure change, not an application protocol or production
semantic change. All existing behavioral assertions and tests remain enabled.
The existing ignored Rust tests remain child-process entry points, not skipped
regressions. See the [verification contract](verification.md).

The PTY drivers have several different synchronization contracts:

| Driver | Existing synchronization | Decision |
| --- | --- | --- |
| IDE `pty_editable_workspace.py` (also used by launcher fixtures) | 300 ms quiet interval, 50 ms polling, four-second read cap; `expect` repeats bounded reads | Retain generic reads; specialize synchronous REPL `command` |
| IDE `pty_shell.py` (also used by workbench shell tests) | 250 ms quiet, five-second cap; bounded startup prompt loop | Retain: `send` includes editing, completion, cancellation and cooked-editor input |
| Configured-agent launcher subclass | Caps generic reads at 500 ms because TUI redraws continually | Retain cap for generic reads, not condition-based command reads |
| Workbench `pty_workspace.py` | Screen reconstruction, 150 ms quiet, 30 ms polling, five-second cap | Retain: arbitrary key events and scrolling lack a universal completion marker |
| Script continuous/scratch drivers | Bounded quiet reads, cursor-query replies | Unchanged; not consumers of the changed helper |
| Terminal completion driver | Explicit byte expectations plus bounded paste observation | Retain: paste non-submission is a negative assertion; resize delay is a separate opportunity |

Other waits have real purposes: Escape ambiguity, mode switches, delayed model
responses, cancellation, observing that a held response stays pending, process
exit, terminal-attribute restoration, and process-group cleanup. A period spent
in `select.select` does not by itself prove wasted time. Generic quiet windows
are defensive settling heuristics, not proofs of completion; continuous TUI
redraw can exhaust the entire read cap.

## Minimal improvement

`Workspace.command` now writes directly and reads until the **submitted command
echo including its newline, followed by the next primary prompt**. Merely seeing
`scope> ` is unsafe: input editing repeatedly redraws that prompt before command
dispatch. Production `shell_input.rs` finishes and flushes the submitted echo
before returning the input; synchronous CLI dispatch then precedes the next
prompt. No production signaling was added.

The existing reader accepts an optional completion predicate. Only that path
ignores quiet intervals; it retains cursor-query replies, raw transcript capture,
EOF/macOS EIO handling and the scenario deadline. Its ten-second command bound
preserves the former maximum four-second send/read plus six-second expect budget.
Failure reports the command and received output. Generic reads are unchanged
apart from preventing a final select from overshooting its deadline.

Recognition normalizes accumulated bytes, rather than separately decoding each
read. This allows CSI, UTF-8, prompts and cursor queries to straddle fragments.
Assertions still receive the whole final OS read, not a truncated prefix.

This specialized helper accepts printable single-line ASCII fixture commands
returning to the same REPL, not editor entry, quit, arbitrary Unicode input or
asynchronous completion. The fixtures' command outputs do not forge a prompt;
this is not a universally unambiguous protocol for arbitrary output containing
`scope> `. Generic `send` remains available for other input. TUI frames still
require their existing synchronization; CSI-stripped output is not a screen.

## Measurements

Local macOS arm64, Python 3.15, existing root target and cached offline Cargo
dependencies; no clean, network downloads or unrelated broad rebuild. Commands
were alternated three times before editing, then three times after editing:

```sh
cargo test --locked --offline -p scope-ide --features tui --test downstream_launcher
cargo test --locked --offline -p scope-workbench --test terminal_pty
```

Wall seconds include Cargo and Python process startup:

| Workload | Before trials | Before median | After trials | After median |
| --- | --- | ---: | --- | ---: |
| IDE launcher target | 58.092, 57.755, 57.636 | 57.755 | 58.206, 54.761, 54.787 | 54.787 |
| Workbench PTY target (all three tests) | 53.951, 53.900, 53.771 | 53.900 | 55.778, 54.044, 53.976 | 54.044 |

The first post-edit trial includes recompilation of the changed Rust test target;
these are **not three entirely warm after trials**. The two subsequent warm
launcher trials have median **54.774 s**, range **54.761–54.787 s**, versus warm
baseline median **57.755 s**, range **57.636–58.092 s**: approximately **3.0 s / 5.2%**
less wall time. Five new synchronization regressions also run in the after target
(in parallel with the real launcher test). Workbench uses other helpers and shows
no demonstrated improvement; its measured baseline here differs from DX2's
72.1-second observation. Do not combine those numbers into a speedup claim.

A direct warm launcher regression passed in 54.525 s wall (54.43 s Rust harness).
An additional Cargo run overlapped another Cargo invocation, took 58.607 s,
and is excluded from the warm comparison because of lock/build contention.

For attribution, cProfile ran the same launcher Python scenario against the same
existing Rust test executable, once with the HEAD helper loaded in memory and
once with the edited helper (no source swapping on disk). The baseline was also
repeated with concise profiling output:

| Profiled observation | Baseline repeat | Edited helper |
| --- | ---: | ---: |
| Total Python scenario | 56.998 s | 54.035 s |
| `select.select` | 56.976 s | 54.011 s |
| Eight `Workspace.command` calls, cumulative | 2.607 s | 0.005 s |
| Workspace constructor (not application readiness) | 0.0015 s | 0.001 s |
| Workspace cleanup | 0.00046 s | below 0.001 s |

Profiles are single observations, not medians. Application startup readiness is
included in reads, not the constructor figure. Setup/cleanup did not dominate
this scenario. Most remaining time is still in generic TUI reads; the patch does
not claim all that time is removable. Workbench total remains dominated by its
separate screen and shell helpers.

## Reliability and validation

Added five deterministic socket-transport regressions, automatically invoked by
the launcher Rust integration target:

- Early typing prompt and submitted echo cannot complete the command; output is
  withheld for 400 ms, longer than the former quiet interval.
- Fragmented CSI, cursor query/reply, UTF-8 and final prompt preserve the output.
- A synchronous command with no result text completes at the new prompt.
- Missing prompt and EOF produce diagnostics rather than success or an infinite wait.
- Unsupported input is rejected before any bytes are written.

Ten consecutive repeat trials of all five regressions passed (50 checks).
These supplement, not replace, actual PTY tests. Passing checks:

```sh
python3 -B crates/scope-ide/tests/test_pty_sync.py -v
cargo test --locked --offline -p scope-ide --features tui --test downstream_launcher
cargo test --locked --offline -p scope-workbench --test terminal_pty
cargo test --locked --offline -p scope-ide --features tui -p scope-workbench
cargo test --locked --offline -p scope-ide --features tui,agent --test configured_proposal_launcher
cargo test --locked --offline -p scope-ide --features tui,agent --test source_proposal_launcher
```

The complete affected-package run includes integration tests and doctests;
workbench retains all three PTY tests. Configured-agent integration required an
unsandboxed run solely for its deterministic localhost HTTP fixture: sandboxed
binding failed with `PermissionError`. It then passed in 86.74 s. A combined
configured/source launcher command exceeded its 240-second tool bound after the
configured test passed; source-proposal was rerun separately with a 360-second
bound and passed in 223.85 s. This retains exact-source, authorization/rejection,
duplicate-acceptance, disk-unchanged and terminal-exit evidence.

The standalone editable-workspace script fails its initial `Scratcheditor` title
assertion against the current example, which emits `Editor`; running the HEAD
helper in memory reproduced the same failure before any command call. This
unrelated regression was not repaired or weakened. Its restored-termios roundtrip
assertion therefore was not reached; launcher and workbench exit coverage passed.

`cargo fmt --all -- --check` reports unrelated existing formatting differences
in production files; no production formatting changes are part of this patch.
The changed Rust test passes standalone `rustfmt --edition 2024 --check`;
`git diff --check` and all five Python documentation-checker unit tests pass.
The repository Markdown link check still fails on missing unrelated milestone
files; the new report links only to the existing verification document.
No Full workspace, Clippy, other feature permutations or Linux execution was
attempted: this is a focused internal test-helper change, not repository-wide
assurance. Concurrent production edits during the experiment also limit strict
whole-run comparability; in-memory baseline/current helper profiling against one
fixed executable provides the narrower causal evidence.

## Remaining opportunities

1. Investigate frame-completion recognition for continuously redrawing TUIs.
   Require a reliable full-frame boundary, fresh state, supported terminal sizes,
   and delayed/fragmented rendering tests before changing generic reads.
2. Split shell command completion from interactive key-edit settling in the
   separate `Shell` helper, accounting for cooked-editor prompts and submitted
   echo. Do not assume every Enter submits a REPL command.
3. Repair the standalone example/title mismatch separately, then rerun its full
   terminal-attribute restoration scenario.
4. Run these checks on Linux CI and under real CPU contention. The deterministic
   delayed-output test is not a whole-host slow-machine benchmark.
