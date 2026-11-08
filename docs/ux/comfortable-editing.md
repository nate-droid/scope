# Comfortable editing and discoverable feedback (UX6b)

## Decision

**Ready for human validation:** Scrolling feels predictably contextual in the verified scenarios, and Check is discoverable without awkward keys.

This is focused UX readiness, not Full verification or release confidence. All
changes are in Scope; no sibling repository was edited.

## Baseline and reproduction

Revision: `6fa677564e38a54bc65cad9dd07b31bd5c5a6c62`. Initial
`git status --short` was empty. No unrelated changes were present.

Inspected the UX5 quiet composition, UX6 renderer and indentation report,
App-backed `EditableScratchPane`, shared `DocumentEditor`, keyboard dispatch,
checked publication/Check action, generic diagnostics and the authoritative
[verification policy](../verification.md).

Before changing the viewport, the real workbench visible-cell PTY passed its
old bottom-pinning oracle. At source cursor row 59, a 31-row editor displayed
`top=29`; shrinking to 25 rows displayed `top=35`; restoring 31 displayed
`top=29`. The cursor occupied the final visible row in each case. The renderer
computed top directly as `row - height + 1`, with no retained vertical origin.
This also explains why moving upward could keep moving the entire viewport.
The updated contextual oracle failed against that implementation before the fix.

## Viewport policy

The existing draft retains a vertical origin. Rendering changes it only when
the cursor enters a three-line top/bottom margin. The margin is
`min(3, (height - 1) / 2)`, so tiny editors retain a visible cursor. It is not
configurable. Each parked draft retains its own origin; source offsets and undo
history remain unchanged. Resize adjusts only enough to restore context, rather
than recentering or resetting the viewport.

The origin is not clamped to document length: EOF can have three blank rows
below it. Near the beginning there cannot be three source lines above the
cursor, so top remains zero. Large pastes/jumps move just enough to reveal the
cursor with context. Insert/delete/indent/undo retain the origin whenever the
cursor remains inside the comfortable region. No unconditional clears or new
redraw mechanism were added. Existing horizontal clipping, tab expansion,
wide-grapheme handling and UTF-8 byte authority remain intact.

## Check interaction and diagnostics

**Ctrl-T Check** is shown beside Ctrl-O Open, Ctrl-R Run, Ctrl-Q Quit and
Ctrl-G More in the quiet footer. Alt-Enter and Shift-F5 remain compatibility
aliases, not required interactions. Ctrl-K was rejected because the existing
shell input owns kill-line. Ctrl-Z/Y history, Ctrl-R Run and Tab indentation
remain unchanged. Ctrl-T was unowned in Scope's inspected editor/shell command
paths. It uses the ordinary control byte `0x14`, not a modifier-specific escape
sequence.

Ctrl-T conventionally transposes characters in shell/readline editing and can
request status in cooked BSD terminal input. Scope's retained workspace runs in
raw mode; this binding is a workspace action, not a change to external shell
editing. Existing command-entry/modal precedence is retained. Mac Terminal,
iTerm2 and integrated terminals normally deliver this control byte, but their
user-configured interception and cooked-shell behavior are not established by
our PTY tests. Human checks on those applications remain necessary.

Check continues through the existing exact App draft publication and checked
baseline path, then calls the generic static diagnostics service. It does not
execute, save, select scratch as a fallback, or move editor focus. Stale/rebound
publication and unrelated dirty draft rejection remain in place. Run policy and
current/stale result retention were not changed.

The quiet output replaces old feedback with **No errors**, or existing concise
`line:start-column:end-column: severity [attribution]: message` diagnostics.
No App revision/source-ID header is shown. The advanced output retains its
technical header and richer inspection paths. Diagnostic wording/category and
any corrective advice remain language-service-owned; Scope does not invent
language-specific corrections. The small output strip can truncate long/many
diagnostics; advanced output actions remain available through Ctrl-G. No new
pane, automatic execution, passive scheduler or live checking was added.

## Terminal/navigation findings

