# T1 — Terminal input integration audit

## Outcome

Audited and exercised on macOS, 2026-10-22, using local default-feature builds and Python standard-library PTYs. The reported absence of arrows/Tab is **not a general failure of scope-terminal**. The application has different input paths, and terminal detection and host provider wiring matter.

One reproduced defect was fixed: **Ctrl+C with the IDE completion menu open dismissed the menu but retained the draft**. The next command was appended to that draft. No terminal framework, shared abstraction, evaluation behavior, or authorization policy was changed.

The cooked source editor intentionally has textual editing commands, not raw arrow navigation or completion. The Script scratch-runner example uses injected buffered I/O even when launched in a PTY; it is not the standalone Script terminal REPL.

## 1. Input-routing architecture

### Shared primitives

`crates/scope-terminal/lib.rs` supplies:

- `LineBuffer`: UTF-8 byte cursor on Unicode scalar boundaries, insertion/deletion and horizontal movement.
- `History` and `HistoryNavigation`: command recall and draft restoration.
- `Reader`: single-line key policy plus caller-owned synchronous `CompletionProvider`.
- `read_line_with_completion_and_paste_policy`: raw-mode lifecycle, crossterm `event::read`, key/paste dispatch, escaped bounded single-line rendering, completion label output.
- `sanitize_paste`: normalized layout sanitizer for multiline consumers.
- `read_from`: buffered fallback, physical-line framing, no prompts, editing, sanitization, completion, or history side effects.

Both stdin **and stdout** must be terminals for the shared terminal reader. Redirecting either disables interactive behavior. Raw-mode guards restore modes they enabled and disable bracketed paste on return/error. Decoding belongs to crossterm, not a hand-written escape parser in each host.

### Scope IDE main shell and multiline input

```text
scope-ide --workspace PATH shell
  cli::run_configured (stdin && stdout TTY?)
    run_shell_configured / run_shell_owned
      interactive: shell_input::read_line
        crossterm events -> Input -> InlineViewport
        LineBuffer edits + CommandHistory / HistoryNavigation
        InteractiveSession::complete
          Scope command context -> cli::complete_commands
          otherwise -> active language session.complete
        InteractiveSession completeness -> whole-buffer submission policy
      noninteractive: supplied BufRead::read_line
      submitted source -> existing command/language dispatch
```

Implementation: `crates/scope-ide/src/cli.rs`, `src/shell_input.rs`, `src/history.rs`, and `src/language/interactive.rs`.

The IDE does **not** call the shared single-line reader. `shell_input::Input` owns multiline text, cursor, status, completion choices and selected choice. Its custom bounded inline viewport and completeness policy are necessary differences, not evidence that all shared infrastructure was bypassed. It reuses shared horizontal editing, paste sanitization, and history-navigation primitives. Raw/display lifecycle guards are host-specific duplicates of lifecycle responsibilities, not a second shared framework.

Tab asks the session for candidates. Scope command context takes precedence, including an empty command result; it must not accidentally fall through to a language. Active-language completion is only available if a provider/session was registered. A scratch buffer's language label alone does not attach a live completion provider. Completion edit ranges are checked at UTF-8 boundaries. The IDE retains `CompletionItem` insertion/range/label/detail metadata; the viewport renders bounded candidate information. A unique match is applied immediately. Multiple matches open a selectable menu; Tab/Down and Shift+Tab/Up select, Enter accepts **without submitting**, Escape dismisses.

`CommandHistory` owns bounded host history and optional capability-backed persistence. The caller decides which submissions to record. Up/Down first move between source rows; at a boundary they recall history. Ctrl+P/Ctrl+N explicitly recall history regardless of row. Home/End are row-local; Ctrl+A/Ctrl+E address the whole buffer. Ctrl+J inserts a newline. Enter submits complete source, adds a newline for incomplete source, or reports invalid source; Ctrl+S can force invalid (not incomplete) source submission.

### Standalone Scope Script

```text
scope-script (no file argument)
  main.rs: stdin && stdout TTY?
    yes: repl::run_terminal
      ScriptCompletionProvider over current Interpreter + Host
      scope_terminal::read_line_with_completion
    no: repl::run over bounded buffered physical lines
  retained Interpreter evaluates each submitted source
```

Paths: `crates/scope-script/src/{main,repl,completion}.rs`.

The host is correctly wired: completion includes Script keywords/builtins, current bindings, and literal `scope.invoke` provider/action discovery from `Host::operation_names`. Standalone `NoHost` has no semantic-operation candidates. Ordinary strings, dynamic provider expressions and inferred object fields do not acquire invented candidates. Completion does not evaluate source or invoke operations.

