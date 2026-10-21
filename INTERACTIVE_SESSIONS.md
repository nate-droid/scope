# Interactive language sessions

Scope's interactive shell supports optional, explicitly registered source evaluators.
No builtin language currently registers an evaluator. Analysis-only services remain
unchanged, and opening a document never selects a session or authorizes execution.
The terminal shell supports multiline editing; no language runtime is added.

## Source-aware evaluation

Both REPL submissions and explicit buffer evaluation now dispatch through
`InteractiveLanguageSession::evaluate_request(EvaluationRequest)`. Its default
forwards exact text to the existing `evaluate(&str)` implementation. Buffer-owned
`evaluation_source_id()` and in-memory versions distinguish dirty revisions from
disk baselines; independent submissions receive fresh identities.

`Diagnostic::locations` optionally attributes primary and related ranges to exact
source/revision pairs, including historical definitions. Existing diagnostic
constructors remain compatible; direct struct literals must add `locations`.
Scope does not automatically retain history or resolve locations against disk.
See [source contracts, downstream adaptation, and staged host command/navigation
design](docs/SOURCE_EVALUATION.md) for identity, retention, and compatibility rules.

## Embedding an existing runtime

`scope_ide::cli::run_with_session(app, controller, input, output)` consumes a
host-built `App` and `language::InteractiveSession` and runs the same command
loop as the registry CLI. `input: BufRead` and `output: Write` are injectable;
this entry point always uses line-oriented redirected input, not ambient terminal
input/history. EOF executes a final unterminated line once, then exits. Terminal
multiline/history configuration and contracted agent-edit configuration are not
exposed by this minimal embedding entry point. With the agent feature, normal
proposal-only agent commands use the CLI's default local model configuration.

Install a concrete adapter with `controller.install(language_id, boxed_adapter)`.
No registry provider is required: a nonempty exact identity match is supported.
An adapter can own an `Rc<RefCell<Runtime>>` while the host retains another typed
handle. Scope needs neither downcasting nor access to language-specific state.
The controller owns the adapter, not exclusive access to the shared runtime;
hosts must ensure the adapter's close invalidates runtime operations safely.
Do not install two adapters owning the same runtime into independent controllers.

See the runnable [typed runtime example](crates/scope-ide/examples/embedded_shell.rs):

```sh
cargo run -p scope-ide --example embedded_shell
```

### Read-only host commands

Hosts can supply a borrowed callback through
`scope_ide::cli::run_with_session_and_command(app, session, &mut command, input, output)`.
It uses the same redirected shell and session ownership as `run_with_session`.
The callback implements this interface:

```rust,ignore
impl scope_ide::cli::ReadOnlyCommand for HostCommand {
    fn invoke(
        &mut self,
        document: &scope_ide::document::Document,
        selection: scope_ide::text::TextRange,
        operation: &str,
        output: &mut dyn std::io::Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Inspect the live document and dispatch a host-defined operation.
        writeln!(output, "{operation}: version {}", document.version())?;
        Ok(())
    }
}
```

Invoke it only through the explicit namespace:

```text
:scope host inspect --selection 0:1..0:3
```

The syntax is exactly
`:scope host <operation> --selection <line>:<column>..<line>:<column>`.
Host selections use **zero-based lines and UTF-8 byte columns**, with a half-open
range. In the example, `0:1..0:3` selects `é`, not two characters. This differs
from the one-based coordinates of `scratch replace`. Missing/malformed selections,
reversed ranges, out-of-bounds positions, and positions inside a UTF-8 character
are errors; they do not invoke the callback.

The callback borrows the live **App open document**, including unsaved
edits, rather than rereading its file or using a stale snapshot. Before transferring
ownership, the host can call `app.open_path("example.decl")` and optionally edit
`app.open_document_mut()`. With no active runtime, legacy `open` and `replace`
also operate on this document. The example selection above assumes its text is
`aéz`. The namespaced `:scope open`, `:scope show`, and scratch commands operate
on the separate shell buffers, not this target; they do not open an App document. Its document
identity and version identify that in-memory snapshot: repeated inspection does
not edit it; subsequent edits preserve identity and advance the version. No open App
document is an error. The callback receives no mutable document or session access;
registration does not grant filesystem authority, register agent tools, or evaluate
source. Hosts define and validate their own operation names.

