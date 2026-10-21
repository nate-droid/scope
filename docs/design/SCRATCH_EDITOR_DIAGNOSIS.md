# Scratch editor: diagnosis and source ownership

## Old behavior and the apparent accumulation bug

The legacy `:scope edit` path was a cooked, line-oriented insertion editor. It
copied the selected buffer into a draft and placed the cursor at the beginning.
Reentering a corrected program inserted it before the old source; it did not
replace the source. Accepting retained both versions. Repeating this workflow
could accumulate declarations, invalid fragments, and their diagnostics.

That failure mode is source duplication, not evidence that the parser retained
old diagnostics or interpreter state. Inspect the accepted source with
`:scope show` (or the command editor's `:show`) and its revision before changing
the parser. Pure `:scope script run` uses a fresh interpreter on each run; it
still sees all duplicated text in the selected source snapshot. A registered
interactive language session may separately retain evaluation state; do not
confuse that session model with fresh Script runs.

A minimal reproduction in command mode is:

```text
:scope scratch new script
:scope edit --commands
let n = 6;
n
:done
:scope edit --commands
let n = 8;
n
:show
:done
```

The source contains both programs, with the new one before the old one. Each
repetition shifts old errors downward by the inserted line count. The regression
`cooked_reentry_inserts_instead_of_replacing_existing_source` locks down this
insertion behavior; the keyboard editor avoids requiring whole-program reentry.
The original reported session transcript is unavailable, so this reproduces the
accumulation mechanism rather than identifying every input from that session.

Before this change, command coordinates and `String` insertion/deletion were
implemented directly in `cli/buffer_editor.rs`; only the final whole-document
replacement used `TextBuffer`. They now use `TextBuffer` coordinate conversion
and replacement operations during drafting as well. `scope-terminal` holds only
cursor/viewport state and issues byte-range replacement requests through the
caller-owned `DocumentEditor`; it neither owns accepted text nor creates source
revisions. Commit checks the original scratch ID, version, and text before
applying a changed draft. A stale target fails instead of overwriting it.

## Editing contract

On a TTY, `:scope edit` now selects a raw multiline keyboard editor with arrows,
Home/End, Enter, Backspace/Delete, Tab, and bracketed paste. Ctrl-S accepts;
Ctrl-C/Escape cancel. It edits existing source rather than requiring users to
reenter whole lines. `:scope edit --commands` retains explicit cooked
`:at`, `:insert`, `:delete`, `:show`, `:done`, `:cancel`, and `:complete` operations.
Non-TTY `:scope edit` remains command-driven. Colon commands are not interpreted
as actions in the raw editor.

Both frontends operate on a private draft. Accepting changed text commits exactly
one revision; accepting identical text commits none. Cancellation leaves the
original source, revision, and dirty state unchanged. Editing never implicitly
saves or evaluates. Explicit coordinates are one-based Unicode scalars, while
`TextBuffer` uses UTF-8 byte offsets internally. Neither is a grapheme index or
terminal display-cell column.

## Source and execution model

```text
keyboard / explicit commands
    -> TextBuffer draft (UTF-8 byte offsets)
    -> accepted ScratchBuffer source + revision
    -> explicit evaluate/run of a source snapshot
```

`TextBuffer` owns text editing mechanics, not provider authority or evaluation.
`ScratchBuffer` owns the shell buffer's accepted source identity, revision and
persistence state. A shell buffer is also separate from an App-owned open
document; selection in one does not silently select the other.

Cell is a separate WebAssembly execution and capability-enforcement layer, not
the scratch text store, terminal editor, or parser fix. Improving keyboard input
does not change Cell grants, install a semantic provider, or authorize workspace
writes. Agent proposal authorization is unchanged: editing does not accept a
proposal, bypass baseline revalidation, grant model/tool authority, or turn an
in-memory proposal acceptance into save/evaluate.

## Deferred scope

Syntax highlighting, autocomplete UI, full-screen/modal editing, language-aware
refactoring, new agent planning or Script features, and filesystem persistence
changes are explicitly out of scope.

This integration is not an IDE replacement. Grapheme-aware/display-width motion,
custom keymaps, undo/redo, richer selection/search, raw-editor completion UI,
large-document storage optimization, persistent draft/crash recovery, and shared
App/shell source ownership remain deferred. Existing command-mode completion is
metadata-only and remains available explicitly; no language/runtime or agent
protocol redesign is implied.

See the [short tutorial](../scratch-editing.md),
[session reference](../../INTERACTIVE_SESSIONS.md#workspace-editing), and
[explicit source-context design](T2A_EXPLICIT_SOURCE_CONTEXT.md).
