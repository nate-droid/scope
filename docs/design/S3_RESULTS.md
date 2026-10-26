# S3 / TUI1 — retained scratch editor evidence

This is a historical implementation and validation summary, not the current
launcher tutorial. Use the [workspace tutorial](../../TUTORIAL.md#interactive-workspace)
and [integration guide](../integration.md#contextual-workspace-actions-and-draft-guards)
for current selected-scratch editing, parked drafts, actions and execution.
The original S3 baseline was `a91e23d00f40318f49e8f24429c4524c618e2c30`.

## Ownership and authority established

- The caller owns `ScratchBuffers`; workspace and shell borrow it sequentially.
  App documents, selection and the independent semantic target are not aliases.
- `register_editable` explicitly supplies focused panes with `ScratchEditAccess`.
  Ordinary registration and read-only snapshots do not grant mutation authority.
  The capability does not supply save, execution, target selection or filesystem
  access; trusted native pane code is not thereby sandboxed.
- `EditableScratchPane` reuses `scope_terminal::apply_document_event`,
  `DocumentEditor` and `TextBuffer`, rather than a second accepted-source store or
  keyboard engine. A draft belongs to its persistent pane instance.
- Commit validates source identity, revision and full baseline before changing
  the authoritative scratch. Stale/closed/rebound targets cannot silently receive
  an old draft. Failed commits retain drafts; identical commits add no revision.
  Ctrl-S commits in memory, not disk; Ctrl-C cancels. Neither evaluates, accepts an
  agent proposal, installs a runtime or expands Cell grants.
- Layout changes and shell handoffs retain pane state. Observation sees committed
  snapshots, not private drafts. Historical results keep their original attribution.

S3's original example edited the bound semantic target with one draft. The normal
launcher now edits the selected scratch and retains per-scratch drafts. Do not use
that historical one-draft description as the current launcher contract.

## Read-only pane input (TUI1)

`Pane::handle_view_event(Event, &TuiContext) -> bool` added focused input with
read-only committed source context. Dispatch order is workspace lifecycle keys,
opted-in `handle_source_event`, `handle_view_event`, then legacy key-only
`handle_key`. Returning true stops dispatch. Paste remains an event, not a shell
command; render and typing must not automatically execute a domain operation.

A downstream host must authorize its own operation against the captured snapshot
and retain source identity/revision with its result. Asynchronous results must be
labelled historical when stale; commit is neither execution authority nor rollback.

## Reproduce the neutral example and checks

From the repository root, with cached dependencies for offline commands:

```sh
cargo run -p scope-ide --features tui --example editable_workspace --offline
cargo test -p scope-ide --features tui --lib tui:: --offline
cargo test -p scope-ide --features tui --test retained_workspace --offline
cargo test -p scope-terminal --offline
cargo build -p scope-ide --features tui --example editable_workspace --offline
python3 crates/scope-ide/tests/pty_editable_workspace.py -v
```

The Cargo example requires `tui`. It uses a read-only workspace, unsaved source
and an explicitly host-authorized local metadata action, not a language evaluator.
F6 changes focus, F7 changes layout, F2/Esc hands off to the normal shell,
`:scope quit` returns, and Ctrl-Q exits. The normal launcher's F5 action must not
be confused with the example observer's local metadata shortcut.

## Retained historical validation evidence

The original S3 run reported passing `cargo test -p scope-ide --features tui --lib
--tests`, 29 terminal tests, 9 composition tests and 5 scratch editor tests, plus
example check/build, formatting and diff checks. These are historical counts,
not results from this documentation cleanup.

TestBackend coverage checked source/status/cursor cells, tiny layouts, focus,
layout retention, ordinary registration without editing authority, observer
publication and buffered shell handoffs with stale-draft rejection. Later TUI1
coverage included read-only callback ordering, committed versus draft visibility,
and no observer operation from editor typing or paste.

The real PTY scenario checked `Scratch(0)` versions 1–4, Unicode/trailing empty
lines, TUI and raw `:scope edit` commits, Esc/F2 handoffs, conflicting shell edits,
cancellation, retained final source, normal Ctrl-Q exit, exact terminal attributes
and alternate-screen/cursor restoration. TUI1 also exercised explicit metadata
invocation at revision zero and focus handoff with a dirty draft. Its bounded ANSI
parser checks emitted streams, **not reconstructed screen cells**; TestBackend
provides separate synthetic cell evidence.

Neither scenario establishes real-PTY resize, forced-kill or injected-I/O-error
restoration, all-emulator portability, grapheme editing, undo, automatic merge,
persistent recovery or tested Hilb integration. Tiny-layout tests are not a
readability guarantee. No language evaluation, automatic save or sandboxed native
callback is established by these checks.