`Reader` owns line-editing state; one in-memory `History` lives for the REPL process. Unlike the IDE menu, ambiguous results extend a common prefix if possible, otherwise print escaped/clipped labels (up to 100), then redraw the draft. No implicit trailing space is inserted. There is no completeness-based multiline accumulator. A file argument follows file evaluation, not an input editor.

### Script execution from IDE source buffers

`crates/scope-script/examples/scratch_runner.rs` installs the adapter in `src/runner.rs` through IDE `run_with_session_and_command` / `ReadOnlyCommand::invoke_source`:

```text
:scope scratch new scope-script -> :scope edit -> :done
:scope run OPERATION --selection LINE:BYTE..LINE:BYTE
  selected scratch source + separate open document target
  -> ScriptRunner::run -> fresh Interpreter + constrained SemanticHost
```

Selection is target-relative, zero-based, half-open UTF-8 bytes. Unsaved source is executable; neither editor commit nor run implicitly saves it. The explicit source callback does not register Script as an interactive language session. Each run has fresh bindings. Existing default-deny authorization and provenance/record behavior remain unchanged.

Crucially, `run_with_session_and_command` uses **interactive=false**: input is the injected `BufRead`, including when the example's descriptors are TTYs. Prompts are printed, but shared key acquisition, completion and interactive history are not enabled. This API is a deterministic buffered integration harness, not a terminal Script workbench. Do not infer missing library functionality from arrow bytes received here.

### Cooked scratch/source editor

`crates/scope-ide/src/cli/buffer_editor.rs::run` temporarily reads from the shell's buffered input, after the shell raw guard has returned. It owns a transactional draft and insertion cursor, separate from interactive shell input.

Each physical line is either a textual editor command or literal source plus newline. `:at`, `:insert`, `:delete`, `:show`, `:done`, `:cancel` and `:help` are the interface. Coordinates are one-based Unicode scalars. `:insert` decodes `\\n`, `\\t`, and `\\\\`; literal colons are supported through it. Escape/C1 terminal-control sequences reject the whole line before insertion or dispatch. EOF discards the draft. This is intentionally not a full-screen editor, and it has no raw completion/history/paste reader.

### Downstream Scope Forth reference

Inspected the sibling checkout `../scope-forth` (not a workspace member):

- `src/bin/scope-forth-shell.rs` uses generic IDE `run_with_registry`; `src/interactive.rs` registers the live Forth session. Scope command routing and the IDE multiline/menu editor apply.
- `src/bin/forth-scope.rs` calls private `src/shell.rs::run_terminal`; `src/shell_terminal.rs` adapts shared editing/history/completion primitives for the workbench. `forth repl` enters `forth-repl>`, unfinished definitions continue at `....>`, and `:scope` returns to the workbench.
- Both completion paths use the live interpreter dictionary, whole whitespace-delimited token replacement, and stack-effect/signature metadata. Unevaluated scratch definitions are not dictionary entries.
- The private workbench preserves normalized pasted tabs/newlines, displays layout escaped in a single-line viewport, and evaluates only after Enter. Newlines preserve Forth backslash-comment boundaries. Its pending-definition accumulator and 64 KiB limit are host-owned. Raw mode ends before evaluation. Redirected workbench input is buffered **with prompts**.

## 2. Input-mode contracts and behavior matrix

“Pass” below means observable PTY behavior was checked, not merely a key-handler return value. “Policy” identifies an intentional distinction; validation boundaries follow the table.

| Feature | IDE command / multiline shell | Standalone Script | Cooked source editor | Private Forth REPL |
|---|---|---|---|---|
| Left/Right, insertion/deletion | Pass; shared scalar edits | Pass | Policy: `:at`, `:insert`, `:delete`; raw escapes rejected | Shared scalar editor; cursor replacement checked |
| Home/End | Pass; current row | Pass; entire line | Policy: textual cursor commands | Shared line-buffer policy; not separately PTY asserted |
| Up/Down | Pass; rows first, history at edges | Pass; history and draft cursor restoration | No application history; `:at` for navigation | Pass; history, including multiline entries/draft |
| Tab | Pass; unique insertion / selectable menu | Pass; unique/common-prefix/list | Literal tab data, not completion | Pass; live dictionary |
| Ambiguous metadata | Menu selection; bounded labels/details | List output; no menu | Not supported | Pass; stack-effect descriptions |
| Ctrl+C | Pass; cancel draft, including open menu (fixed) | Pass; cancel without evaluation | OS cooked-mode signal, not transactional editor cancellation; use `:cancel` | Pass; discard pending definition |
| Ctrl+D | Pass; delete under cursor, EOF on empty | Pass; same distinction | Buffered EOF discards draft | Pass; EOF reports pending discard |
| Bracketed paste | Pass; insert-only, normalized layout | Pass; single-line policy | No bracketed-paste acquisition; use commands for controlled insertion | Pass; insert-only, preserves comment newlines |
| Multiline | Pass; Ctrl+J/paste, vertical movement | No continuation editor; each submission evaluates | Pass; textual commands, transactional commit | Pass; continuation and pasted definitions |
| Non-TTY | Buffered physical lines, no raw keys | Prompt-free bounded physical lines | Same buffered editor protocol | Buffered lines, prompts remain |

