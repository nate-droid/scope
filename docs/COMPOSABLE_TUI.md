# Building a Composable TUI with Scope

## Run the example

From the repository root, in a terminal with interactive stdin/stdout:

```sh
cargo run -p scope-ide --features tui --example composable_workspace
```

`crates/scope-ide/examples/composable_workspace.rs` uses the existing Ratatui
0.29 and Crossterm 0.28 dependencies. No agent, network service, execution
adapter, or generated fixture is needed.

1. Use **Up/Down** or **j/k** to change the focused pane's local selection.
2. **F6 / Shift-F6** moves focus forward/backward among visible panes.
3. Select a row in `details`, then press **F7** to cycle between `2x2` and
   `focused`. The same `details` instance keeps its selection. Hidden panes
   keep theirs too.
4. **F2** or **Esc** suspends the workspace and opens the normal Scope shell.
   Try `:scope downstream demo status`, then `:scope quit` to return.
5. **Ctrl-Q** exits the workspace. Plain `q` and Tab are not host shortcuts;
   they are available to panes (this example does not handle them).

## Build your first downstream workspace

The complete runnable source is
[`composable_workspace.rs`](../crates/scope-ide/examples/composable_workspace.rs).
Copy it as a starting point: it implements one small `Pane` type and constructs
four instances with different content. Replace its immutable synthetic `Domain`
with your own state, keeping per-view selection in each pane. No custom terminal
setup, event polling, or shell implementation belongs in your application.

The essential composition is:

```rust,ignore
let mut ui = WorkspaceTui::new();
ui.register("source", source_pane)?;
ui.register("results", results_pane)?;
ui.register("details", details_pane)?;
ui.register("output", output_pane)?;
ui.layout("laboratory", View::vertical([
    View::horizontal([View::leaf("source"), View::leaf("results")]),
    View::horizontal([View::leaf("details"), View::leaf("output")]),
]))?;
ui.layout("focused", View::vertical([
    View::leaf("source"),
    View::horizontal([View::leaf("results"), View::leaf("details")]),
]))?;
ui.run(&mut app, &mut session, Some(&mut command_host))?;
```

This sketch assumes the pane and host values already exist; the linked example
contains all imports, implementations and construction, and compiles without
implementation-private APIs. Register a second instance of a pane type under a
new key when comparing two selections of the same domain, rather than repeating
one key in the same layout.

## Construction and ownership

The public construction path is `Workspace::read_only(env!("CARGO_MANIFEST_DIR"))`
followed by `App::builder(workspace).build()`. This anchors browsing to the
example's crate without creating files or granting workspace write authority.
`InteractiveSession::new()` starts without an execution adapter.

`WorkspaceTui::new()` owns registered panes and their per-instance scroll state.
`register(id, pane) -> Result<(), String>` takes a unique, nonempty instance key.
The four example instances all return `"synthetic-list"` from `Pane::id()`:
that is a pane **type** identifier, not the instance key used by layouts.

`layout(name, view) -> Result<(), String>` adds a named preset;
`select_layout(name)` selects one by name and returns an error for unknown names.
Presets refer to already registered instances, rather than recreating panes.
Empty/duplicate names, unknown leaves, empty splits, invalid tab selections,
and nesting beyond the supported depth are rejected. A duplicate leaf anywhere
in one layout is rejected, even across inactive tab children. Reusing an
instance in different presets is supported.

Compose values with:

```rust,ignore
View::leaf("details")
View::horizontal([View::leaf("overview"), View::leaf("details")])
View::vertical([View::leaf("overview"), View::leaf("details")])
View::split(Direction::Horizontal, [
    (Constraint::Percentage(30), View::leaf("overview")),
    (Constraint::Min(0), View::leaf("details")),
])
View::Tabs {
    selected: 1,
    children: vec![View::leaf("overview"), View::leaf("details")],
}
```

The last two forms use `ratatui::layout::{Constraint, Direction}`. Horizontal
means side by side; vertical means stacked. Equal splits use `Constraint::Fill(1)`.
Ratatui solves constraints; tiny terminals can produce zero-sized, invisible
children. Focus follows visible leaves in layout order.

Implement `Pane::render(&self, frame, area, &TuiContext)` for drawing and
`Pane::handle_key(&mut self, key, &mut TuiContextMut) -> bool` for focused input.
The supplied area is already inside the workspace's border. `TuiContext`
provides read-only App access, focus and scroll information. `TuiContextMut`
provides viewport/scroll helpers and mutable App access; the example deliberately
ignores that authority and only changes its own selection.

The four synthetic views and the command host share an `Rc<RefCell<Domain>>`.
Rendering borrows it immutably, input only reads its row count, and no callback
mutates domain data. Keep borrows short; `RefCell` is runtime borrow checking,
not a concurrency primitive or security boundary.

## Investigation: editor versus workspace

These are complementary layers, not competing terminal runtimes:

