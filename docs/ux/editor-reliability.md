# Editor reliability (UX6)

## Result

**Ready for human validation:** Rendering is reliable under the reproduced
conditions, and Tab works naturally.

The reproduced defect was **rendering corruption, not document corruption**.
All changes are in Scope; no downstream language implementation was changed.
The quiet editor/output composition and Ctrl-O Open, Ctrl-R Run, Alt-Enter
Check, Ctrl-Q Quit bindings are unchanged.

## Reproduction and diagnosis

The real writable App editor was opened in a Unix PTY with this literal source:

```text
\tlet café = "猫";
\tlet second = 2;
```

Here `\t` denotes a literal source tab. Typing `X`, pasting `λ`, and navigating
across the tab should display the first line as `Xλ  let café = "猫";`.
Before the fix, reconstructed terminal cells instead showed:

```text
Xλ   clet café = ";
```

After focus changes and resize they showed `Xλ    lcafé = "猫";`.
An explicit App Save nevertheless preserved the exact expected UTF-8 bytes:
`Xλ\tlet café = "猫";\n\tlet second = 2;`.

Captured terminal output included:

```python
b'\x1b[3;4H\xce\xbb\tlet caf\xc3\xa9 = "\xe7\x8c\xab\x1b[3;20H";'
```

The renderer put raw HT into a cell-oriented backend. The terminal interpreted
it as movement to its own absolute eight-column tab stops. Ratatui's cell
positions, cursor placement, horizontal clipping and terminal positions no
longer agreed. Later partial updates could therefore leave overlapping or stale
characters. This was not evidence of incorrect source edits or revision loss.

## Correction

`crates/scope-ide/src/tui/scratch_editor.rs` now converts source graphemes to
safe display strings before constructing spans. Literal source tabs expand at
four-column, source-relative display stops. Other control graphemes display as
replacement characters rather than terminal commands. The same display widths
are used for the cursor and horizontal viewport. A clipped wide grapheme leaves
blank cells at either viewport edge instead of emitting half a glyph.

Source bytes and selection offsets remain authoritative UTF-8 offsets. Display
expansion never edits the source. No unconditional clears, delays, renderer
replacement, or terminal framework was introduced. Existing Ratatui diffing and
resize handling remain in use.

The inline document editor already escapes non-ASCII/control scalars for safe
ASCII output; it did not share the raw-tab TUI defect. It retains that rendering
policy. Vertical navigation retains the existing grapheme-column policy rather
than introducing a new preferred visual-column contract.

## Tab editing

`crates/scope-terminal/document.rs` inserts soft spaces to the next four-grapheme
source-column stop (existing literal tabs advance to a stop). Shift-Tab, decoded
as BackTab or shifted Tab, removes only leading ASCII spaces to the previous
indentation stop. It never deletes non-whitespace source or existing literal
tabs. Cursor offsets adjust safely even when the cursor is before the removed
indentation. A terminal must transmit a distinguishable Shift-Tab sequence.

In the retained editor, Tab/Shift-Tab indent/dedent all selected lines in one
undoable operation, preserve selection direction, and exclude a following line
when the selection ends at its column zero. Ctrl-Z/Ctrl-Y restore text, cursor,
and selection. No-op dedent adds no history. Key releases do not edit history.
New indentation does not publish a draft or change the committed revision.
Quick Open and shell completion keep their existing Tab handlers.

No editor indentation setting existed, so the default is four spaces. Source
column stops after wide characters remain grapheme-based in the shared editing
primitive; rendering and cursor coordinates use terminal display widths.

## Completion investigation

Completion is deferred, not implemented or hardcoded for any language.
`cli/buffer_editor.rs` already accepts a host completion callback, and
`shell_input.rs` validates UTF-8 replacement ranges and applies checked candidates.
The retained document editor does not expose that provider bridge. Existing
shell completion alone is not an appropriate semantic provider for a source
pane.

The smallest useful future contract is a host-owned request containing source
identity, draft revision/text and UTF-8 cursor offset, returning labeled candidates
with a byte replacement range and insertion text. The host must revalidate or
recompute against the current draft before acceptance. An explicitly active
suggestion could consume Tab and apply one history transaction; absent a useful
candidate, Tab must continue to indent. This needs neither language keywords in
Scope nor a new indexer/language server, but the provider-to-retained-editor
integration is outside this fix.

