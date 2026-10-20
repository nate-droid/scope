# Interactive terminal foundation (T1)

## Inspection and ownership

Scope already had a crossterm 0.28 editor in `crates/scope-ide/src/shell_input.rs`.
It supports Unicode-scalar cursor movement, insertion, deletion, Home/End,
Ctrl+A/E, cancellation, EOF, bounded multiline rendering, bracketed paste,
resize handling, history, completeness checks, and completion menus. Raw-mode
and display guards restore terminal state on return, I/O errors, and unwinding.
`cli.rs` already selects buffered input when stdin or stdout is redirected.
`buffer_editor.rs` is a separate buffered draft workflow; the optional ratatui
TUI has its own event loop. Neither needs replacement for this milestone.

The shell editor was private and depended on IDE session/completion types;
it was not an independent downstream input interface. The smallest useful
shared boundary is a UTF-8 line buffer plus an optional single-line terminal
driver, not the shell's language routing or its full multiline UI.

`scope-terminal` owns that boundary. Its only external dependency is the
already-used crossterm version. It has no Cell, IDE, language, parser, registry,
workspace, authorization, or proposal dependency. Scope uses its buffer operations
for Left/Right, Backspace/Delete, Ctrl+D and word-erasure movement. Its paste
sanitizer also protects Scope's existing multiline paste path. Scope retains
its existing rendering, history persistence, completion, and session policy.
No new completion behavior was introduced.

## Public integration interface

- `LineBuffer`: terminal-independent text/cursor state; cursor offsets are UTF-8
  bytes. `handle_key` maps crossterm key events into `Outcome`. Direct editing
  methods permit consumers to own their own key mapping.
- `Outcome`: `Continue`, `Submitted(String)`, `Cancelled`, or `Eof`. Submitted
  source text is separate from key events and terminal rendering.
- `Reader`: deterministic key handling with draft-preserving Up/Down navigation.
  It resets after submission, cancellation, or EOF and can be reused.
- `History`: simple in-memory history; interactive `Reader` submissions record
  automatically, suppressing empty input and consecutive duplicates.
- `read_line(prompt, &mut history)`: convenience driver using process stdin/stdout.
  It enters raw mode only if **both** are terminals; otherwise reads a buffered
  line with no prompt, terminal escapes, or history side effects.
- `read_from(&mut impl BufRead)`: deterministic buffered fallback, preserving
  input verbatim except a terminating LF and optional preceding CR. A final
  unterminated line is submitted; zero bytes means EOF.
- `sanitize_paste`: removes ANSI/terminal sequences and controls, preserves tabs
  and newlines, and normalizes CRLF/CR to LF for multiline consumers.

`LineBuffer::insert` sanitizes complete inserted text chunks, converts newlines
to spaces, and removes tabs for single-line use. `from_parts` is deliberately
verbatim for adapting existing editors; it asserts a valid UTF-8 cursor boundary.
Do not use it as a sanitization boundary.

## Supported bindings

| Key | Single-line behavior |
| --- | --- |
| Left / Right | Move one Unicode scalar, bounded at either end |
| Home / End; Ctrl+A / Ctrl+E | Beginning / end of input |
| Backspace / Delete | Remove scalar before / at cursor |
| Printable character | Insert at cursor |
| Enter | Submit input |
| Ctrl+C; Escape | Discard draft and return `Cancelled` |
| Ctrl+D | EOF on empty input; otherwise delete at cursor |
| Up / Down | Recall history / restore draft and its cursor |
| Tab | Ignored; no completion |

Release events and unsupported modifier combinations are ignored. Crossterm
decodes keyboard escape sequences into events rather than source text. Pasted
CSI, OSC, DCS and related escape sequences are stripped, including unfinished
sequences at the end of a paste. Scope retains its richer existing bindings:
Home/End are logical-line-relative, Ctrl+A/E are buffer-relative, Ctrl+P/N recall
history, and Ctrl+J, Ctrl+S, Ctrl+K/U/W keep their existing meanings. Scope's
Ctrl+C reprompts internally; the standalone API returns cancellation so the
consumer controls policy.

## Downstream adoption

Depend on the `scope-terminal` workspace package (or a path to its directory),
not `scope-ide`. See `crates/scope-terminal/examples/minimal.rs`:

```sh
cargo run -p scope-terminal --example minimal
```

The example echoes submissions, reprompts after cancellation, and exits on EOF.
A language environment should evaluate only `Submitted(text)` and keep its own
language semantics. Consumers with existing event loops should use `LineBuffer`
or `Reader`, not the blocking terminal driver. The standalone driver must be
the sole owner of terminal input while it is running.

## Cleanup and limitations

The terminal guard is constructed immediately after raw-mode entry, before
bracketed-paste setup. Drop disables bracketed paste and restores raw mode only
if this reader enabled it. Errors from event reads, terminal sizing, rendering,
and flushing propagate through this guard; unwinding also cleans up. Cleanup
is best-effort if stdout itself fails. Abort, SIGKILL, process termination without
unwinding, and terminal emulator failures cannot be repaired by a Rust destructor.
Bracketed-paste mode has no portable prior-state query, so nested terminal owners
are not supported even though a pre-existing raw-mode setting is preserved.

Rendering is intentionally ASCII-escaped, with horizontal scrolling and one
reserved terminal column to prevent autowrap. Unicode input is preserved, but
editing uses scalars rather than grapheme clusters; non-ASCII is not rendered
at native glyph width. ANSI-styled prompts are escaped, not interpreted. The
single-line reader does not support multiline editing or asynchronous output.
Escape filtering is chunk-local for complete paste payloads, not a streaming
ANSI parser for arbitrarily fragmented caller inserts. Buffered input is not
sanitized: scripts must retain their original bytes/text semantics.

History in the standalone convenience reader is unbounded and in-memory only;
long-lived consumers should impose retention policy or use their existing
history model with `LineBuffer`. Scope's bounded history, duplicate rules,
persistence authority, and draft behavior are unchanged. Persistence belongs
to the consumer, not this crate. Future completion should be an explicit optional
consumer-supplied interface with validated replacement ranges, not a dependency
on IDE command parsing or language syntax.

## Validation

Deterministic tests cover beginning/middle/end insertion, UTF-8 boundaries,
movement/deletion at boundaries, Home/End and Ctrl+A/E, cancellation and reuse,
terminal sequence stripping, history draft restoration, bounded safe rendering,
buffered CRLF/EOF behavior, invalid UTF-8, and I/O error propagation. Scope's
existing shell/history tests remain in place. Real terminal rendering and
signal/terminal-emulator behavior still warrant manual platform testing.
