# Scope Desktop preview

From the Scope checkout, on a graphical macOS session:

```sh
cargo run --locked -p scope-workbench --features desktop --bin scope-desktop -- . --open README.md
```

Omit both `.` and `--open README.md` to enter a directory in the project opener.
`--open` takes a path relative to the project.
The project is read-only unless you pass `--writable` or explicitly allow saves in
the project opener. The defaults for `cargo run` and the TUI are unchanged.

## Edit and inspect

- Browse the project tree, filter it by filename, or use **Quick Open**
  (**Cmd/Ctrl-P** or **Cmd/Ctrl-O**). **Recent** lists App files opened in this
  session. Opening a file creates or selects its tab. Click a tab or use
  **Ctrl-Tab / Ctrl-Shift-Tab** to cycle. Tabs mark unpublished or unsaved work
  with a dot. Text, cursor, selection, viewport and undo history stay with the
  document when you switch. Close a clean tab with **×** or **Cmd/Ctrl-W**.
  A dirty tab refuses to close: save it, or undo the changes you don't want.
- Edit in the workspace pane. Use Shift+arrows to select, OS clipboard
  shortcuts to copy/cut/paste, and **Cmd/Ctrl-Z** to undo. Use
  **Cmd-Shift-Z** or **Ctrl-Y** to redo. The editor shows line numbers and a
  line/column readout. **Tab / Shift-Tab** indent or dedent; in Sprout files,
  **Enter** carries forward leading whitespace. `.sprout` App documents use a
  small lexical colorizer on visible lines. It follows Sprout token boundaries
  but is not the canonical compiler lexer: an unterminated string is colored to
  the end of its line without affecting later lines. Other files keep the
  generic presentation. In an explicitly writable workspace, **Cmd/Ctrl-S** with
  focus in the App editor publishes the draft and saves the active document
  through Cell's revision-checked write. An external edit rejects the write and
  leaves the published source in memory. In read-only workspaces and scratch
  editors, the shortcut only publishes. **Actions → Publish editor draft**
  always updates memory only. The toolbar distinguishes unpublished work,
  published but unsaved App source, and saved source.
- **Actions** (**F1** or **Cmd/Ctrl-G**) opens the same contextual action picker
  as the TUI. The installed language service and the current source determine
  whether Check, Format, Run, Run Test, navigation, and inspection are available.
  The stock launcher has Scope Simple diagnostics. Use Sprout's launcher below
  for Sprout language services.
- To see genuine execution without a downstream runtime, choose **New Script**,
  enter `40 + 2`, publish, and press **F5**. Append ` + 1`, publish, and run again.
  The output pane shows the retained result (`42` then `43`); **Alt-Left/Right**
  steps through Script results. This is Script result history, not the structured
  **Compare Results** action that requires compatible semantic observations.
  **F6** changes pane focus and **F3** returns to the previous pane.
- Closing the window with unsaved work asks whether to keep editing or discard.
  The window can repaint and resize while the workspace is processing a Run;
  the workspace itself still processes source and execution requests serially.

## Rendering and boundaries

The opt-in `desktop` feature adds `eframe` with the Glow renderer and Linux
X11/Wayland feature support. The project opener, tree, filter, toolbar, clipboard,
and close confirmation are native egui widgets or integration. The editor,
output, action picker, semantic inspector, Result Explorer, and Compare share
the existing Ratatui rendering and workspace loop. The desktop painter reads a
Ratatui frame buffer; it does not emulate a PTY or create a separate editor model.
The current `egui_ratatui` 2.x uses `ratatui-core` 0.1, whereas Scope uses Ratatui
0.29. The older compatible `egui_ratatui` 0.0.31 also pins egui 0.31 and documents
a HiDPI rendering issue. No Ratatui upgrade was needed for this preview.

Opening and saving use the existing Cell-backed workspace mount. Actions dispatch
through the existing launcher host and guards. Desktop does not install new
capabilities, register agents or language producers, or launch a child shell.
macOS is the first launch target; Linux GUI behavior is not yet validated.

## Sprout development

