# Integration

Scope integrations are compile-time Rust composition, not dynamically loaded
language plugins. APIs and formats are experimental (`0.1.0`). Keep your parser,
interpreter or proof/scientific kernel independent of Scope's presentation layer.

- [Choose an integration path](#choose-an-integration-path).
- [Implement a language](#3-implement-languageservice) and adapt source positions.
- [Embed sessions](#interactive-language-sessions), commands and retained source.
- [Compose a TUI](#tui-integration) with explicit draft and execution boundaries.
- [Execute components](#execute-a-typed-pipeline-through-cell) or exchange manifests.

See [Development](development.md) for setup/features and [Agents](agents.md) for
model loops, call-aware tool policy, budgets and filesystem proposals.

## Prerequisites and current limits

Use a local Scope checkout. Scope path dependencies are development paths, not
portable distribution; no registry release or MSRV guarantee is assumed. Cell
comes from Git tag `v0.0.2`; no sibling Cell checkout is needed. Direct consumer
Cell dependencies should use the same URL/tag. Host integrations need no Wasm
target or model server. Guest development follows the matching
[Cell tag](https://github.com/nate-droid/cell/tree/v0.0.2).

`scope-ide` defaults to `tools`; `agent` and `tui` opt in and include tools.
`logging` is separate. Downstream CLI features must explicitly forward dependency
features, e.g. `agent = ["scope-ide/agent"]`. Cargo feature unification can still
enable tools through `scope-script` despite a direct `default-features = false`.

## 1. Mental model

| Surface | Owns / supplies |
| --- | --- |
| `Workspace` | Capability-bearing project file access |
| `Document` / `DocumentSession` | Open App text / revision-bound headless source |
| `LanguageService` | File detection, analysis, semantic descriptions and adapters |
| `InteractiveLanguageSession` | Explicitly selected, stateful source evaluation |
| `ScratchBuffers` | Independently editable committed source and semantic binding |
| `App`, CLI, TUI | Human presentation and host-controlled workflows |
| `ToolProvider` / Cell | Native structured operations / capability-backed components |

## Choose an integration path

### 1. Language service only

Depend on `scope-language`, implement `LanguageService`, and test with
`LanguageDocument`. No IDE or Cell runtime is needed. Expose a static service
for later registration. Keep parsing and checking in your own library.

### 2. Headless app through `AppBuilder`

Add `scope-ide` and register with
`App::builder(workspace).language(&LANGUAGE).build()?`. Prefer a read-only
workspace for inspection. An App does not require a TUI or agent.

### 3. Reuse the CLI with `cli::run_with_languages`

Forward arguments **without** the executable name:

```rust
scope_ide::cli::run_with_languages(
    std::env::args().skip(1),
    [&LANGUAGE as &'static dyn scope_language::LanguageService],
)?;
```

The stock binary does not register your service. Default composition appends
services after built-ins; first match wins. For custom precedence supply a
`LanguageRegistry` through `AppBuilder::language_registry`. Unmatched documents
use neutral semantic fallback.

### 4. Typed component execution without the IDE

Use `scope-types` for descriptions/validation alone, or add `scope-execution`
and Cell for components. This needs no `LanguageService`, TUI, agent or API key.
See [component execution](#execute-a-typed-pipeline-through-cell).

## 2. Scaffold a language

From the Scope root:

```sh
cargo run -p xtask -- new-language ../scope-calc-language \
  --extension calc --id example.calc --name Calc --with-runner
cargo test --manifest-path ../scope-calc-language/Cargo.toml
```

The generator creates a standalone crate, starter tests and optional host binary.
Start with file detection, declaration symbols and one useful diagnostic. The
complete [standalone consumer](../examples/downstream/src/lib.rs) demonstrates a
library shared by [headless](../examples/downstream/src/bin/headless.rs) and
[CLI](../examples/downstream/src/bin/calc-scope.rs) hosts.

## 3. Implement `LanguageService`

Implement `language_id`, `language_name`, `matches_path` and `analyze`. Return
`LanguageAnalysis { diagnostics, symbols }` from your own parser/checker.
`actions` is optional descriptive metadata, not automatic execution.

### Combined document analysis

`diagnostics` and `outline` default to projections of `analyze`; never call those
defaults from `analyze` or they recurse. Share one parser pass when both outputs
are needed; override individual projections only for cheaper single-output paths.
`App::open_analysis` uses combined analysis. There is no cross-call/redraw cache.
Older services implementing only diagnostics/outline must implement `analyze`.

`LanguageDocument::version()` is local to a document lifetime. Reopening a path
can restart at zero with different text: `(path, version)` is not a safe global
cache key. Retain source identity and the exact revision/text with derived results.

### Source positions and diagnostics

`Position` uses zero-based lines and **UTF-8 byte columns**; `TextRange` is
half-open. Use stable symbol kinds and precise diagnostic ranges. Complete
`Diagnostic` literals must include `locations`; constructors remain compatible.

### Adapting parser byte spans

Build one `scope_language::LineIndex` per immutable snapshot. Use
`range_for_span` / `span_for_range` for ranges and `position_for_byte` /
`byte_offset` for endpoints. Invalid, reversed, out-of-bounds or split-character
positions return `None`, never clamped. Convert inclusive parser ends explicitly
with checked arithmetic; UTF-16 units and character indices are not byte offsets.

Only LF starts a line. Its offset is the preceding line's final column; the byte
after LF is `(next_line, 0)`. CR is ordinary data, with no CRLF normalization.
EOF, empty/multiline spans and a trailing empty line are valid; empty text has
only `(0, 0)`. Accepted positions round-trip. Construction is O(bytes), byte lookup
O(log lines), position lookup O(1). See the [API and tested example](../crates/scope-language/src/line_index.rs).

### Headless source sessions

`DocumentSession::open(workspace, document_id, path)` provides owned snapshots
without an App/UI/agent. Keep the opaque session-local revision and call
`snapshot_at(&revision)` before using derived results as current. External changes
or read failures invalidate the snapshot; `reload()` explicitly accepts current
source, and `close()` releases the retained grant. Changes after a check, or
changes reverted between reads, can escape observation.

### Reviewable agent edits

Headless edits separate `propose`, `review`, `authorize`, `apply`. Analyze the exact
candidate, not the base; invocation approval is not persistence approval. Apply
requires a writable workspace and checked saving. See the complete
[headless edit workflow](agents.md#reviewable-headless-edits); it is separate from
unsaved App buffers, scratch proposals and filesystem snapshots.

## 5. Grow semantics before infrastructure

Add outline and diagnostics before structured queries, domain tools or custom
panes. Prefer `definition(symbol)` or `check_proof(term)` over generic file/process
execution. Proof validity belongs in the kernel, not in the UI or model.

## 6. Optional agent tools

A `ToolProvider` supplies namespaced structured operations, independently of
language registration. Describe actual `ToolEffect` and inputs; use `invoke` or
`invoke_with_app` when live App state is needed. Hosts compose providers in
`AgentSession`, discover via `catalog()`, and authorize actual arguments before
dispatch. See [catalog and policy](agents.md#model-independent-harness-discovery).

### Discoverable revision-bound operations

For headless queries implement `tool::operations::OperationProvider` (`tools`)
and register in `OperationRegistry`. Descriptors have provider/local IDs, closed
recursive schemas, effects, execution characteristics and host permission names.
Invoke against a `DocumentSession`, expected revision, arguments and call-aware
policy. The registry rejects unknown operations, malformed inputs, stale revisions
and denial before dispatch; it validates bounded output and rechecks freshness.

Providers receive an immutable snapshot, not an App. This is not automatic chat
registration or unsaved-buffer integration. Permission names require host policy;
payload bounds neither sandbox native code nor limit its time/allocations or
roll back effects. See the [public API and neutral tests](../crates/scope-ide/src/tool/operations.rs).

### Interactive workbench

`WorkbenchPane` (`tools,tui`) presents the headless `workbench::Workbench` with a
`DocumentSession` and registry, not a second copy of unsaved App state. Hosts must
supply operation authorization, an `OperationResult -> VerificationReport`
interpreter, and diagnostics for the exact candidate. There are no permissive
defaults. Queueing is not execution; invocation and edit acceptance are separate.

Stale candidates cannot apply; refresh failure hides unavailable source/evidence.
Verification remains bound to the checked revision and distinguishes passed,
failed, inconclusive, unavailable, skipped, denied, cancelled and error outcomes.
A passed check is not edit approval. Cancellation removes queued work, not an
in-flight synchronous provider. `push_message` does not connect remote chat or
register tools. See the [pane API and controls](../crates/scope-ide/src/tui/workbench.rs).

## Trust and filesystem authority

Native services, providers, panes and session adapters are **trusted host code**,
not sandboxed Cell guests. Read-only parameters and effect metadata constrain
APIs, not ambient Rust filesystem/network/process access. Do not bypass host
policy or Cell confinement using ambient APIs. Registration, discovery, binding
and successful evaluation grant no write, execution or filesystem authority.

`Workspace::read_only(root)` denies writes; `Workspace::new(root)` permits them.
Discovery skips symlinks/special entries and propagates lookup errors, but explicit
reads may follow confined relative symlinks. Cell enforces confinement per
operation; do not substitute host-path canonicalize/check/write preflights.
`capabilities()` lists interfaces, not roots or permission: both policies report
`cell:fs/fs@0.1.0`. Authority/descriptors are advisory, not authorization checks.

## 8. Host capabilities and logging

With `logging`, a trusted host constructs `(LoggingBinding, LoggingController)`,
retains the controller, authorizes a Cell `Logger` and gives adapters only binding
handles. Bindings deny by default. Revocation affects later admissions, not
already-admitted calls; receipts do not guarantee durability. Analysis/inspection
never create grants. See the complete [logging example](../crates/scope-ide/examples/native_logging.rs).

## Structural workspace snapshots

`workspace::WorkspaceMeta` builds bounded typed graphs through the confined
workspace, without optional features. It supports navigation, source locators,
revision-aware references, explicit invalidation and optional Cell watching.
`scope-ide --workspace <path> meta` is read-only. Metadata grants no source access;
see [graph contracts](../crates/scope-meta/README.md) and the adapter's generated
API for freshness/truncation limits.

## Normal launcher with Script inspection

`cli::run_with_registry_and_command(args, registry, &mut command)` combines your
`LanguageRegistry` and retained `ReadOnlyCommand` through normal startup.
`run_with_registry` and `run_from_args_with_command` remain alternatives.

Bind semantic source with `:scope semantic-target ID` (IDs from `:scope buffers`);
report with no argument or clear with `none`. Selection and binding are independent.
`:scope script run` supplies Script source and the bound target to
`invoke_script_with_target(source, target, output)` as immutable invocation-local
borrows. Its default delegates to `invoke_script`. A closed explicit target fails,
never falling back to App/disk/selection. Source selection is not authorization.
See the complete [launcher example](../crates/scope-script/examples/semantic_target_cli.rs)
and [SI6 source/authorization contract](../crates/scope-script/docs/SI6.md).

## Namespaced downstream shell commands (SC1)

Override `ReadOnlyCommand::downstream_namespaces` and `invoke_downstream`.
Use `:scope downstream` to discover and `:scope downstream demo echo first second`
to dispatch. Startup snapshots/validates at most 64 unique namespace names, each
matching `[a-z][a-z0-9_-]*`, at most 64 bytes. Invalid registration fails startup.
There is no interception of bare input or unknown built-in commands.

Arguments are borrowed whitespace-delimited tokens; Unicode and literal `$` stay
literal. Quotes/backslashes are rejected (`invalid_arguments`); no quoting,
escaping, expansion or expression syntax exists. Return `Handled`, `Unhandled`
(rendered as `unknown_command`), or `DownstreamError::new(code, message)`.
Unknown namespaces never invoke the callback. Errors display and the shell
continues; safely render untrusted callback output for terminals.

The target is only the independent bound semantic scratch: `None` is unbound,
a closed binding is `target_unavailable` before dispatch. Its source ID, version
and exact unsaved text remain consistent during the borrow. Convert version with
`RevisionId::from_version`; require a target explicitly if your operation needs one.
No mutable source/workspace/session access or new authority is supplied.

Keep existing concrete authorization, e.g. dispatch a fixed trusted Script through
the same runner/bindings. Never interpolate untrusted executable text or bypass an
authorized semantic provider. Check interpreter records **and semantic envelopes**:
a successful callback/setup result is not semantic success. Dispatch is synchronous;
there is no native cancellation, dynamic registration or argument completion.
Complete [consumer](../crates/scope-script/examples/downstream_shell.rs) and
[permission/provenance tests](../crates/scope-script/tests/downstream_shell.rs).

## Bounded host command compositions

`ReadOnlyCommand::compositions()` registers explicit `:scope DOMAIN COMMAND`
sequences of typed create/open/select/edit/bind and downstream steps, not recursive
shell fragments or mutable App access. Registration grants nothing. Use
[bounded compositions](BOUNDED_HOST_COMPOSITIONS.md) for complete examples and limits.

## Interactive language sessions

No built-in language registers an evaluator. Analysis and opening a document do
not select a session or authorize execution. Register an `InteractiveSessionProvider`
separately from `LanguageService`, or install a host-owned session directly.

### Minimal downstream registration

Implement `InteractiveLanguageSession` identity, `evaluate`, `reset`, and optional
`close`, adapting an already-authorized backend. A provider's `create_session`
must return an independent session matching its nonempty language ID. Register
with `LanguageRegistry::builtins().with_interactive_provider(provider)` and launch
via `run_with_registry`; add analysis independently with `with_language`.
See [interactive traits](../crates/scope-language/src/interactive.rs) and the
complete [embedded runtime example](../crates/scope-ide/examples/embedded_shell.rs).

### Contracts and execution boundary

Providers are `Sync`; session state need not be `Send`/`Sync`. Evaluation is
synchronous. `EvaluationResult` contains ordered output and diagnostics: `Success`
may be silent, `Failed` is a recoverable language failure, and authorization/
infrastructure failures are distinct `SessionError`s. `Cancelled` reports whether
reset is required. Mutations before failure need not be rolled back.

Factories/evaluators receive no workspace/Cell/grant context. Reuse the host's
existing authorized execution path; do not acquire capabilities or widen grants
during creation, evaluation, reset or inspection. Scope supplies neither a universal
source compiler nor persistent interpreter state through its component executor.

### Source-aware evaluation

REPL and buffer evaluation use `evaluate_request(EvaluationRequest)`; its default
forwards exact text to `evaluate`. Buffer source IDs and versions distinguish dirty
revisions; independent submissions get fresh identities. `Diagnostic::locations`
can identify primary/related historical source revisions. Retain exact source text
and identities yourself; Scope does not load locations from disk or retain your
runtime history. See [source evaluation](SOURCE_EVALUATION.md).

### Embedding an existing runtime

`cli::run_with_session(app, controller, input, output)` consumes a host App and
`language::InteractiveSession`. `BufRead`/`Write` are injectable; input is always
line-oriented, without ambient terminal/history. EOF evaluates a final unterminated
line once. This minimal entry point does not configure terminal multiline/history
or contracted agent editing; agent proposal commands use default local settings.

`controller.install(language_id, boxed_adapter)` needs no registry provider. Shared
`Rc<RefCell<Runtime>>` handles are possible, but the host must enforce closed/reset
state and avoid independent controllers owning the same runtime. Standard registry
startup instead builds its own App and empty controller.

### Read-only host commands

`run_with_session_and_command` additionally borrows a `ReadOnlyCommand`. Its
`invoke` inspects the live **App open document**, not the selected shell scratch:

```text
:scope host inspect --selection 0:1..0:3
```

Selections are zero-based, half-open UTF-8 byte positions (`é` in `aéz` above).
Malformed/split-character/reversed/out-of-range selections reject before callback;
missing App document is an error. No mutable document, session-generation token,
filesystem grant or agent registration is provided. Callbacks validate operation
names and shared-runtime validity. Errors continue the shell, never fall back to
evaluation; bare `host ...` remains language source when a session is active.

### Shell routing and selection

`:scope ` (ASCII space) and exact `:scope` reserve Scope commands; unknown commands
are errors, never evaluator input. With a session, all other nonblank input is
source, including `help` and `quit`; exit with `:scope quit`. A namespace token on
a continuation line is source, not a separate command. Without a session legacy
bare command behavior remains. `:scope session ID` selects, `session` reports,
`session none` deselects, and `session-reset` resets without discarding buffers.
Each submitted multiline buffer is one history entry. Piped input is line-oriented
and does not persist history; noninteractive CLI commands create no sessions.

### Lifecycle

#### Ownership and lifecycle

| Event | Contract |
| --- | --- |
| Install / select | Validate candidate before changing active state; successful replacement closes/drops old adapter. Same-language selection creates fresh state, not a cache. |
| Invalid identity | Candidate closes best effort and drops; active/reset state stays unchanged. Unsupported selection/factory failure also leaves active state untouched. |
| Old close fails | Candidate closes/drops; old session stays selected but requires successful reset, recreation or clear. Close may have partial effects. |
| Host-only ID | Cannot be reselected through a registry lacking its provider; existing session remains active. |
| Reset | Only successful reset clears reset-required state; buffers and selection survive. |
| Deselect | Detach before close; drop even on close failure. |
| Quit / EOF | Explicit clear; normal cleanup errors are reported. |
| Early I/O/error | Drop closes best effort without replacing the original error. |

Close may be retried after failure; adapters need RAII resource cleanup. Successful
close is not repeated for a detached adapter. Shared handles do not gain a runtime
attachment/generation guarantee from these APIs.

### Symbol inspection

`inspect(&self, name)` returns optional `SymbolDescription`; default `Unsupported`,
unknown `Ok(None)`, authorization/infrastructure errors distinct. `:scope describe`,
`effect`, `where` reuse this in-memory metadata without loading source or evaluating.
Optional locations use zero-based ranges and are display-only, not navigation.
Output is bounded/control-escaped; definition preview in `describe` is 160 rendered
bytes with a hint to `where` when truncated.

### Tab completion

`complete(&self, source, cursor)` defaults empty. Cursor and half-open replacement
ranges are UTF-8 byte offsets in the **whole buffer**, on character boundaries.
Scope rejects invalid cursors/drops invalid ranges. Completion cannot evaluate,
record history or reset navigation; the 1 MiB input bound applies atomically.
Bare source completes via the active language; `:scope` completes built-ins.
Command arguments have no completion. Tab inserts a sole candidate or opens choices;
Tab/arrows cycle, Enter inserts without submitting, Esc/Ctrl+C dismisses.

### Completeness and cancellation

`completeness(&self, source)` defaults `Complete`; `Incomplete` requests more input,
`Invalid { diagnostics }` retains editable source. Complete does not guarantee
evaluation success. Inspection, completion and completeness must use authorized
in-memory/syntax data only: no speculative evaluation/rollback, interior mutation,
I/O or capability acquisition. `&self` is a trusted contract, not a sandbox.
These hooks neither clear reset-required state nor create sessions.

An optional independent `Arc<dyn CancellationHandle>` lets another host thread
signal intent; only an eventual `Cancelled` result confirms cancellation. Prevent
idle/stale requests from cancelling later work. `requires_reset: true` blocks later
evaluation until recovery. The terminal shell has **no in-flight Ctrl+C dispatcher**,
even when a provider supplies a handle; editing Ctrl+C only clears pending input.

#### Terminal input keys

Enter submits complete input, continues incomplete input and retains invalid input.
Ctrl+J inserts a newline; Ctrl+S forces complete/invalid submission, not incomplete.
Ctrl+C abandons pending input; Ctrl+D is EOF only when empty. Ctrl+P/N navigate
history; arrows move through source and recall history at boundaries. Bracketed
paste stays editable and never submits automatically. The 1 MiB UTF-8 limit rejects
oversize insertion atomically. Terminals may intercept Ctrl+S or conflate Ctrl+J
with Enter; without bracketed-paste support pasted newlines may act as Enter.

## Scratch buffers

Shell scratches are independent of REPL input/history and the App document. They
retain non-reused IDs, language, text/version, dirty state and optional destination
with observed disk revision. Session reset/replacement/failure does not discard them.

| Explicit command | Purpose |
| --- | --- |
| `:scope scratch new LANGUAGE`, `scratch capture` | New source / copy last REPL submission; neither selects a session |
| `:scope open PATH`, `buffers`, `buffer ID`, `show` | Open/select/inspect the shared collection; reopening selects without rereading |
| `:scope scratch replace ...` | One-based lines/UTF-8 byte columns, half-open range; `\n`, `\t`, `\\` escapes |
| `:scope buffer eval` / `scratch eval` | Evaluate exact selected unsaved text through the matching active session |
| `:scope scratch destination PATH` | Observe/rebind an existing file baseline without writing |
| `:scope save`, `save-as PATH`, `scratch promote PATH` | Explicit checked persistence; promotion binds then saves |
| `:scope close`, `reload` | Refuse dirty source, never implicitly save |
| `:scope discard`, `reload-discard` | Explicitly lose unsaved text |

Evaluation never saves/switches/reset sessions; saving never evaluates. Missing or
wrong-language sessions fail without fallback. Interpreter mutations before failed
evaluation may remain. Capture is not extraction from interpreter state.

Promotion requires an **existing regular file**. Save As checks an existing
regular file's observed revision, or exclusively creates a missing destination
through Cell's public `Workspace::create_text`. Both require existing write
authority; neither grants permissions or creates parent directories. Only
`NotFound` permits the create attempt, which never replaces a competing entry
(including a dangling symlink). The binding and clean state change only after
success; subsequent Save uses the returned revision. Conflicts leave source
usable; inspect before deliberately rebinding the baseline. Checked replacement
is best-effort, not atomic compare-and-swap or a multi-file transaction.

Exclusive creation is Unix-only and has no fsync durability guarantee. Cell's
contract requires a parent without hostile writers: interference with the named
temporary source can invalidate content/revision guarantees. This is not a
hostile-directory transaction. The pinned Cell `v0.0.2` (`86145d1`) already exposes
this API; the locally inspected newer `31f760f` source has the same contract, so
no dependency upgrade or direct host-filesystem fallback is needed. Retained
editor Save/Save As still refuse any dirty draft (including parked drafts);
publish remains separate from persistence and approvals are unchanged.

### Workspace editing

Namespaced `:scope` operations address these buffers; legacy bare `open`/`replace`
address the separate App document when no session is active. TTY `:scope edit`
uses a private raw draft: Ctrl+S accepts, Ctrl+C/Esc cancels, paste is literal source.
Non-TTY or `edit --commands` uses cooked editing: `:at`, `:insert`, `:delete`,
`:show`, `:complete`, `:done`, `:cancel`. Colon-leading source uses `:insert` there;
unknown colon commands error. EOF cancels. Only the outer edit enters history.

Draft coordinates are one-based **Unicode scalars**, unlike byte-column replacement
commands and zero-based API ranges. Editing is not grapheme/display-cell editing.
Accepting a changed draft creates one revision; cancellation creates none. Neither
saves nor evaluates. `show` is a bounded escaped preview, not exact export.
See [editing controls/tutorial](../TUTORIAL.md) for the complete workflow.

### Inspect and edit source files

For legacy App editing, `outline`/`diagnostics` use current unsaved text; `replace`
uses one-based half-open byte coordinates with `\n`, `\t`, `\\` escapes. Exactly
one separator after coordinates is consumed; other spaces are literal. Invalid
ranges reject unchanged. `save` uses checked existing-file replacement and retains
unsaved source on conflict; `revert` restores saved text. Final symlinks cannot be
saved. See [Development](development.md) and matching Cell docs for platform limits.

## Retained semantic source across workspace handoffs (S2)

Own one `ScratchBuffers` and pass it to
`WorkspaceTui::run_with_sources(app, session, command, &mut sources)`. For custom
hosts use `cli::run_terminal_borrowed_with_sources` (suspend TUI/readers first) or
`run_borrowed_with_sources` with injected streams. Plain `run`/`run_terminal_borrowed`
use disposable invocation-local scratches.

Pane `ctx.bound_semantic_source()` distinguishes no supplied owner (`None`),
`BoundSource::Unbound`, closed `Unavailable`, and `Available(snapshot)`. Import
`BoundSource` from `scope_ide::scratch`. `render_with_sources` supplies this view;
plain render does not. Snapshots own historical source/revision/text, not a live
view; query again next render. Compare `SourceId`, not paths/local scratch numbers.

Live text, identities/revisions, dirty/destination state, selection and independent
binding/tombstone survive handoffs. Committed changes are not rolled back by quit,
EOF or errors; explicit close/discard/unbind still apply. Invocation-local proposals,
inspections, submission capture and shell state do not survive; nothing persists
across process restart or saves/evaluates automatically. App remains separately
owned. Binding retention never renews authorization. Complete
[retained workspace example](../crates/scope-ide/examples/retained_workspace.rs) and
[lifecycle tests](../crates/scope-ide/tests/retained_workspace.rs).

## Structured source edit review

`source-propose JSON` captures selected source/revision; `source-proposals`,
`source-proposal ID`, `source-diff ID` review; `source-accept ID` applies;
`source-reject ID` rejects. Use `:scope` prefixes. These are in-memory proposals,
not filesystem `proposal-*` snapshots described in [Agents](agents.md#isolated-text-proposals).

Edits use zero-based half-open UTF-8 byte coordinates against the original source.
All edits must validate; overlaps/unknown JSON fields reject. Acceptance rechecks
the original target/revision and never silently rebases or retargets. It increments
version once and marks dirty even for identical-text edits, but never saves,
evaluates, changes selection or grants authority. Applied/rejected states are
terminal; failed acceptance preserves source and leaves the proposal pending.
Diff display is a UTF-8-safe 16 KiB prefix before terminal escaping, not a bound
on candidate allocation or applied edits. No multi-buffer transaction is provided.

### Retained source proposals (W8)

Workspace F4 shares a retained source-edit controller across submissions and blocks
acceptance on unresolved editor drafts. Trusted hosts may borrow it through
`WorkspaceCommandOutput::source_edit_controller` / `WorkspaceTui::source_edit_controller`.
See [complete lifecycle and public consumer](RETAINED_SOURCE_PROPOSALS.md).

### Contracted semantic editing in the ordinary workspace (W8b)

`agent edit` normally produces a single-decision **unevaluated** proposal. Hosts
opt into `InteractiveEditConfig` through `run_with_registry_and_editing` or
`run_with_registry_and_command_and_editing` (workspace needs `agent,tui`). The latter
retains host commands, validated contract, authorization/model settings and proposal
controller. Configured evaluation never falls back to unevaluated generation.
Evaluation authority is not proposal-production authority, acceptance does not
certify semantics, and there is no separate revocable scratch-acceptance callback.
See [contract/configuration](CONTRACTED_WORKSPACE_EDITING.md) and the complete
[host adapter](../crates/scope-ide/examples/contracted_workspace.rs).

### Normal agent against retained semantic source (SI7)

With `agent`, return `SemanticAgentBinding` from `ReadOnlyCommand::semantic_agent`
to investigate only the bound scratch via an operation allowlist and per-call
policy. Missing/closed/wrong-language targets fail before model execution, with no
App/disk fallback. Immutable borrowing pins revision; the observer gets attribution,
termination/counts and bounded protocol bytes. Returning `None` preserves ordinary
agent behavior. See [SI7](SI7.md) and the complete
[offline example](../crates/scope-ide/examples/retained_semantic_agent.rs).

## TUI integration

Enable the dependency feature `tui`. `Tui::builder().with_default_panes()` adds
Files/Document/Language; `.pane(...)` adds custom `Pane`s. Without default panes
only yours are installed. Distinguish this App browser from editable scratch-based
`WorkspaceTui`; browser opening refuses dirty App documents but custom panes must
enforce their own replacement safety. Use [composable workspace examples](COMPOSABLE_TUI.md)
for nested layouts, stable instances and shell handoffs.

### Custom TUI panes

Give panes distinct IDs. `render` receives assigned geometry/read-only context;
`handle_key` receives mutable context only while focused, with no fallback to
unfocused panes. Use runtime focus/scroll/viewport state; clamp scroll to content
and account for borders. Render full content every frame; Ratatui diffs cells.
Standalone contexts lack persistent runtime scroll storage.

### Redrawing from your own event loop

`Tui::run` owns terminal setup and blocking polling. Custom loops own raw mode,
alternate screen, cleanup, resize and exit policy; call `draw` initially and after
input/background changes/resizes. `handle_key` returns `InputOutcome`, including
quit intent. Modal input takes precedence. `redraw_interval` changes poll timeout,
not guaranteed cadence; zero may busy-poll. For frames use `render` in your
terminal draw callback. See the [TUI API](../crates/scope-ide/src/tui/mod.rs).

## Contextual workspace actions and draft guards

`ReadOnlyCommand::downstream_actions` (`tui`) supplies contextual F1 actions through
the normal launcher; custom workspaces can register `DownstreamAction` directly.
Use public `scope_ide::tui` re-exports and register namespace/authorized dispatch
separately. Fixed literal presets have no expansion/interpolation.

Invocation rechecks pinned selected source identity/language/revision and **all
reported drafts**, including hidden/parked panes, before explicitly binding that
scratch and dispatching. This changes independent binding even if dispatch denies.
Workspace F4 downstream/composition/exact `script run` also reject dirty drafts.
Standalone shell/`WorkspaceCommandOutput` lack pane access: hosts must supply their
own draft boundary, and custom panes must report accurately. Transcript attribution
is historical, not semantic approval. Complete [W7b consumer and limits](DOWNSTREAM_WORKBENCH_ACTIONS.md).

Pure F5 Script (`supports_direct_script` / `invoke_script_direct`) is distinct from
authorized target-bound Script. Default workbench uses a fresh interpreter with
`NoHost`: no semantic/filesystem authority or persistent bindings. See [Script](scope-script.md).

## Execute a typed pipeline through Cell

### Try the working demonstration first

```sh
cargo run -p scope-execution --example text_pipeline -- 'Hello component world'
cargo test -p scope-types -p scope-execution
```

This runs real components compiled from WAT fixtures (no guest compiler/target):
uppercase text, word count `3`, dependency order `z-uppercase`, `a-count`.
Casing/word segmentation are ASCII, not Unicode. Complete
[host example](../crates/scope-execution/examples/text_pipeline.rs) and
[ABI/reference](../crates/scope-execution/README.md).

### Validate a downstream-owned transformation

The standalone [consumer test](../examples/downstream/tests/wasm_binding.rs) binds
its own byte-reversal component and `reverse-bytes` export with custom exact types,
including non-UTF-8 payloads and type-version rejection:

```sh
cargo test --locked --manifest-path examples/downstream/Cargo.toml --test wasm_binding
```

### Describe, bind, and execute

Declare a typed `Pipeline`, supply `Bindings` keyed by exact transformation
identity, external `Value`s and explicit host planning grants. Components must
implement the selected `Binding.export`, not necessarily `transform`, with ABI:

```text
func(list<tuple<string, string, string, list<u8>>>)
  -> list<tuple<string, string, string, list<u8>>>
```

Tuples are `(port, type name, type version, payload)`. This is a **component**, not
an arbitrary core module. All output ports/types must match exactly; versions are
opaque strings, not compatibility ranges. Host adapters/components agree on and
validate payload encodings; nominal tags do not validate semantics. Type-chain
discovery is not an optimizer: explicitly wire required same-type steps.

### Keep planning approval separate from runtime authority

Requirements are declarations; `granted` comes from host policy, never copied from
requirements. Install matching imports separately on the **same Cell** passed to
execution; grants cannot install imports or override capability denials. Full
graph/input/output/link/ABI preflight occurs before any guest starts. Nodes then
use fresh stores/instances in dependency order, publishing outputs only after
validation. Require empty `result.errors` and a present selected output. Runtime
failure retains earlier successful artifacts, not rollback of guest effects.

### Export and import portable compositions

`Manifest::from_pipeline(...).to_json()` exports a named result; `from_graph`
supports multiple results. JSON v1 preserves unresolved identities/incomplete wiring
for editing. Resolve against a trusted local `Registry`, grants and separate local
bindings; `with_implementations(bindings.keys())` adds binding-presence diagnostics.
`pipeline("result")` requires the whole resolution, including all outputs, to be
valid. Keep the original Manifest: resolved graphs omit unresolved declarations.

Import never registers declarations, downloads code or grants authority. Manifests
contain identities/wiring, not code, binding/host paths, grants or port/requirement
declarations. Names are opaque, not executable/path instructions. Unsupported
versions reject; unknown fields are ignored, not retained. Binding presence does
not prove ABI/runtime validity. See [complete manifest format and examples](../crates/scope-types/README.md#portable-composition-manifests).

## Managed component execution

Legacy `execute` remains unconfigured. `execute_with_policy` accepts Cell
`ExecutionPolicy`: fuel, per-linear-memory limits, cooperative deadline/cancellation
and recursive envelope input/output byte limits, without installing capabilities.
Fuel resets for startup and invocation separately in each node; an absolute deadline
spans nodes but is not a hard kill. Compilation/linking and arbitrary native host
calls cannot be forcibly interrupted. Payload limits follow construction/lifting,
not host allocation, and do not bound total process memory/aggregate artifacts.
Identity pipelines run no component and do not apply these Cell checks.

Handle `ExecutionError::Managed { node, phase, kind, message }` structurally, not
by parsing message text. New exhaustive matches need a `Managed` arm. Partial
artifacts remain failure evidence, not successful computation. See the complete
[policy example, phases and limitations](SE1_MANAGED_COMPONENT_EXECUTION.md).

## Checked-in standalone consumer

`examples/downstream` and `examples/harness` are separate Cargo workspaces, not
covered by root `--workspace`. The former demonstrates language/host and component
integration; the latter demonstrates model-independent tool policy (see [Agents](agents.md#standalone-scripted-harness)).

## Validate and extend

`cargo run -p xtask -- check-downstream --offline` tests/runs both consumers and a
freshly scaffolded runner without model calls. Offline requires cached dependencies;
generated crates use resolved versions/shared output, are removed on success and
retained on failure. This is not publication or portability validation. See
[Development](development.md#validation-commands) for the broader check list.

## 9. Checklist

- Test analysis, UTF-8/CRLF/EOF spans and stale-source rejection.
- Test registration and discovery separately from authorization/denial.
- Test dirty drafts, closed targets and candidate/acceptance/save separation.
- For components/logging, test missing grants, runtime denial and partial failures.
- Validate each standalone consumer against the intended Scope revision.

## Local downstream compatibility audit

Local sibling references are checkout-dependent, not pinned release contracts or
proof of current build compatibility. Key adoption risks from the read-only audit:

| Consumer | Reference / compatibility boundary |
| --- | --- |
| `../path` | `crates/path-scope`, `DX5B_RESULTS.md`: exact source/revision/preview validation and separate acceptance; candidate checks are not final committed-source kernel checks or saves. |
| `../hilb` | `src/launcher.rs`, `docs/UX1_SCOPE_API.md`: older closed-F1/missing-draft-guard findings predate W7b; lower-level callbacks still need host draft policy. Pure and authorized Script remain separate. |
| `../scope-forth` | `Cargo.toml`, semantic/Script adapters: absolute Scope paths and optional path-sourced Cell logging risk portability and distinct Rust source/type identities versus tagged Git Cell. |
| `../cross` | `Cargo.toml`, `src/discovery.rs`, X12 guide: inherits local Scope/Forth/Path paths and feature unification; exact nominal versions, ABI, payload validation and mathematical verification remain distinct. |

Account for required `analyze`, `Diagnostic::locations`, new outcome/event variants
and budget fields (`..Default::default()` where suitable). Local path contents are
not pinned by Cargo.lock. Retain exact source identities/revisions with evidence;
closed explicit targets fail without fallback and bindings never renew authority.