- `crates/scope-terminal/lib.rs` and its README describe the synchronous line
  reader: input editing, history, completion, paste handling and raw-mode
  lifecycle. `document.rs` supplies editing primitives. The normalized-layout
  paste path preserves normalized LF/tab data but displays an escaped single-line
  viewport; it is not a full multiline editor or a Ratatui workspace.
- `crates/scope-ide/src/tui/mod.rs` contains the existing default IDE TUI, pane
  exports, and its own shortcuts. Its Tab/quick-open/semantic-inspection controls
  should not be confused with the composable workspace's F-key controls.
- `tui/composition.rs` adds value-based `View` layouts, instance storage, focus,
  and the `WorkspaceTui` event loop. `tui/pane.rs` defines the public pane contexts.
- `cli::run_terminal_borrowed` is the bridge to the existing Scope shell; it
  borrows the caller's App, session and optional `ReadOnlyCommand` host rather
  than constructing another runtime or a second shell implementation.

`WorkspaceTui::run(&mut self, &mut App, &mut InteractiveSession,
Option<&mut dyn cli::ReadOnlyCommand>)` returns `Result<(), Box<dyn Error>>`.
On F2/Esc it drops the terminal guard before starting the borrowed shell, so
there are not two concurrent terminal readers. On successful shell return it
re-enters the alternate-screen UI. Errors propagate out with terminal guards
dropped, rather than silently resuming.

App, session, registered panes, domain and command host survive the round trip.
Explicit shell session commands can still reset, replace or clear a session.
Scratch buffers, pending proposals, inspection state and submission capture are
**shell-invocation-local** and discarded on return; nothing is auto-saved.
Shell history follows the existing shell configuration, not pane state.

### Reused launcher, session, and source contracts

Existing `cli::run_with_registry_and_command` launchers register languages and
namespaced shell callbacks while retaining Scope's argument parsing and workspace
authority setup. Their default `--tui` path remains the existing IDE TUI; it does
not implicitly choose downstream presets. The example explicitly constructs the
App and chooses `WorkspaceTui`, passing the same host into its shell handoff.
Language registration and session execution adapters remain independent of views.

The App's open document is not a shell scratch buffer. Shell-selected source and
the separately bound semantic scratch target are also distinct: namespaced
commands receive the latter (or no target), not an implicit document fallback.
Scratch editing never implicitly evaluates or saves. Domain mutations should use
existing semantic operations or command callbacks with the host's normal policy,
not a new UI action dispatcher. Selection/scrolling are purely pane-local actions.

## Security boundaries

Layout registration and rendering do not install capabilities or execution
contexts. The sample uses synthetic strings, a read-only workspace, and an empty
session. The only downstream command is `:scope downstream demo status`; it
accepts no arguments and prints a row count using an immutable domain borrow.
The required document-operation callback rejects all operations, and default
script callbacks remain unsupported. Unknown downstream commands are unhandled.

`ReadOnlyCommand` is a trusted native-host contract, not a sandbox. Its name does
not prove callback purity. Likewise a pane can access mutable App state during
input, and arbitrary native pane code could perform external effects. Real hosts
must authorize concrete operations and avoid adding execution, writes or provider
credentials merely to display a pane. This example supplies none of those.

## Deferred features

- **Tab chrome and tab-switch keys:** `View::Tabs` renders only the selected child;
  `selected` is chosen by the application in the layout value. There is no tab bar
  or interactive tab-selection controller. F7 cycles named presets, not tab indices.
- **Inline shell:** intentionally deferred. The current bridge releases terminal
  ownership and runs the normal shell. A future inline shell must preserve draft
  input/cursor, history, completion, paste semantics and cancellation without
  competing readers or weakening shell authorization. This example does not
  preserve shell drafts or scratch buffers between invocations.
- Dynamic layout editing, persistence, mouse resizing, asynchronous domain feeds,
  and execution adapters are outside this small demonstration.

## Validation

```sh
cargo check -p scope-ide --features tui --example composable_workspace
```

Automated validation for this milestone:

- `cargo test -p scope-ide --features tui --lib`: **397 passed**, including
  composition geometry, tab selection, constraints, tiny sizes, duplicate and
  unknown identities, two instances of one pane type, focus/input routing,
  retained state, borrowed shell lifecycle, and existing downstream commands.
- `cargo test -p scope-terminal`: **29 passed**, covering existing input primitives.
- `cargo check -p scope-ide --all-features --examples`: passed.

Terminal restoration is guarded on normal exit, errors and unwinding, including
partial setup failure after raw mode is enabled. Actual terminal mode restoration
has not been exercised through a PTY in this milestone; deterministic rendering
uses Ratatui's TestBackend. Resize the example down to a tiny window and back,
enter/leave its shell twice, and exit with Ctrl-Q to smoke-test restoration and
state retention in your emulator.

Compilation checks the public integration. The key sequence above is the manual
interactive smoke test; compilation alone does not verify terminal restoration
or function-key delivery in a particular terminal emulator.