For daily use, install the Desktop-enabled CLI once from a build-ready Sprout
checkout:

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked --features desktop --bin sprout --target-dir target
install -m 755 target/debug/sprout ~/.cargo/bin/sprout
```

Then run `sprout ide .` from any project directory. This opens a writable
project, even if it is empty. Use `--read-only` to opt out of disk writes or
`--open main.sprout` to open a file immediately. No module enrollment or
artifact configuration is needed.

Create `main.sprout` with `fn main() -> Int32 { 40 + 2 }` using your usual file
creation tool. Open it with **Quick Open**, choose **Actions → Format** and
apply the preview if needed, then **Check** (Ctrl-T) and **Run** (Ctrl-R). The
output shows `42`; press **d** for captured details. Save with **Cmd/Ctrl-S**.
In this preview, Run selects the zero-argument `main` in the focused root.
A local text file can be opened for editing, but Run does not yet pass it as
an argument. Use an escaped string literal for small inputs. Run imported tests
from the project directory with `sprout test .` while leaving Desktop open.
Sprout's `examples/puzzles/` directory includes a grouped-totals example and
its test and input fixtures.

### Advanced preview launchers

From the Scope checkout, launch a maintained standalone Sprout example:

```sh
cargo run --locked --manifest-path ../sprout/Cargo.toml --features desktop --example scope-desktop -- ../sprout/examples/programs --open result_observation.sprout
```

1. Edit the source. **Actions → Format** drafts a preview; **Enter** applies it
   locally without publishing. **Ctrl-T** or **Alt-Enter** publishes the focused
   App draft and checks that in-memory revision. It does not write to disk.
   For a standalone source diagnostic, **Actions → Navigate result (if current)**
   jumps to the first diagnostic only if the checked editor snapshot is still current.
2. **Actions → Evaluate / inspect document** offers `evaluate-typed`. Choose it
   and press **Enter** to review, then **Enter** to authorize the captured App
   revision. Publish or Check before opening this action; it does not read the
   unpublished draft or the file on disk. **Ctrl-R** is the ordinary preview Run.
3. Press **x** on a result to open **Result Explorer**. Use Right to expand and
   Up/Down to select. Press **Esc** to return to the editor, change `observe(21)`
   to `observe(24)`, and repeat with `evaluate-typed`. The checked nested
   `Observation.measurement.total` changes from 42 to 48.
4. Open **Actions → Compare Results**. `n`/`p` step through differences, and
   `1`/`2` open each observation in Explorer. The original remains historical
   after the edit; neither navigation nor comparison re-executes it.

Registered multi-module Run and the standalone cursor-sensitive test host are
separate maintained Sprout routes:

```sh
cargo run --locked --manifest-path ../sprout/Cargo.toml --features desktop --example scope-desktop -- ../sprout/examples/village_expedition --modules . --open main.sprout
cargo run --locked --manifest-path ../sprout/Cargo.toml --features desktop --example scope-desktop -- ../sprout/examples/programs --test-file scout.sprout
```

In module mode, Check publishes the focused App draft and discovers the reachable
registered closure without execution. Run authorizes the exact captured closure
under Sprout’s existing Cell policy and displays the result without confirmation.
Use **d** in Output or **Evaluate / inspect document** for retained invocation
details. If a launcher requires review, **Esc** cancels before confirmation; there
is no in-flight cancellation support. Published
but unsaved modules are still owned by the registered App and can participate in
later checks. The retained registered result is available through **Evaluate /
inspect document**, but this producer returns a bounded text preview, not the
checked JSON tree required for the typed Compare workflow above.

`--test-file` opens a standalone file as a scratch source. Put the cursor inside
`test_unsafe_location_recovery` and choose **Run Sprout test**. Outside a test the
action asks for a name. Check and Test use the exact pinned scratch draft, including
unpublished edits; **Navigate result** applies only while that evidence is current.
This scratch host does not handle imported tests. For imported projects use the
checked-entry workflow below; the maintained Sprout CLI also supports explicit
additional source roots.

Sprout supplies the configured App, language service, and optional producer to
`WorkspaceConfig::run_desktop` via `scope_workbench::desktop::launch`. Desktop
renders the same workspace and structured observation surfaces as the TUI. It
adds no language parser, execution shortcut, or result store. The workspace
processes input serially: the native window can repaint independently and shows
**Workspace processing…** for pending work, but editor input is queued until
execution returns. Result provenance is bound to the original invocation.

### Checked project runs and imported tests

The default Sprout features include `call-bundle-v1`, which supports zero-argument
entries returning `Bool` or checked records with `Bool`/`Int32` leaves, including
nested records. `--checked-entry` selects this mode; ordinary module Run without
the flag still uses the bounded preview producer. Both checked launchers use the
same Cell bundle-call host and Scope's existing registered review, completion,
Result Explorer and Compare. The JSON tree is projected from the verified return,
not decoded from display text. Source identity stays in the observation binding.

From Scope, open the maintained village expedition project in Desktop or TUI.
`--modules` names the workspace-relative module directory; it does not enroll
its files. Only `main.sprout` opens at launch. Check and Run use Sprout's import
graph to acquire `journey.sprout`, `policy.sprout`, and the transitive
`supplies.sprout`; the two test files are not loaded for `main`.

```sh
cargo run --locked --manifest-path ../sprout/Cargo.toml --features desktop --example scope-desktop -- ../sprout/examples/village_expedition --modules . --open main.sprout --checked-entry main
cargo run --locked --manifest-path ../sprout/Cargo.toml --example scope -- workspace ../sprout/examples/village_expedition --modules . --module main --checked-entry main
```

1. Use a temporary working copy if saving. Format with **Actions → Format**, then
   apply the draft and press **Ctrl-T** to publish and Check. To try diagnostic
   navigation, introduce a syntax error in `journey.sprout`, publish it, return to
   `main.sprout`, and Check. **Actions → Navigate result (if current)** opens the
   acquired import at the checked byte location. It refuses a changed document
   revision or unpublished draft. Execution faults currently remain textual.
2. With `main.sprout` active, press **Ctrl-R**. The checked plan appears in Output
   without an approval prompt. Use **d** for the captured invocation or **Actions →
   Inspect Result** to open Result Explorer. Return to the editor with **F3** after
   closing Explorer. Open `journey.sprout`, change `(days + 1)` to `(days + 2)`,
   and publish with **Ctrl-S** in the read-only launch. Switch back to
   `main.sprout`, then Run again. **Actions → Compare Results**
   shows `food_needed` 30 → 36, `water_needed` 45 → 54 and `permitted` true → false. The first observation is
   historical; `1` and `2` open either observation without running it again.
3. For a maintained imported test, launch the applicable root with the test
   function as the checked entry, then use the same **Run** without confirmation. Test
   selection is explicit for the session, not cursor-sensitive as in scratch mode.

```sh
cargo run --locked --manifest-path ../sprout/Cargo.toml --features desktop --example scope-desktop -- ../sprout/examples/village_expedition --modules . --open planner_test.sprout --checked-entry test_permitted_plan
cargo run --locked --manifest-path ../sprout/Cargo.toml --features desktop --example scope-desktop -- ../sprout/examples/imported_testing --modules . --open intentional_failure.sprout --checked-entry test_wrong_square_expectation
```

Expected feedback: `PASS test_permitted_plan`; and for the intentional failure,
`expected: 48, received: 49`. For responsiveness, use the same failure fixture
with `--checked-entry test_resource_exhaustion` and record the interval from
Run to result separately from window repaint and input handling. Resize
while running, look for **Workspace processing…** (shown after 180 ms), and
queue a clean document switch. Verify the editor eventually switches and the
observation still names its captured root. If the call is too short to interact
with, record that limitation rather than claiming responsiveness. There is no in-flight cancellation. These native checks require a graphical session.

### Registered launcher policy

A launcher with an established host policy can opt into
`RegisteredExecution::with_policy_authorized_run`. Its review classifier inspects
only the sealed invocation and returns a short reason if human approval is
required. For example, name the resource requested and explain why routine Run
approval does not cover it. Returning no reason skips confirmation, not policy:
Scope still asks the host policy about the exact sealed invocation before
dispatch. A denial never turns into an approval prompt, and approval cannot
expand the effective execution policy. Launchers that do not opt in still
require confirmation. Full captured details remain available during review with
**d**, and after completion through **Evaluate / inspect document**.

### Work on Sprout's frontend

From a Sprout checkout with the optional Scope desktop integration available:

```sh
cargo run --locked --features desktop --example scope-desktop -- . --open frontend/lexer.sprout
```

Open `frontend/parser.sprout` and `tests/frontend/checks.sprout` in other tabs.
On a temporary working copy, use **Actions → Format** to preview and apply a
change. Open that copy with `--writable` if you want to save. This launch browses
and edits without configuring registered execution.

For a registered checked return, stage the maintained Boolean fixture and its
modules together in a temporary directory. The registered launcher currently
uses one flat module directory, including the entry. From the Sprout checkout:

```sh
work=$(mktemp -d)
cp frontend/lexer.sprout frontend/parser.sprout tests/frontend/checks.sprout "$work/"
cargo run --locked --features desktop --example scope-desktop -- "$work" --modules . --open checks.sprout --checked-entry main
```

Run `checks.sprout` and confirm Output and Result Explorer both show `true`. Change
`end:end` to `end:start` in the `Token` constructor in `lexer.sprout` and publish
it. Run `checks.sprout` again for `false`, then restore the constructor and run
again for `true`. The fixture checks literal and token spans; it does not
exercise full compiler self-hosting. The CLI still accepts the original layout
with `sprout run tests/frontend/checks.sprout --source-root frontend`.

Opened documents use their published App revisions, even if disk differs. Run
and Check publish the focused draft without saving, and other unpublished drafts
still block the action. Clean tab closure does not discard an acquired document
or its retained observation. Dependencies not opened in an editor are read via
the workspace grant: changed revisions are picked up on the next preparation,
and changes detected during preparation or authorization require a fresh Run.
Opening a project never grants execution or write authority, and the existing
closure resource limits still apply.

Other preview limits: no persistent tabs, no project switching inside a running
window, file tree snapshots are built at project open, and editor scrolling and
selection are keyboard-oriented. Desktop agent and shell handoff are not enabled.