A shell without a registered callback reports an error for this command. Callback
errors are printed as shell errors and the loop continues: later host commands,
editing, and explicit evaluation remain usable. Namespaced host commands never fall
back to language evaluation, even on failure. Bare `host ...` is not a host-command
alias: without a session it is rejected, and with a session it remains ordinary
language source, just like other unprefixed input.

This narrow read-only hook is not the broader mutable, generation-aware host
namespace proposal below. A callback retaining its own runtime handle is responsible
for checking whether that runtime is still valid; this interface supplies no active
session attachment or generation token.

### Ownership and lifecycle

| Event | Contract |
|---|---|
| Installation | Takes the candidate by value; validates before touching the active session. Success transfers session ownership to the controller. |
| Invalid identity / empty requested ID | Candidate closes best effort and drops; active session and reset requirement stay unchanged. The candidate is not returned. |
| Successful replacement | Close old session, drop old adapter, install candidate and clear reset requirement. Same-language installation also replaces. |
| Old close fails during replacement | Candidate closes best effort and drops. Old session stays selected but evaluation requires successful reset, recreation, or clear. Close may have partial effects; no rollback is promised. |
| Registry reselection | Always creates a fresh independent session, even for the current language. A host-only ID without a provider cannot be reselected through the registry; the existing host session remains active. |
| Unsupported selection / factory failure | Active session is untouched. |
| Deselection (`session none`) | Detaches before close, drops even on close error. No runtime retention by Scope. |
| Reset | Calls the actual active adapter; only success clears reset-required state. Buffers are untouched. |
| Quit / EOF | Explicitly clears; close error is returned. Controller Drop does not close an already detached session again. |
| I/O or other early error | Controller Drop closes best effort; original error is preserved. |

A failed close on a retained session can be retried at later replacement/clear;
this is intentional existing behavior, not a guarantee that close is called only
once across failures. Session implementations must release resources via RAII
when close fails. Successful close is never followed by another controller close
on that detached adapter. Reset must actually restore usability before succeeding.

### Verified architecture and scope

The normal `run_with_registry` still builds its own App and empty controller.
Registry factories remain independent; embedding does not change their contract.
The existing REPL and explicit buffer-evaluation paths already share one controller;
this API supplies that controller rather than introducing another evaluator.
The App owns the legacy open document; the shell separately owns unified scratch/
file-backed buffers and proposal state. Both are consumed for the shell lifetime,
not returned to the host. Reset/replacement do not discard edited shell buffers.
Opening, editing, saving, inspection, and evaluation remain separate operations.
Language/session registration grants no filesystem or agent-tool authority.

The downstream `scope-forth/GENERIC_SHELL_SESSION_DESIGN.md` and unified-buffer
tests were inspected without modification. Their observations about private shell
ownership, independent factories, same-language recreation, and shared REPL/buffer
evaluation were confirmed. This embedding foundation does not migrate the Forth
workbench or add its tracing, source retention, or host command surfaces.

### Deferred extension stages (proposals, not implemented)

1. **Human command namespaces:** reserve explicit host namespaces at shell
   construction, reject duplicate/reserved namespaces, and dispatch with current
   App, selected shell buffer, and active-session generation at invocation time.
   Host handlers retain typed runtime handles, check active attachment/generation
   and closed state, and fail closed after replacement/deselection. Reset advances
   runtime generation so trace views cannot claim stale snapshots are live. This
   needs a buffer-aware shell context rather than just `&App`, because unified
   shell buffers currently live outside App. Human handlers may explicitly mutate
   authorized runtime configuration; registration must not create agent tools.
   Agent tools require separately registered read-only providers and independent
   authorization policies; evaluation/reset/trace mutation remain unavailable by
   default. Existing generic symbol inspection remains read-only.
