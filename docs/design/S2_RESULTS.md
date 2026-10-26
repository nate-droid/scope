# S2 — retained workspace source evidence

Current API usage is in the [integration guide](../integration.md#retained-semantic-source-across-workspace-handoffs-s2).
This summary preserves the lifecycle evidence from the original implementation
baseline `17b31ade395e4b3a7d32041935ae259a18f21865`, not a new validation run.

## Contract and security boundary

One caller-owned `ScratchBuffers` supplies both the borrowed shell and workspace
through `run_borrowed_with_sources`, `run_terminal_borrowed_with_sources` and
`WorkspaceTui::run_with_sources`. `render_with_sources` explicitly supplies the
owner to render contexts. No second source registry, App-document alias or disk
fallback is introduced.

`bound_source()` distinguishes `Unbound`, `Unavailable { scratch_id }` and an
available owned `SourceSnapshot`. `TuiContext::bound_semantic_source()` returning
`None` instead means no owner was supplied. Identity, revision, language and exact
text are captured under one immutable borrow; historical snapshots cannot mutate
or silently refresh their owner.

The owner retains committed edits, dirty/destination state, selection, independent
semantic binding and closed-target tombstones across shell quit, EOF and errors.
Edits committed before an error remain committed; this is not rollback. Dropping
the owner releases live sources but not already owned snapshots. There is no
restart persistence. Pending proposals, inspection state and submission capture
remain invocation-local. Legacy entry points still use disposable scratch owners.

Observation installs no runtime, invokes no evaluator, renews no authorization,
writes no file and grants no mutable scratch reference. Native callbacks remain
trusted code, not newly sandboxed. S2 itself adds observation, not editing;
[S3](S3_RESULTS.md) records the subsequent explicit editing capability.

## Reproduction and retained evidence

```sh
cargo run -p scope-ide --features tui --example retained_workspace --offline
cargo test -p scope-ide --features tui --test retained_workspace --offline
cargo test -p scope-ide --features tui disposable_source_tests --lib --offline
```

The Cargo example requires `tui`; offline runs require cached dependencies.
`crates/scope-ide/tests/retained_workspace.rs` drives the real buffered shell and
Ratatui TestBackend. The original four integration tests covered repeated edits,
Unicode unsaved source, equal text/revisions with distinct identities, identical
destination paths, independent selection, old snapshots, close/unbind/rebind,
quit/EOF, read errors after committed edits, startup registration/output failures,
read-only failed save, and no implicit dispatch/session installation. The unit
regression checks that the original borrowed shell drops bindings between calls.

The original report also recorded successful TUI library/integration tests,
all-feature checking and the example check. **No live terminal/PTY handoff was
manually exercised in S2**: stream lifecycle and synthetic rendering are separate
from terminal input and alternate-screen behavior. Later PTY evidence is recorded
in [S3/TUI1](S3_RESULTS.md), not retroactively attributed to S2.