## Regression evidence

`crates/scope-workbench/tests/visible_screen.py` is a bounded stdlib-only VT cell
model. It reconstructs fragmented UTF-8, cursor addressing, erase operations,
terminal tabs, BMP wide characters and combining accents. It rejects unsupported
operations instead of treating transcript matches as visual proof. Bounds are
200 rows, 500 columns and a limited pending sequence buffer. It does not assert
general terminal emulation, styles, emoji/ZWJ width or all terminal implementations.

`pty_visual_editor.py`, registered as `editor_visible_cells_pty`, runs the real
Scope binary with a writable App document and a 90-second deadline. It compares
all visible editor cells, trailing blanks and cursor coordinates after typing,
multiline bracketed paste, repeated insert/delete/undo cycles, Tab/Shift-Tab with
undo/redo, vertical navigation through 60 lines, horizontal scrolling, clipping
wide glyphs at both edges, focus roundtrips, resize while scrolled, publication
and explicit Save. Exact saved UTF-8 bytes are checked separately. The test failed
on the original renderer and passes after the correction. The final focused run
took 12.62 seconds with a warm 0.46-second build; these are measurements, not cold
build estimates.

Unit coverage also checks safe control/tab display conversion, selected-line
indentation history, cursor offsets, unchanged published text/revision, modifier
guards, replacement errors and source-tab preservation.

## Verification ledger

All Cargo commands use default features unless `--features tui` is specified;
workbench defaults include `tui`. Commands ran from the repository root.

Passed:

- `cargo test --locked -p scope-terminal --lib document::tests`: 12 tests.
- `cargo test --locked -p scope-terminal --lib`: 35 tests.
- `cargo test --locked -p scope-ide --features tui --lib tui::scratch_editor`:
  final run 18 tests, including control display and collapsed-selection dedent regressions.
- `cargo test --locked -p scope-ide --features tui --lib`: 511 tests before
  adding that regression; the production renderer and indentation changes were present.
- `cargo test --locked -p scope-ide --features tui --lib display_cells_expand_tabs_and_never_emit_controls`:
  1 added regression.
- `cargo test --locked -p scope-ide --features tui --test retained_workspace --test downstream_launcher`:
  retained workspace 4 passed; launcher 3 passed, 2 intentionally ignored PTY child entries.
- `cargo test --locked -p scope-workbench --bin scope`.
- `cargo test --locked -p scope-workbench --test terminal_pty editor_visible_cells_pty -- --exact --nocapture`:
  1 real PTY test plus 2 Python model tests; final rerun after the collapsed-anchor
  correction passed in 12.77 seconds (warm build 1.99 seconds).
- `cargo test --locked -p scope-workbench --test terminal_pty workbench_end_user_pty -- --exact`:
  1 passed, preserving the ordinary workbench workflow.
- `python3 -B crates/scope-workbench/tests/pty_visual_editor.py -v`: 2 tests.
- `python3 -B crates/scope-ide/tests/test_pty_sync.py -v`: 9 tests.
- `cargo test --locked -p scope-script --test terminal_pty`: 8 tests.
- `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v`: 5 tests.
- `python3 -B .github/scripts/check_markdown_links.py`: passed.
- `cargo fmt --all -- --check` and `git diff --check`.

An early exploratory `cargo test --locked -p scope-workbench --test terminal_pty -- --nocapture`
reproduced the visual regression before correction; ordinary workbench and existing
terminal consumer tests passed, but the invocation timed out at 200 seconds with
`workspace_end_to_end_pty` still running. It was not repeated as a broad check.
A later `cargo test --locked -p scope-workbench --lib` was rejected because the
package has no library; the actual binary target was checked instead.

### Outstanding System / Full assurance

The focused tests establish the reproduced defect and nearby editor safeguards,
not repository-wide correctness. The complete timed-out workspace PTY remains
unverified. The Full matrix in [the verification policy](../verification.md)
(workspace default/all-feature tests and Clippy, IDE isolated feature configurations,
rustdoc and standalone/scaffold consumers) has not been run for this milestone.
The shared Script terminal consumer checks passed separately. No dependencies,
public signatures, source
identity or authorization contracts changed. No live/paid model calls were used.
Human terminal validation and hosted platform coverage remain necessary, especially
for terminal-specific Shift-Tab decoding and complex Unicode width behavior.