2. **Source-aware requests:** add a language-neutral borrowed
   `EvaluationRequest { text, source_id, revision_id }` and a default session hook
   forwarding to `evaluate(request.text)`. Both controller evaluation routes would
   call it. Source and revision are opaque identifiers, not filesystem authority.
   Mint identities scoped to the shell/session and buffer/submission; bind each
   revision to the exact text snapshot before evaluation, including dirty edits.
   Repeated revisions must denote identical snapshots and cannot be recycled after
   reset. Retained historical traces require the runtime to retain those identities
   and exact text. Diagnostics from retained definitions must carry their original
   source/revision, not be relabeled with the latest request. This needs a compatible
   diagnostic/provenance design as well as the request hook; defer both rather than
   imply attribution is solved by passing a buffer version alone.

## Shell routing and selection

```text
scope> :scope session example.calc
Active language: example.calc
example.calc> some complete source
(provider output)
example.calc> :scope help
(Scope command help)
example.calc> :scope session-reset
session reset
example.calc> :scope session none
Active language: none
scope> quit
```

- With no session, bare commands retain their existing behavior.
- `:scope <command>` always routes to Scope, including unknown commands, which
  produce command errors and are never submitted to an evaluator.
- With a session, all other nonblank input is source, even `help` or `quit`.
  Use `:scope quit` to exit. Source is not chosen by a failed command parse.
- The namespace is exactly `:scope ` (ASCII space), or exact `:scope` for an
  empty command. There is no shell escape. This leaves Forth's `: name ... ;`
  syntax alone and preserves the existing document `language`/`lang` commands.
  The exact namespace prefix is reserved; a downstream language cannot evaluate
  a submission starting with that prefix through this shell.
- `:scope session` reports the active language. `:scope session <id>` explicitly
  selects an interactive provider; `none` is reserved for deselection.
- Each submitted buffer is one history entry, including embedded newlines. Source
  and namespaced commands share history; incomplete intermediate lines are not
  recorded. The persistence format is unchanged. Piped input remains line-oriented
  and does not persist history. Non-interactive CLI commands do not create sessions.

## Structured source edit review

Source proposals target Scope-owned in-memory buffers, independently of evaluator
selection. Use `:scope` before these commands when a language session is active.
These are separate from the older filesystem snapshot `proposal-*` commands.

With the `agent` feature, `:scope agent edit "<instruction>"` proposes changes to
an existing buffer. By default this is a single-decision, **unevaluated** proposal.
A downstream host can opt into a fixed contract-backed workflow through
`cli::run_with_registry_and_editing` and `cli::InteractiveEditConfig`. That shell
requires configured evaluation and never falls back to unevaluated generation.
Language registration or REPL session selection alone does not enable it. See
host configuration, assurance, and examples (historical reference unavailable in this checkout: `docs/design/M2A_HOST_CONFIGURATION.md`).
Both routes retain explicit review and acceptance; neither applies or saves automatically.

- `source-propose <JSON>` captures the selected buffer and its current version.
- `source-proposals` lists pending proposals.
- `source-proposal <id>` validates and displays proposal details.
- `source-diff <id>` validates and previews a unified diff with three context lines.
- `source-accept <id>` revalidates and atomically replaces only the target buffer.
- `source-reject <id>` rejects without changing source, including stale proposals.

For example, replace the first byte of the selected buffer with multiline Unicode:

```text
source-propose {"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":1}},"replacement":"猫\nsecond line"}],"rationale":"Review this replacement"}
source-proposals
source-proposal 1
source-diff 1
source-accept 1
```

Coordinates are zero-based lines and UTF-8 byte columns on character boundaries;
all ranges refer to the original source. Every edit must validate, and overlapping
edits are rejected. Unknown JSON fields are rejected. A buffer change after preview
causes a revision conflict on acceptance; proposals are never silently rebased.
Acceptance increments the buffer version once and marks it dirty, even for an
identical-text edit. It does not change selection, save, evaluate, grant filesystem
access, or interpret replacement text as commands. Applied/rejected proposals are
terminal. Failed acceptance preserves the buffer and leaves the proposal pending.
Saving and evaluation remain separate explicit operations.

