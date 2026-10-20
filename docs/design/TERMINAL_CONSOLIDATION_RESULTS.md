# Terminal consolidation results (T2)

## Decision and scope

Extract one responsibility: deterministic history navigation. Keep Scope IDE's
existing multiline editor and terminal driver. No dependencies, command parsers,
session/agent lifecycle, authorization, proposals, or Cell code were changed.

## Responsibility comparison

| Responsibility | `scope-terminal` | Scope IDE | T2 disposition |
| --- | --- | --- | --- |
| Editing state/operations | `LineBuffer`: UTF-8 byte cursor, scalar movement/deletion, single-line insertion | `Input`: text/cursor plus status and completion state; already adapts `LineBuffer` for movement/deletion | Existing primitive sharing retained |
| Terminal events | `Reader` and standalone blocking driver; strict modifier matching | Completion-first routing, extra editing bindings, completeness-aware submission | Keep separate policies |
| Rendering/cursor positioning | One horizontally scrolled ASCII-escaped line | Anchored inline viewport, logical lines, continuation prompts, bounded rows, completion menu, resize/scroll handling, final submission echo | Keep IDE renderer |
| History | Unbounded in-memory entries; submissions record automatically | Bounded entries, rejects whitespace-only commands, caller-controlled recording, workspace-backed JSON persistence | Share navigation only |
| Completion | None | Provider results, validated replacement ranges, menu selection/cancellation, atomic size limits | Keep in IDE |
| Multiline | Single-line insertion flattens newlines and removes tabs | Logical-line movement, vertical navigation, newline insertion, completeness checks, 1 MiB atomic input limit | Keep in IDE |
| IDE integration | No language/workspace dependencies | Language prompts/completeness/completion, shell routing, persistence authority, buffered CLI branch | Keep in IDE |

Other concrete duplication remains: raw-mode ownership/restoration, bracketed-paste
setup/cleanup, event loops, printable-character insertion, and ASCII escaping.
Those implementations surround different rendering and output ownership contracts;
combining them is not necessary to share history and was deliberately deferred.

## Responsibilities now shared and duplicate code removed

`HistoryNavigation<D>` in `crates/scope-terminal/lib.rs` owns the recall position
and original draft. `previous` captures the first draft, recalls newest-to-oldest,
and stays at the oldest entry. `next` walks forward and returns the draft exactly
once. `reset` discards navigation state. `HistorySelection<D>` distinguishes a
recalled string from the caller's draft representation. Entries must stay unchanged
during navigation; callers reset before changing storage.

Both `Reader` and `CommandHistory` now delegate to this implementation. Their
duplicate index advancement, oldest-boundary handling, initial draft capture,
newest-boundary restoration, and reset logic were removed. `Reader` retains a
`LineBuffer` draft (including cursor); IDE retains a `String` draft. This avoids
imposing a new editor state model on the IDE. Existing public consumer APIs remain
available; the generic navigation API is additive.

## Behavior deliberately preserved / differences remaining

- Standalone restores the draft cursor; IDE places the restored draft cursor at
  its end, as before. Edits to recalled entries do not mutate stored history.
- Standalone Up/Down always navigate history. IDE first moves between logical
  lines, falling back to history at the boundary; Ctrl+P/N explicitly recall.
- IDE Home/End are logical-line-relative; Ctrl+A/E are buffer-relative.
  Standalone has a single line, so both pairs target the buffer boundaries.
- Standalone Escape or Ctrl+C returns cancellation. IDE Ctrl+C reprompts inside
  the read loop; an open completion menu consumes Ctrl+C to dismiss the menu.
- Both use empty Ctrl+D for EOF and nonempty Ctrl+D for deletion. IDE additionally
  owns Ctrl+J/S/K/U/W and language completeness rules. Modifier handling differs;
  no key mapping was unified.
- Standalone records nonempty submissions, including whitespace-only input, and
  has no persistence. IDE keeps capacity, blank/duplicate suppression, recording
  decisions, JSON bounds, and workspace read/write authority unchanged.
- Standalone redirected input uses `read_from` without prompts or sanitization.
  IDE's CLI selects its existing buffered branch when either stream is redirected;
  the private interactive editor itself rejects redirected streams. Neither branch
  was changed.

## Unicode and rendering limitations

Both edit Unicode scalars at UTF-8 byte boundaries, not grapheme clusters. Combining
sequences and emoji clusters can be split by movement/deletion. Both escape
non-ASCII for display rather than computing native glyph widths. IDE preserves
logical newlines and displays continuation rows; standalone paste flattens them.
ASCII escaping avoids wide-character cursor ambiguity but is not native Unicode
presentation. Clipping can expose partial escape representations. Both reserve
space to avoid terminal autowrap; neither is an asynchronous-output compositor.
Paste filtering remains chunk-local, not a streaming ANSI parser. Buffered source
is deliberately not sanitized.

## Validation performed

Passed in this checkout:

- `cargo test -p scope-terminal -p scope-ide` (default features, including doc tests).
- `cargo test -p scope-ide history::` (8 history/persistence tests).
- `cargo fmt --all -- --check` and `git diff --check`.
- Built and ran the standalone `minimal` executable with redirected stdin/stdout:
  exact Unicode output, CRLF trimming, unterminated final line, EOF, and absence of
  prompt/escape bytes asserted.
- Existing IDE `piped_shell_keeps_plain_input_and_does_not_write_history` regression
  ran as part of the full suite: buffered commands, plain output, no history write.
- New deterministic tests exercise empty history, oldest boundary, edited recalls,
  one-time Unicode/multiline draft restoration, reset, and exact `LineBuffer`
  cursor restoration. The IDE navigation test now uses a multiline Unicode draft.
  Existing completion, multiline, viewport, cancellation/EOF, history persistence,
  and redirected-input tests remain intact.

An automated Python PTY smoke check drove the standalone executable at 80x24 with
Left/Up arrows, Home/End, Ctrl+A/E, Backspace/Delete, bracketed Unicode/multiline
paste, Ctrl+C, and Ctrl+D. It asserted edited/returned text, paste enable/disable
sequences, successful exit, and exact restoration of the PTY's termios settings.
This exercised a kernel PTY, **not a visual terminal emulator or manual test**.

### Manual terminal testing performed

None. The agent has bounded command execution, not a human-operated visual terminal.
No automated test above is claimed as manual evidence. The following remain
manually unverified in both editors: arrow/cursor positioning, Home/End and Ctrl+A/E,
Backspace/Delete, Ctrl+C/EOF experience, history recall display, bracketed paste,
Unicode presentation, prompt redraw, and terminal restoration as observed in an
emulator. Scope IDE multiline navigation, completion menus, scrolling/resizing,
and restoration after an interactive rendering/event error also remain untested
in a real emulator. The standalone PTY smoke does not establish those IDE results.

Before/after equivalence was assessed from the removed transitions and unchanged
consumer regression assertions; no pre-change executable/visual A/B run was made.
Existing cleanup guards were not modified: they still unwind on normal I/O errors
and cancellation, restore raw mode only when they enabled it, and best-effort
disable bracketed paste. Hard termination and failed cleanup output remain outside
RAII guarantees. Optional feature combinations were not validated for T2.

## Recommended next integration target

Investigate a small reusable raw-mode ownership guard, independently of rendering
and bracketed-paste output ownership. First add PTY tests for pre-existing raw mode,
setup failure, output failure, and unwinding, plus manual IDE terminal checks. Do
not replace the multiline editor with `Reader`: that would require a broader event,
completion, rendering, and submission-policy redesign for no demonstrated benefit.
