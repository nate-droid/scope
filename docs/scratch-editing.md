# Edit, rerun, reopen

Start `cargo run` in a terminal. At the shell prompt, enter
`:scope scratch new script`, then `:scope edit`.

1. Type `let n = 6;`, press Enter, then type `n * 7`.
2. Press Ctrl-S to accept the draft. This changes memory, not disk.
3. Run `:scope script run`; the final value is `42`.
4. Reopen `:scope edit`. Existing source is still there: navigate to the `6`
   with arrows/Home/End, delete it with Delete or Backspace, and type `8`.
   Do not retype the whole program over the existing text.
5. Press Ctrl-S and run again; the final value is `56`.
6. Reopen, make a change, then press Ctrl-C or Escape. Run again: the accepted
   source and its revision are unchanged.

The editor uses an inline viewport of up to 12 terminal rows, scrolling to follow
its cursor. Printable ASCII is shown literally; Unicode and tabs are displayed as
safe ASCII escapes so cursor placement does not depend on terminal character
widths. The underlying source is unchanged, and navigation/deletion still uses
Unicode scalars, not the individual characters of the displayed escape.

Enter inserts a newline; Tab inserts a tab. Bracketed multiline paste inserts
source, including colon-leading text, without executing shell or editor commands.
Ctrl-S accepts, not saves or evaluates. An accepted changed draft creates one
revision regardless of keystroke count; an unchanged draft creates none.

## Explicit command mode

Use `:scope edit --commands` for the legacy Enter-submitted editor. Non-TTY
`:scope edit` selects this mode automatically. For example, in an empty buffer:

```text
:scope edit --commands
let n = 6;
n * 7
:done
```

To revise it, reopen with `:scope edit --commands`, then enter:

```text
:at 1 9
:delete 1
:insert 8
:show
:done
```

`:at LINE COLUMN` uses **one-based Unicode scalar** positions, not UTF-8 bytes,
grapheme clusters, or terminal display cells. `:delete COUNT` deletes scalars
forward, including newlines. `:insert` supports `\n`, `\t`, and `\\` escapes.
`:show [START]` previews source; `:complete` lists metadata-only completions and
`:complete INDEX` applies one. `:cancel` or EOF discards the command-mode draft.
These commands are not keybindings in the raw editor.

Command mode starts at the beginning and **inserts**, rather than replacing the
old program when you reenter source. Inspect with `:show` before revising.
See the [architecture diagnosis](design/SCRATCH_EDITOR_DIAGNOSIS.md) for why
repeated diagnostics can result from duplicated source, not parser state.

Editing does not grant save authority, run providers, or accept agent proposals.
Use explicit save and evaluation commands separately. See
[Interactive Sessions](../INTERACTIVE_SESSIONS.md#workspace-editing) for details.