Diff output is limited to a UTF-8-safe 16 KiB prefix before terminal escaping, with
an explicit truncation notice; acceptance always uses every original edit. Control
characters are escaped for display. Large changed regions may produce nonminimal
replacement hunks to bound quadratic diff work. The display limit does not bound
candidate allocation. Proposals and IDs live only for the current shell; there is
no persistence, multi-buffer transaction, or language-specific validation.

Hosts can also use `SourceEditProposal::preview`, `unified_diff`,
`unified_diff_bounded`, `accept`, and `reject` without an evaluator or shell.

## Scratch buffers

Scratch buffers are independent in-memory source documents, not the pending REPL
input or command history. They retain stable, non-reused IDs, a language ID,
editable `TextBuffer` source, an edit version, dirty state, and an optional
workspace destination with its observed disk revision. They survive session
reset, replacement, deselection, evaluation errors, and failed saves, but are not
restored after the shell exits.

All scratch commands require the explicit namespace:

| Command | Action |
| --- | --- |
| `:scope scratch new <language-id>` | Create and select an empty, dirty buffer; does not select a session. |
| `:scope scratch open <path>` | Open and select an existing workspace file in the same scratch collection, using the registered language services for language detection. Does not select a session. |
| `:scope scratch list` | List IDs, languages, dirty state, and selection. |
| `:scope scratch select <id>` | Select an existing buffer. |
| `:scope scratch show` | Display selected source exactly. |
| `:scope scratch replace <start-line> <start-column> <end-line> <end-column> <text>` | Apply the existing shell replacement syntax, including `\n`, `\t`, and `\\` escapes. Coordinates are one-based, with UTF-8 byte columns. |
| `:scope scratch capture` | Create and select a copy of the last REPL source submission and its language, including embedded newlines. |
| `:scope scratch eval` | Evaluate selected source through the active, matching registered session. |
| `:scope scratch destination <path>` | Observe an existing workspace file's revision without writing. Explicit rebinding accepts its current revision. |
| `:scope scratch save` | Save to the bound destination with revision checking. |
| `:scope scratch promote <path>` | Explicitly bind and save; retains the baseline when already bound to that destination. |
| `:scope scratch save-as <path>` | Explicitly save selected source to an existing workspace file using checked atomic replacement. |
| `:scope scratch close` | Close the selected buffer only if clean; refuses unsaved changes and never saves implicitly. |
| `:scope scratch reload` | Reload the selected buffer's bound destination only if clean; refuses unsaved changes and never writes. |
| `:scope scratch reload-discard` | Explicitly abandon unsaved edits and reload the bound destination; never writes. |
| `:scope scratch discard` | Explicitly delete the selected buffer, including unsaved edits, and select the newest remaining buffer. |

Evaluation never switches languages or saves source. Saving never executes source.
After evaluation, the existing `describe`, `effect`, and `where` commands inspect
the active session normally. Capture retains the last submission independently of
history; it does not extract source from interpreter state.

Persistence uses only the supplied Workspace's Cell filesystem capabilities and
existing checked atomic-replacement API. Missing write authority, confined-path
failures, and external revision conflicts leave source usable and dirty. After a
conflict, inspect the destination before deliberately rebinding its baseline and
retrying. The public `scope_ide::scratch` API exposes structured errors and retains
underlying session, text-edit, and filesystem errors.

**Current persistence limitation:** Cell's checked-save API requires an existing
regular file. Save As and promotion are existing-file-only: new-file destinations
are rejected rather than using an unchecked
create/overwrite operation. Create the destination separately using an authorized
host workflow before binding it or using Save As. Atomic replacement has best-effort revision
checking, not atomic compare-and-swap or a durability/fsync guarantee. Native
session adapters remain subject to the existing authorization contract below;
scratch operations do not install capabilities or widen grants.

## Workspace editing

The explicit `:scope` namespace exposes the same buffer collection without the
`scratch` prefix: `open <path>`, `buffers`, `buffer <id>`, `buffer eval`, `show`, `edit`, `save`,
`save-as <path>`, `reload`, and `close`. Opening an already-open path selects it
without rereading or discarding edits. Bare legacy commands retain their old
single-document behavior; use the explicit namespace for workspace buffers.