Arrow keys and normal editing are the ordinary navigation path. Tab/Shift-Tab
retain UX6 indentation and undo behavior. No Function key or Page key is needed
for ordinary editing or Check. The retained TUI has no mouse event handling or
mouse capture; wheel/trackpad scrolling is therefore **not implemented or
verified**. Depending on terminal settings a wheel may affect terminal history
or synthesize keys; that is not a supported cursor-independent viewport gesture.
Adding mouse input was deliberately not part of this milestone.

## Verification ledger

Commands ran at the Scope root, pinned toolchain, default features unless noted.
Existing root artifacts were reused. Duplicate-package notices refer to the
existing cached Cell dependency, not edits to Cell.

| Command | Result |
| --- | --- |
| `cargo test --locked -p scope-workbench --test terminal_pty editor_visible_cells_pty -- --exact --nocapture` | Baseline passed old bottom-edge oracle; contextual oracle then failed before fix; final implementation passed 1 real PTY plus 2 Python model tests. Final measured runtime 23.73 s, incremental build 2.41 s. |
| `cargo test --locked -p scope-ide --features tui --lib tui::scratch_editor` | 19 passed, including contextual origin, tiny heights, Unicode, history and source rejection. |
| `cargo test --locked -p scope-ide --features tui --lib tui::composition::tests` | 58 passed: Check dispatch/aliases, footer, diagnostics, publication and neighboring actions. |
| `cargo test --locked -p scope-ide --features tui --lib --test retained_workspace` | 515 unit tests and 4 retained-workspace integration tests passed. |
| `cargo test --locked -p scope-ide --features tui --test downstream_launcher` | 3 passed; 2 intentionally ignored PTY child entries. |
| `cargo test --locked -p scope-ide --features tui --test downstream_launcher h4_shortcuts_through_real_launcher -- --exact` | 1 passed after setting the extended scenario's bounded deadline to 120 s. |
| `cargo test --locked -p scope-workbench --test terminal_pty workbench_end_user_pty -- --exact` | 1 passed. |
| `cargo clippy --locked -p scope-ide --features tui --lib -- -D warnings` | Passed on final implementation, including viewport changes. |
| `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v` | 5 passed. |
| `python3 -B .github/scripts/check_markdown_links.py` | Passed, including this report. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |

The bounded UX6 `VisibleScreen` reconstruction checks editor cells and cursor
coordinates, not merely transcript substrings: downward/upward scrolling,
beginning/EOF, short resize, long horizontal lines, tabs, wide glyphs, combining
accents, boundary insert/delete/undo, multiline paste, indentation and resize.
Explicit Save verifies exact UTF-8 bytes separately. The workbench fixture is a
scratch-backed editor; it cannot prove App Check. The separate real App launcher
now also uses `VisibleScreen`: unpublished error draft Check, corrected clean
Check, unchanged cursor coordinates, immediate continued editing, no disk write
or evaluation, and exact checked revisions are asserted. Existing Run executes
its exact revision once, and Open/More/dirty-Quit/terminal restoration remain
covered.

Direct and adjacent checks plus selected real System PTYs were chosen for the
internal UI input/presentation boundary. Public APIs, source lifecycle rules,
authorization, dependencies and feature wiring were not changed. Full workspace
default/all-features tests, all-target lint, isolated feature matrix, rustdoc and
standalone/scaffold consumers were **not run**. This does not close historical
UX5/UX6 Full or broad workspace PTY gaps. No live/paid models or credentials were
used. Complex emoji/ZWJ terminal widths and real terminal/platform parity remain
outside the bounded screen model.

## Human validation and next step

Try a real long source file on a MacBook in Terminal, iTerm2 and an integrated
terminal: hold arrows in both directions, edit near the margins, resize, then
Ctrl-T on a clean and broken unpublished draft. Confirm the three-line margin
feels natural and the footer is readable at the widths actually used. Validate
custom terminal key mappings and Shift-Tab delivery. Assess whether the output
strip is enough for ordinary diagnostics before expanding presentation.

Recommended next UX step: test discoverability and diagnostic truncation with
humans; polish only the observed bottleneck. Cursor-independent mouse-wheel
scrolling is a concrete future input-boundary task, not claimed here.
