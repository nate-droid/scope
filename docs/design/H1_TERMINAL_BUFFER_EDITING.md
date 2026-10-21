# H1 — Terminal-friendly scratch buffer editing

## Human workflow

In a Scope shell whose downstream host registers the Path interactive provider, with the lesson file present in the workspace:

```text
:scope session language.path
:scope open learning/lesson_01_reflexivity/exercise.path
:scope show
:scope line 3 "  refl x"
:scope show
:scope buffer eval
```

The lesson must have the intended proof on line 3; inspect it first. Scope itself does not ship Path or this lesson. Use the language ID registered by your host if different. `:scope session` lists providers. Evaluation uses the existing matching active session and its normal language facilities; the edit neither evaluates nor switches languages.

A runnable language-independent editing example in any Scope shell:

```text
:scope scratch new text
:scope line 1 "  example"
:scope show
:scope line 1 ""
:scope show
:scope discard
```

`text` here is just the buffer's language label; no evaluator is required for editing.

## Command contract

`:scope line N JSON_STRING` replaces one complete line in the explicitly selected shell scratch buffer. `N` is one-based. The replacement is exactly one JSON string, not shell quoting: spaces inside quotes are preserved; `\t`, `\"`, and `\\` represent tabs, quotes, and backslashes. `""` leaves an empty line. Decoded CR or LF is rejected; this command cannot split or join lines. Invalid numbers, missing lines, malformed strings, and extra arguments fail without editing.

Lines are separated by LF. An existing LF or CRLF terminator is preserved byte-for-byte, as is every byte outside the replaced line. A bare CR is line content, not a separator. A trailing LF includes a final empty line: replacing line 2 of `a\n` with `"b"` produces `a\nb`; replacing line 1 retains its terminator. Empty source has one empty line. An unterminated last line remains unterminated. Indentation is supplied explicitly in the JSON string; there is no automatic indentation.

The implementation translates the selected line's byte span through the existing `TextBuffer` range API and calls `ScratchBuffer::apply_edit`. Every successful edit, even identical replacement text, advances the existing version once and marks the buffer dirty. Buffer source identity and destination baseline remain unchanged. There is no implicit save. Save separately with `:scope save` if desired.

## Ownership and lifecycle

Bare `open`/`show` retain their legacy App-document meaning outside language routing. Explicit `:scope open`/`show`/`line` use shell scratch buffers. The App document is not edited even when it has the same path as the selected buffer. Selection, evaluation attribution/revisions, checked persistence, and proposal staleness continue to use existing mechanisms. No new source store, synchronization, language syntax, or proposal lifecycle is introduced.

No exact-text replacement convenience is added. Existing range replacement remains available; this change does not guess which repeated hole a user intends.

## Cooked transactional editor safety

This section describes the legacy cooked path, now explicitly selected with
`:scope edit --commands` (and still used by non-TTY `:scope edit`). TTY
`:scope edit` instead uses the [raw multiline editor](../scratch-editing.md).
The cooked path remains a separate, command-driven transactional draft, not a terminal navigation editor. Input lines containing ESC or C1 controls are rejected in full before insertion or command execution, with a diagnostic that does not echo the unsafe input. This includes common arrow, Home/End, and function-key sequences. Other text on that input line is also discarded. Use `:at LINE COLUMN` to move within this editor. `:done` commits once; `:cancel` and EOF discard the draft. Previously existing escape bytes in source are not rewritten.

The ordinary multiline terminal input editor, history, and completion are unchanged, except that explicit `:scope line` is offered as a command completion. Cooked editor arrow navigation remains unsupported; the safety guard prevents source corruption rather than implementing another terminal editor.

## Validation

Regression tests cover line replacement, Unicode and indentation, empty replacements, LF/CRLF and unterminated/final-empty lines, invalid-input non-mutation, version/dirty tracking, explicit routing and selection, same-path App/buffer/disk separation, and evaluation of edited unsaved source. Cooked-editor tests cover escape rejection before literal insertion and command dispatch, as well as commit/cancel/EOF rollback.

Relevant suites: `scope-ide` library tests (default, no-default, and agent features), `scope-terminal` tests, and `scope-script` terminal PTY tests. The script PTY fixture exercises the script REPL, not the Scope cooked editor; cooked-editor safety is tested directly with byte-stream inputs rather than claiming PTY coverage of that path.