**Command contract:** Tab requests host completion; Left/Right edit; Up/Down recall commands when not navigating multiline text or a completion menu. Paste is insertion-only, not keystrokes. Standalone Script's established command policy flattens newlines to spaces and removes tabs; IDE shell paste preserves layout. These are existing, explicit differences.

**Multiline source contract:** IDE arrows move text where a neighboring row exists, Home/End are row-local, explicit Ctrl+P/N access history, and Tab requests completion rather than indentation. Pasted tabs/newlines are preserved (CR/CRLF normalize); keyboard Tab is not literal indentation. There is no automatic indentation engine. Whitespace is not flattened before language evaluation. Forth's private REPL instead uses a single-line escaped viewport plus host continuation; it is not the IDE's row editor.

**Cooked source contract:** editor commands remain the supported interface. Literal lines retain spaces/tabs and gain LF. Source is committed only with `:done`; `:cancel` or EOF discards. No claim is made about full-screen movement, interactive completion, application history, Ctrl+C transaction handling, or safe arbitrary bracketed multiline paste. In particular, pasted physical lines beginning with editor commands can dispatch commands; use `:insert` escapes for controlled source insertion.

## 3. Reproduction and validation

Run from the `scope` root with cached dependencies and Python 3. PTYs attach stdin/stdout/stderr to the slave; a pipe for stdout would test fallback instead. The IDE harness answers crossterm cursor-position queries, sets terminal dimensions, and uses an isolated temporary workspace. Assertions inspect submitted-command effects and output, not just key-handler outcomes.

```sh
cargo build --locked --offline -p scope-ide --bin scope-ide
python3 crates/scope-ide/tests/pty_shell.py
cargo test --locked --offline -p scope-ide --lib
cargo test --locked --offline -p scope-terminal -p scope-script
cargo build --locked --offline -p scope-terminal --example completion
python3 crates/scope-terminal/completion_pty.py target/debug/examples/completion
cargo fmt --all -- --check
git --no-pager diff --check
```

Results:

- IDE PTY: **7 passed**, covering cursor edits, history/draft restoration, unique/ambiguous completion, menu cancellation, delete/EOF, Ctrl+J, bracketed multiline paste and actual vertical editing, cooked commands/commit/cancel/control rejection.
- IDE library: **341 passed**, including **16** shell-input tests. Existing unit tests cover provider completeness, routing whole buffers without evaluation, explicit multiline history, Unicode boundaries, limits, viewport bounds and paste.
- `scope-terminal`: **23 unit tests passed**; completion PTY passed both default and `--normalized-layout` policies, including actual submitted text, metadata escaping/clipping, narrow widths, history, cancellation, paste controls/layout and terminal restoration. Redirected input framing was also checked.
- Script tests passed in the combined package run; terminal PTY rerun after adding paste coverage: **8 passed**. Editing checks evaluate the resulting arithmetic, completion checks current bindings and ambiguous names, cancellation verifies no partial evaluation, and EOF checks deletion versus exit. New paste coverage verifies no evaluation before Enter and exactly one result (`42`) after CRLF/tab/control sanitization.
- Script scratch-runner integration: **7 passed**, including unsaved multiline source execution, fresh bindings, target selection/provenance and denied authorization. This is buffered integration coverage, not a claim of interactive Script completion in the example.
- Formatting and diff whitespace checks passed. Cargo emitted cached Cell duplicate-package warnings; these did not prevent builds/tests.

### Exact regression reproduction

In the main IDE shell, type `s`, Tab (open ambiguous command menu), Ctrl+C, then `help`, Enter.

- Before: Ctrl+C only cleared choices. Submitted text became `shelp`; output was `error: unknown command: shelp`.
- After: output includes `^C`, draft/history navigation reset, and `help` prints `Commands:`. No accidental prefix survives.

`crates/scope-ide/tests/pty_shell.py::test_cancel_completion_draft` runs this actual binary scenario. The new `shell_input` unit regression additionally checks cursor/status/choices/history reset and subsequent submission.

### Forth downstream validation

No downstream source was changed. Build outputs were kept inside this workspace:

```sh
cargo test --offline --locked --manifest-path ../scope-forth/Cargo.toml --target-dir target/forth-audit --features ide --lib shell -- --nocapture
cargo test --offline --locked --manifest-path ../scope-forth/Cargo.toml --target-dir target/forth-audit --features ide --test interactive -- --nocapture
cargo test --offline --locked --manifest-path ../scope-forth/Cargo.toml --target-dir target/forth-audit --features ide --test ide repl_ -- --nocapture
cargo build --offline --locked --manifest-path ../scope-forth/Cargo.toml --target-dir target/forth-audit --features ide --bins
```

Respectively **9**, **14**, and **8** focused tests passed; both downstream binaries built. The existing `../scope-forth/tests/completion_pty.py` was executed with its hardcoded binary assignment replaced **in memory only** by `target/forth-audit/debug/forth-scope`. It exited 0 and reported:

```text
PTY: builtin/live completion, metadata ambiguity, cursor replacement, continuation, comment-preserving paste, history draft, cancellation and EOF passed
```

The actual workbench evaluated completed `dup` input to `82`, and a pasted multiline definition containing a backslash comment produced `404`. Tabs and CR normalization have complementary unit coverage. Results apply to the current sibling working tree, which already contained unrelated local changes; those were untouched.

### Validation boundaries

These are PTY byte-stream/output tests, not terminal-emulator pixel/screen conformance tests. IDE checks strip ANSI sequences and do not validate every cursor cell or termios on teardown; shared/Script PTY drivers compare termios on normal exit. Forced termination cleanup is not established. The generic `scope-forth-shell` adapter was built and unit-tested but not separately PTY-tested. Script source execution was tested through its existing buffered integration tests, not misrepresented as raw terminal input. No agent/network features were exercised.

## 4. Diagnosis and minimal fix

| Observation | Owner / root cause | Resolution |
|---|---|---|
| Ctrl+C leaves input prefix with menu open | IDE `Input::completion_key` consumed Ctrl+C before cancellation routing | Removed the special interception; default menu dismissal now forwards Ctrl+C to the existing cancellation handler. Escape remains menu-only. |
| Arrows/Tab unavailable in cooked editor | Independent intentional buffered command editor, not a shared-reader regression | Document textual controls; do not replace it or feed escape bytes into source. |
| Arrows/Tab unavailable in Script scratch-runner example | Injected buffered CLI entry point explicitly disables interactive acquisition | Document the boundary; do not change deterministic host API semantics incidentally. |
| No semantic operation candidates in standalone Script | Correct provider attached, but `NoHost` has no operation catalog | Not an input-library bug; real hosts must supply discovery metadata. |
| No Script completion just from selecting a Script scratch buffer | Source-runner callback is not a registered interactive session | SS2 needs an explicit host/session adapter, not a new completion engine. |
| Up/Down sometimes move source instead of history | IDE multiline row policy, or active completion menu | Intentional; Ctrl+P/N provide explicit history access. |
| Paste differs between Script and Forth | Single-line versus normalized-layout host policy | Preserve both; flattening Forth source would change comment semantics. |

Source changes are limited to IDE Ctrl+C routing and its unit regression. Added the IDE actual-binary PTY harness and a standalone Script paste PTY regression. No evaluation, authorization, persistent history format, Forth source or shared terminal API changed.

## 5. Remaining limitations and SS2 recommendations

1. Reuse `scope-terminal` for a standalone command REPL; attach `ScriptCompletionProvider` to the current interpreter and actual host catalog, as the existing terminal REPL already does.
2. For IDE multiline authoring, register an explicit Script `InteractiveSessionProvider` / session adapter and reuse `shell_input`'s whole-buffer completeness/completion route. A language label or `ReadOnlyCommand` source callback alone is insufficient. Keep inspection/completion read-only and authorization separate.
3. Do not silently turn the injected buffered shell API into a real-terminal reader. A future interactive Script workbench entry point needs an explicit terminal lifecycle and input-policy decision; keep deterministic stream harnesses available.
4. Specify whether SS2 is a command REPL, multiline source submission, or transactional source editing. Reuse the corresponding existing path instead of assuming keyboard parity. If pasted Script becomes multiline source, choose normalized layout explicitly and add source-text assertions before evaluation; do not change the established standalone command policy accidentally.
5. Preserve Forth tabs/newlines and live dictionary metadata. Do not derive runtime completions by executing drafts. Keep replacement ranges and descriptive labels distinct.
6. Keep cooked `:at`/`:insert`/`:delete`/`:show`/`:done`/`:cancel` functional. A richer editor is a separate design decision, not justified by this audit.
7. Completion output and input buffers have different limits across hosts; Script's bounded stream reader does not imply a bounded terminal edit buffer. Unicode editing is scalar-based, not grapheme-aware. History persistence also differs by host. These are documented limitations, not features fixed by T1.

No new shared terminal abstraction is needed for the confirmed defect. Existing primitives and explicit host input policies provide the SS2 reuse path.