`:scope buffer eval` evaluates the selected buffer's current in-memory contents,
including unsaved edits, through the active matching language session. It uses the
same evaluation API and output/diagnostic rendering as scratch and REPL evaluation.
It never saves, resets, switches languages, or discards edits on failure. Interpreter
mutations before failure may remain; subsequent REPL input and symbol inspection
use that same session. `:scope session-reset` resets interpreter state, not buffers.
Missing buffers, inactive sessions, language mismatches, and unsupported session
providers report errors without implicit selection or fallback evaluation.

For example, select a registered session with `:scope session <language-id>`, open
source with `:scope open <path>`, edit with `:scope edit` and Ctrl-S on a TTY
(or `:scope edit --commands` and `:done`), then run
`:scope buffer eval`. Inspect definitions with `:scope describe <symbol>` and save
independently with `:scope save`. These operations retain the existing provider and
Workspace capability authorization boundaries.

`:scope show` previews the buffer identity and dirty state with numbered source,
limited to 40 lines and 160 Unicode scalars per line. Control characters are
escaped for display only. Use the editor's `:show START` to browse later pages.

On a TTY, `:scope edit` enters a language-neutral raw multiline draft editor.
Arrows and Home/End move through existing source; Enter inserts a newline,
Backspace/Delete remove text, Tab inserts a tab, and bracketed paste inserts
literal source rather than submitting shell commands. Ctrl-S accepts; Ctrl-C
or Escape cancels. Colon commands are source in this mode, not editor actions.

`:scope edit --commands` explicitly selects the legacy cooked, line-oriented
draft editor. Non-TTY `:scope edit` also uses command mode:

| Editor command | Action |
| --- | --- |
| `:at LINE COLUMN` | Move across lines; one-based Unicode-scalar coordinates, not byte columns. |
| `:insert TEXT` | Insert text at the cursor, preserving spaces; supports `\n`, `\t`, and `\\` escapes. Also inserts source beginning with a colon. |
| `:delete COUNT` | Delete Unicode scalars forward, including newlines. |
| `:show [START]` | Display a bounded numbered page, starting at line 1 by default. |
| `:complete [INDEX]` | List metadata-only source completions, or apply a current item; does not evaluate or authorize. |
| `:done` | Keep the draft in the selected buffer; neither saves nor executes. |
| `:cancel` | Drop the draft, leaving the original text and dirty state unchanged. |
| `:help` | Display editing commands. |

In command mode, other input lines insert literally at the cursor with a trailing newline,
including blank lines and indentation. The initial cursor is at the start of the
buffer. The prompt shows its ID, destination, cursor, and `*` for dirty contents.
EOF cancels the draft. Unknown colon commands are errors, not source or REPL
commands. Leaving the editor restores the existing language session unchanged.
Only the outer `:scope edit` command enters normal terminal history; individual
editor operations do not access history or its persistent store.

Save remains an explicit capability-backed checked replacement. Failed saves
retain text. Dirty close/reload are rejected; `:scope discard` and
`:scope reload-discard` explicitly authorize losing unsaved contents.
The existing-file-only Save As restriction above still applies.

Both modes edit a private draft: acceptance creates exactly one buffer revision
if and only if source changed; cancellation creates none. Neither saves nor
evaluates. Explicit line/column coordinates are one-based Unicode scalars;
`TextBuffer` uses UTF-8 byte offsets internally, not terminal display columns.

**Limitations:** Unicode-scalar editing is not grapheme-cluster editing. Drafts
copy source in memory. In command mode, multiline paste is processed as individual
input lines, so colon-leading source should use `:insert`; use the raw editor for
bracketed source paste. Buffers remain shell-local, without crash recovery.
See the [editing tutorial](docs/scratch-editing.md) and
[architecture diagnosis](docs/design/SCRATCH_EDITOR_DIAGNOSIS.md).

## Symbol inspection

