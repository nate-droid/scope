# T7 — Rich completion results

## Decision: no candidate API change required

`scope-terminal::CompletionCandidate` already separates replacement/insertion
from an optional display-only `label`. `Reader::complete` validates ranges and
insertion text, applies a unique insertion or a shared insertion prefix, and
otherwise returns the labels in provider order as `CompletionAction::Display`.
Neither labels nor descriptions participate in matching or replacement. A caller
can therefore compose a name and optional description into a label without a new
field, structured semantic type, or source-breaking change to existing providers.

Scope IDE has a richer `scope_language::CompletionItem` with a separate optional
`detail`. Its inline viewport displays the selected candidate's detail alongside
its label, with other candidates available through selection. The synchronous
terminal reader has no selected-candidate menu: every ambiguous candidate gets a
separate line. A flattened display label is sufficient for this presentation.
A separate terminal detail field would become useful if independent styling,
layout, or selection were required; none is needed for this milestone.

## Downstream integration

The independent sibling checkout `scope-forth` uses the shared `Reader` within
its existing multiline-paste-preserving workbench driver. Its `src/shell.rs`
adapter now delegates conversion to `src/shell_terminal.rs::completion_candidate`.
That conversion preserves the dictionary result's range and insertion verbatim,
and composes `label -- detail` only for nonempty details. Missing metadata leaves
the plain name. Production and the reader regression test use the same converter.

Metadata comes exclusively from existing `dictionary_completions` results:

| Origin | Display meaning | Limitation |
| --- | --- | --- |
| Primitive | Static primitive stack effect | Not execution evidence |
| Control syntax | `compile-only control syntax` | Not a runtime effect |
| Annotated definition | `declared` plus retained annotation text | A declaration, not inferred or verified behavior |
| Host word | `host-declared` plus installed descriptor signature | Does not install or invoke a capability |
| Unannotated definition | No description | No additional inference is attempted |

The adapter forwards these strings without stripping their provenance qualifiers.
Discovery remains dictionary-based, whitespace-token-based, case-normalized for
ASCII names, and source-relative. It does not evaluate pending source, infer new
stack effects, parse comments/strings, register capabilities, or mutate semantic
state. Existing dictionary and metadata limitations are unchanged.

## Display safety and compatibility

Both drivers already escape labels with `char::escape_default`, render ASCII,
reserve the last column against autowrap, and display at most 100 candidates.
Unicode, combining characters, controls, escape sequences, and newlines become
literal escape text, so byte length equals display columns. Escapes can be clipped
mid-sequence; they remain inert printable ASCII. Long descriptions can be cut off
entirely after the name. No ellipsis, Unicode-width dependency, new keybindings,
registry, editor, runtime authority, or language-specific terminal logic is added.

The stock terminal renderer now takes the width limit during escaping instead
of allocating an entire escaped label before clipping. Provider allocation and
runtime remain the provider's responsibility. Forth's existing custom driver
retains its escaped-and-clipped rendering. `CompletionAction::Display` remains raw
display data: custom drivers must continue escaping and bounding it themselves.
All public completion types, providers, and insertion behavior remain compatible.

## Validation

- `cargo test -p scope-terminal`: 21 tests, including descriptions versus missing
  labels, common-prefix and unique replacement, ambiguous display with preserved
  cursor/input, invalid ranges, Unicode, control escaping, widths including zero,
  and million-character metadata truncation. Existing history, paste, cancellation,
  and EOF tests remain intact.
- `cargo build -p scope-terminal --example completion` and
  `python3 crates/scope-terminal/completion_pty.py target/debug/examples/completion`:
  real PTY coverage for described ambiguous candidates, escaped controls/Unicode,
  unique insertion, resize/narrow clipping, prompt and mid-line cursor restoration,
  history recall, bracketed paste, cancellation, EOF, and terminal-mode restoration.
  The PTY child owns its controlling terminal so resizing tests the actual PTY,
  rather than an inherited editor terminal. The master retains mode access after
  the child session exits on macOS.
- Forth `cargo test --features ide --lib shell::terminal::tests`: production adapter
  conversion exercised through `Reader`, declared and absent details, exact unique
  insertion, unchanged stack, and existing multiline paste/history behavior.
- Forth `cargo test --features ide --test interactive`: 13 tests including repeated
  completion of pending source containing definitions/output, unchanged dictionary
  behavior and stack, session isolation, and no newly installed host authority.

Forth tests use its own manifest with an in-workspace `CARGO_TARGET_DIR`; no Cell,
source evaluation, type system, or runtime authority changes are involved.
