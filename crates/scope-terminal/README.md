# scope-terminal

Minimal synchronous single-line input, UTF-8 byte cursors, reusable history and
editing primitives. Interactive input requires both stdin and stdout to be TTYs;
otherwise input is prompt-free and verbatim apart from line termination.

## Opt-in normalized-layout paste

Existing `read_line` and `read_line_with_completion` calls retain single-line
paste: newlines become spaces and tabs are removed. To preserve pasted layout:

```rust,no_run
use scope_terminal::{History, PastePolicy, read_line_with_paste_policy};

let mut history = History::new();
let outcome = read_line_with_paste_policy(
    "> ", &mut history, PastePolicy::NormalizedLayout,
)?;
# Ok::<(), std::io::Error>(())
```

With completion, use
`read_line_with_completion_and_paste_policy(prompt, history, provider, policy)`.
Both entry points share the existing event loop and terminal lifecycle.
`PastePolicy::SingleLine` is the default; `NormalizedLayout` inserts **exactly**
the output of `sanitize_paste`: LF, tabs, blank lines, surrounding whitespace and
Unicode survive, CRLF/lone CR normalize to LF, and terminal sequences/other
controls are removed. This is normalized text preservation, **not raw-byte
fidelity**. Filtering is chunk-local and unfinished escape sequences consume the
remainder of the payload.

Pasted LF/tab are data, never Enter/Tab events. Real Enter still submits the whole
buffer; keyboard Tab still requests completion (or does nothing without a
provider). Completion insertions still reject LF/tab. Up/Down still navigate
history and Home/End still target the whole buffer. The same single-line escaped
viewport displays `\n`/`\t`; this is **not full multiline editing**, logical-line
movement, or completeness-aware submission. No language semantics are added.
Redirected input ignores the paste policy and remains one LF-framed record per
call, with no prompt, provider call or history mutation.

Custom event drivers can use `Reader::handle_paste(text, policy)`, which always
returns `Outcome::Continue` without recording history or resetting navigation,
or `LineBuffer::insert_paste(text, policy)` for insertion alone. Avoid passing
`sanitize_paste` output through ordinary `insert`, which flattens it again.
Guarantees apply to decoded paste events, not unbracketed input arriving as keys.

## Optional completion

Use `read_line_with_completion(prompt, history, provider)` instead of `read_line`.
A `CompletionProvider` (also implemented for `FnMut` closures) receives the full
input and cursor as a UTF-8 **byte** offset and returns `CompletionCandidate`s.
Each candidate specifies a half-open byte `replacement` range, `insertion` text,
and optional `label`. Ranges must be ordered, within the input, and on UTF-8
scalar boundaries. Invalid candidates and unsafe insertion text are ignored.
Insertion must contain no controls, tabs, newlines, or terminal escape sequences;
labels are escaped for display. No spaces are automatically appended.

Descriptions need no separate terminal API: compose the optional description into
`label`, leaving `replacement` and `insertion` unchanged. For example, a downstream
adapter can use `Some(format!("{} -- {detail}", item.label))` when a detail exists,
and `Some(item.label)` otherwise. Preserve provenance qualifiers such as
`declared` or `host-declared`; displaying a description does not verify it.
Labels never affect candidate validation, common-prefix matching, or insertion.
The terminal treats the entire label as untrusted display-only text, never code.

Tab applies a unique candidate. Multiple candidates extend a common prefix only
when all replace the same range ending at the cursor and extend the existing
range text. Otherwise their labels print in provider order, one per line, clipped
to terminal width minus one column (maximum 100 labels). Escaping is streamed
up to that width, so even long metadata does not allocate a full escaped copy.
The last column is reserved to avoid autowrap. Another Tab discovers fresh candidates;
there is no selection menu, cycling, ranking, asynchronous work, or cached state.
No matches leave input unchanged. `Reader::complete` exposes the same mechanics
without terminal I/O, returning `CompletionAction` for custom drivers.

Discovery, filtering, ordering, tokenization, quoting, catalog access, and
permissions remain downstream. Providers run synchronously and should be fast,
read-only and bounded; the reader cannot bound provider runtime or allocation.
Redirected input never invokes a provider. Existing `read_line` callers retain
Tab-as-no-op behavior.

Editing uses Unicode scalars rather than grapheme clusters. Rendering escapes
non-ASCII text rather than calculating display width. Candidate insertion is
exact after validation, and the cursor lands immediately after inserted bytes.
`LineBuffer::replace` provides the same checked atomic replacement primitive.

## Example and validation

```sh
cargo run -p scope-terminal --example completion
cargo run -p scope-terminal --example completion -- --normalized-layout
cargo test -p scope-terminal
cargo build -p scope-terminal --example completion
python3 crates/scope-terminal/completion_pty.py target/debug/examples/completion
```

Try `he<Tab>` (extends to `hel`), then Tab (lists `help` and `hello`), or
`ha<Tab>` (inserts `halt`). The example only echoes submissions, not commands.
The example includes descriptions and an intentionally unsafe description to
show escaping. The PTY smoke test requires Unix and Python 3's standard library;
it covers described ambiguous results, unique insertion, control/Unicode escaping,
narrow-width truncation, mid-line cursor and prompt restoration, history, paste,
cancellation, EOF, and terminal-mode restoration. It runs both paste policies,
asserting exact submitted rich-paste text after a single Enter and no submission
before Enter, plus unchanged redirected line framing. It is a Unix PTY smoke test,
not a visual terminal emulator test.