Use `:scope describe <symbol>` for all available metadata, `:scope effect <symbol>`
for a signature or stack effect, and `:scope where <symbol>` for origin, location,
and definition. `:scope help` lists these commands without requiring a session.
Inspection requires an active session; names are passed verbatim after trimming
surrounding whitespace. These commands never submit source to the evaluator.
Output is bounded and terminal control characters are escaped. `describe` shows
at most 160 rendered bytes of definition text (plus a truncation marker); when
shortened, it points to `:scope where <symbol>` for the existing, larger bounded
view. Missing definitions remain `[unavailable]`, without a preview hint.

Downstream providers can override `InteractiveLanguageSession::inspect(&self, name)`
to return `Result<Option<SymbolDescription>, InspectionError>`. `Ok(None)` means
unknown, the default is `Unsupported`, and authorization/infrastructure failures
have separate code/message variants. Existing providers require no changes.
`SymbolDescription` reuses `SymbolKind`; optional `SymbolLocation` uses
`LanguagePath` and zero-based `TextRange`. Signature, documentation, origin,
definition text, and location are optional. Providers should reuse existing
analysis/interpreter metadata, including session-local definitions, rather than
create a second metadata registry. Scope does not load documents for inspection.

Lookup must use already authorized in-memory information: never evaluate (even
with rollback), mutate state, acquire capabilities, read files, or access the
network. Return `Unsupported` when safe inspection is impossible. Do not expose
unauthorized resource metadata. Native providers are trusted code; `&self` is a
contract, not a sandbox against interior mutability or ambient authority.
Inspection does not clear an evaluation reset requirement. Locations are display
only; there is no navigation or automatic documentation generation.

## Contracts and execution boundary

`scope-language` exports `InteractiveSessionProvider`,
`InteractiveLanguageSession`, `EvaluationResult`, `EvaluationStatus`,
`SessionError`, `Completeness`, `CompletionItem`, `CancellationHandle`, and `CancellationError`.
The synchronous APIs match Scope's current execution conventions. Session state
need not be `Send` or `Sync`; providers are `Sync`, like analysis services.

`EvaluationResult` contains ordered textual output and existing language
`Diagnostic` values. `Success` allows no output; `Failed` is a recoverable
language-level evaluation failure, not an infrastructure error. Infrastructure
and authorization failures use distinct structured `SessionError` variants with
codes and messages. `Cancelled` is separate and states whether reset is required.

The factory and evaluation methods receive no workspace, Cell, filesystem,
network, logging, or grant context. Registration is an explicit host decision,
not a grant. Native adapters are trusted host code, not sandboxed merely by
implementing a Rust trait: they must not use ambient authority to bypass Cell.

`scope-execution::execute` accepts a typed component pipeline, component bindings,
inputs, an existing Cell, and explicit planning grants. It creates fresh stores
for component calls; it supplies neither source compilation nor persistent
interpreter state nor interruption. Consequently this milestone adds a narrow
adapter contract instead of modifying that execution engine. A downstream
provider may lower source to pipelines and call `execute` with its already
configured Cell and grants, or adapt its existing authorized Cell execution
path. Existing Cell policies remain authoritative; do not install capabilities
or widen grants during session creation, evaluation, reset, completeness, or
cancellation. Scope does not supply a universal source-to-component adapter.

## Tab completion

`InteractiveLanguageSession::complete(&self, source: &str, cursor: usize)` is an
optional read-only hook returning `Vec<CompletionItem>`; the default is empty.
`CompletionItem` contains `label`, `replacement: Range<usize>`, `insert_text`, and
optional `detail`. The cursor and half-open replacement range are **UTF-8 byte
offsets in the entire source buffer**, not character counts, terminal cells, or
UTF-16 positions. They must be character boundaries. Scope drops invalid ranges
and rejects invalid cursors. Providers own all language syntax and symbol lookup.

Completion must not evaluate, mutate session state (including through interior
mutability), acquire capabilities, widen grants, or perform host side effects.
Like completeness, this is a trusted-provider contract, not a sandbox guarantee.
`InteractiveSession::complete` routes without creating or evaluating sessions.

- **Tab**: request completion at the cursor. Zero candidates leaves input unchanged;
  one inserts immediately; multiple open a bounded inline choices row.
- In the chooser, **Tab/Down** advances and **Shift+Tab/Up** moves backward, wrapping.
  **Enter** inserts the selection without submitting; **Esc** or **Ctrl+C** closes
  the chooser without changing the buffer or cursor. Other editing/history keys
  dismiss it and retain their normal behavior.
- Choices are shown in one row, selected choice first; narrow terminals truncate
  the row. Cycling reveals every choice. A one-row terminal cannot show choices.

Shell-only input completes command names and aliases from the parser declaration.
With an active language, bare input goes only to that language's completion hook;
`:scope <command>` completes Scope commands in either mode. Partial `:scope` at
the end of the whole buffer completes the reserved namespace. `session` is offered
only in the explicit namespace; `session-reset` requires an active session.
Agent commands are offered only when the agent feature is enabled. Command
arguments are not completed. A `:scope` token on a continuation line remains
language source, just as it does for submission.

Replacements preserve surrounding source, indentation, and newlines, placing the
cursor immediately after inserted text. Completion does not inspect completeness,
submit, record history, or reset history navigation. The editor's 1 MiB input limit
also applies atomically to completion insertions. No builtin language-specific
candidates, filesystem completion, fuzzy ranking, or automatic suggestions are added.

## Lifecycle

One session is retained across submissions. Reset preserves the selection while
restoring fresh provider state. Selecting any ID, including the current ID,
creates a fresh session and closes the old one; switching never caches sessions.
Resolution, creation, or ID-validation failure leaves the old session intact.
If closing the old session fails, the replacement is closed and discarded, and
the old session requires successful reset, recreation, or deselection before
further evaluation. Failed reset does not clear a reset requirement.

Deselection detaches and drops the session even if close fails. EOF and explicit
quit close the active session; the shell reports normal-path cleanup errors.
Error unwinding also attempts close through the controller's `Drop`. Providers
must release resources through their own RAII/`Drop`, since cleanup can fail.

## Completeness and cancellation

`InteractiveLanguageSession::completeness(&self, source: &str) -> Completeness`
is optional: its default returns `Complete` for every source, so existing providers
need no changes. Providers can override it using their existing syntax inspection:

- `Complete`: source is ready for evaluation, though evaluation may still fail.
- `Incomplete`: additional input may complete the source.
- `Invalid { diagnostics }`: a syntax error that additional input cannot resolve.
  Reuse `scope_language::Diagnostic` with source-relative ranges and optional
  source/code fields. The vector may be empty when diagnostics are unavailable.

The hook must never evaluate source (even speculatively with rollback), mutate
session state, dictionaries, stacks, or reset requirements, request or acquire
capabilities, widen grants, or perform filesystem, network, or other host side
effects. Use syntax-only inspection with local temporary data. A shared `&self`
borrow prevents ordinary mutation, but Rust traits cannot prohibit interior
mutability or ambient host access: this is a trusted provider contract, not a
sandbox guarantee. Providers should test state and authorization preservation.

Embedding hosts and the terminal REPL input handler can call
`scope_ide::language::InteractiveSession::completeness(&self, source)` through the
existing registered session. It returns `Some(Completeness)` for an active session
and `None` when none is selected, without creating a session or changing selection.
Inspection remains available when evaluation requires reset and never clears that
requirement. On Enter, the terminal editor routes the whole buffer first, then
inspects language source: complete source submits, incomplete source inserts a
newline, and invalid source remains editable with a recovery message. Scope
commands bypass completeness. A namespaced command on a continuation line is
part of the source buffer, not an independent command.

### Terminal input keys

- **Enter**: submit complete input; continue incomplete source; retain invalid input.
- **Ctrl+J**: insert a newline independently of completeness.
- **Ctrl+S**: explicitly submit complete or invalid input; incomplete source is refused.
- **Ctrl+C**: abandon the entire pending buffer and reset history navigation.
- **Ctrl+D**: EOF on an empty buffer; otherwise delete the character at the cursor.
- **Left/Right**: move across characters and newline boundaries.
- **Up/Down**: move between source lines; recall history at buffer boundaries.
- **Ctrl+P/Ctrl+N**: previous/next history entry regardless of cursor position;
  advancing past the newest entry restores the pending draft.
- **Home/End**: beginning/end of the current line; **Ctrl+A/Ctrl+E**: whole buffer.
- **Backspace/Delete**: delete characters, joining lines at newline boundaries.

Bracketed paste remains editable and preserves embedded newlines; it never
submits automatically. Buffers are limited to 1 MiB of UTF-8, with oversized
insertions rejected atomically. Unicode and whitespace are preserved in source;
non-ASCII and control characters are displayed as escapes, as in the previous
editor. The inline viewport is bounded to eight rows and scrolls to the cursor;
long lines scroll horizontally. Submitted buffers enter normal terminal scrollback.

These bindings require a terminal that reports the corresponding keys. Ctrl+J
may be indistinguishable from Enter in some terminals; Ctrl+S may be intercepted
by terminal settings. Shift+Enter is not required. Without bracketed-paste support,
the terminal may deliver pasted newlines as ordinary Enter keys instead. Resize
handling relies on terminal cursor reports and reflow behavior; a one-row terminal
cannot display a separate recovery/limit status row.

An optional independent `Arc<dyn CancellationHandle>` allows an embedding host
to signal an in-flight synchronous evaluation from another thread without
borrowing the session. No handle means unsupported; a handle may also explicitly
return `CancellationError::Unsupported`. A successful request signals intent,
not completion. Only the eventual `Cancelled` result confirms cancellation.
Providers must prevent idle/stale signals from cancelling a later submission.
If coherent continuation is impossible, return `Cancelled { requires_reset: true }`;
the controller blocks subsequent evaluation until reset or replacement.

The current terminal shell has no in-flight Ctrl+C dispatcher. In-flight terminal
interruption is **unsupported**, even for a provider exposing a handle. Ctrl+C
while editing clears the entire pending buffer. Safe terminal signal
integration and actual Cell interruption are separate work; this API does not
pretend to implement them or add a scheduler.

## Minimal downstream registration

The following adapter delegates to a downstream backend; it is not an interpreter.
The backend must retain an already authorized Cell/execution path and translate
its results to Scope's structured result/error types.

```rust
use scope_language::{
    EvaluationResult, InteractiveLanguageSession, InteractiveSessionProvider,
    SessionError,
};
use scope_ide::language::LanguageRegistry;

// Implemented by the downstream language using its existing authorized runtime.
trait AuthorizedBackend {
    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError>;
    fn reset(&mut self) -> Result<(), SessionError>;
    fn close(&mut self) -> Result<(), SessionError>;
}
struct Session(Box<dyn AuthorizedBackend>);
impl InteractiveLanguageSession for Session {
    fn language_id(&self) -> &str { "example.calc" }
    fn evaluate(&mut self, source: &str) -> Result<EvaluationResult, SessionError> {
        self.0.evaluate(source)
    }
    fn reset(&mut self) -> Result<(), SessionError> { self.0.reset() }
    fn close(&mut self) -> Result<(), SessionError> { self.0.close() }
}
struct Provider {
    // Host-supplied factory; does not install capabilities or widen existing grants.
    open_authorized_backend: fn() -> Result<Box<dyn AuthorizedBackend>, SessionError>,
}
impl InteractiveSessionProvider for Provider {
    fn language_id(&self) -> &'static str { "example.calc" }
    fn create_session(&self) -> Result<Box<dyn InteractiveLanguageSession>, SessionError> {
        Ok(Box::new(Session((self.open_authorized_backend)()?)))
    }
}
fn run(provider: &'static Provider) -> Result<(), Box<dyn std::error::Error>> {
    let registry = LanguageRegistry::builtins().with_interactive_provider(provider);
    scope_ide::cli::run_with_registry(std::env::args().skip(1), registry)
}
```

Analysis providers can independently be added with `with_language`. Existing
`run_with_languages` callers remain compatible. Embedded shells can use the
public `scope_ide::language::InteractiveSession` controller directly.
