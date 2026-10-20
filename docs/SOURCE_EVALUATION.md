# Source-aware evaluation

## Status and boundaries

This document separates architecture verified in the source from proposed host work. The implemented core carries exact in-memory evaluation text and descriptive source identity; it does not provide a historical-source database, a navigation UI, filesystem authority, or an agent execution grant. The staged commands, typed host handles, and downstream adaptation below are proposals, not claims of implemented features.

Evidence in this checkout:

- `crates/scope-language/src/source.rs`: source identity, revision, request, location, snapshot, and resolver contracts; publicly re-exported by `crates/scope-language/src/lib.rs`.
- `crates/scope-language/src/lib.rs`: `Diagnostic::locations` and diagnostic constructors.
- `crates/scope-language/src/interactive.rs`: optional request-aware dispatch with a text-only default.
- `crates/scope-ide/src/language/interactive.rs`: session ownership, request forwarding, routing, and reset/close behavior.
- `crates/scope-ide/src/document/mod.rs`, `crates/scope-ide/src/app/mod.rs`, and `crates/scope-ide/src/scratch.rs`: editable-text ownership and identities.
- `crates/scope-ide/src/cli.rs` and `crates/scope-terminal/lib.rs`: shell framing and buffered input acquisition.

Downstream findings are based on read-only inspection of the complete `../scope-forth/src/interactive.rs`, `../scope-forth/src/interpreter.rs`, `../scope-forth/src/shell.rs`, `../scope-forth/GENERIC_SHELL_SESSION_DESIGN.md`, and `../scope-forth/tests/unified_buffers.rs`. No downstream changes are part of this document. Test assertions described below were inspected, not executed; downstream reports of earlier validation are not validation claims for this checkout. No builds or tests were run for this documentation change.

## Implemented public source types

All seven types are defined in `scope-language/src/source.rs` and available from the `scope_language` crate root:

| Type | Contract |
| --- | --- |
| `SourceId` | Opaque process-local identity backed by `Arc<u8>`. `fresh()` allocates a new identity; cloning preserves it. Equality and hashing use allocation identity, not text, paths, or numeric buffer IDs. It is not serializable or a filesystem capability. |
| `RevisionId` | Opaque `u64` wrapper made with `from_version`. Identifies a revision within one source, not a disk conflict token. Never associate different text with the same source/revision pair. |
| `EvaluationRequest<'a>` | Borrowed exact `text: &'a str`, an owned `source_id: SourceId`, and `revision_id: RevisionId`. Describes input; it does not transfer the buffer or authorize operations. |
| `SourceLocation` | `source_id`, `revision_id`, and `range: TextRange`. Uses the same coordinate convention as `Diagnostic::range`. |
| `SourceSnapshot` | Source/revision pair plus retained `text: Arc<str>`. Read-only historical text, not an editable buffer or a request to open a file. |
| `SourceUnavailable` | Explicit `UnknownOrExpired` result when an exact revision cannot be supplied. |
| `SourceResolver` | Host-owned `resolve(&self, source: &SourceId, revision: RevisionId) -> Result<SourceSnapshot, SourceUnavailable>`. Resolve only the exact requested pair. |

A resolver must not substitute current buffer text, another revision with equal-looking content, or text reread from disk. Unknown or evicted history is unavailable, not a reason to guess. A successful resolution grants no editing, file-opening, evaluation, filesystem, or network authority. Hosts choose retention limits, eviction, and privacy policy; the core defines no history store or retention duration. Keeping a `SourceId` alive keeps its identity allocation alive, not its former text.

Do not confuse `scope_language::SourceId` with `scope_ide::source_edit::SourceId` (the scratch/document edit-target enum), `DocumentId`, a shell buffer number, a disk revision string, or `scope_meta::SourceRef`. They serve different purposes and do not implicitly convert into one another. The public evaluation getter on both `Document` and `ScratchBuffer` is now `evaluation_source_id()`, not `source_id()`. This avoids shadowing the existing edit-identity abstraction's `source_id()` method with an incompatible evaluation-identity return type. In this checkout that abstraction is named `scope_ide::source_edit::SourceTarget` (the `EditableSource::source_id` concern in the requested terminology): its scratch implementation returns `SourceId::Scratch(self.id())`, and `DocumentSource` returns `SourceId::Document(document.id())`. Editing/proposal identity remains intact; evaluation identity is a separate descriptive identity, not a replacement.

## Diagnostics and compatibility

`Diagnostic` now has `locations: Vec<SourceLocation>`. Locations describe original sources, primary first; additional entries can represent related locations such as call sites. The existing `range`, `severity`, `message`, `source`, and `code` fields remain.

An empty locations vector uses the legacy presentation convention: `range` is presented relative to the evaluated text, but this supplies no proven original-source identity. In particular, it does not prove that a runtime error's span actually originated in the latest submission. `source: Some("forth")` is a diagnostic producer label, not provenance or permission to find a file. Explicit `locations`, when present, are authoritative for attribution; do not reinterpret their ranges against current input.

The current `cli.rs::print_evaluation` uses the first explicit location's start coordinates and prints the literal tag `[attributed source revision]`. With no locations it prints `diagnostic.range` without that tag. Both displays add one to line and column for presentation. The tagged branch does not resolve history, display an opaque source/revision identifier, or render additional locations; the tag deliberately avoids implying that its coordinates belong to the latest evaluated text.

**Compatibility caveat:** existing `Diagnostic` struct literals must now include `locations: vec![]` (or actual locations). This is a source-level breaking change for literals, despite the default-compatible trait method. `Diagnostic::error`, `warning`, `information`, and `hint` initialize an empty vector, so constructor-based callers do not need that literal update. A producer can construct a diagnostic normally and then populate `diagnostic.locations`.

For a historical error, derive the range from the retained original text and the original span, then attach that exact source/revision pair. Never apply a retained definition's byte offsets to the later calling submission or the current edited buffer. If provenance is not known, preserve that uncertainty rather than manufacture a location.

## Dispatch and ownership

`InteractiveLanguageSession::evaluate(&mut self, &str)` remains required. Its new `evaluate_request` default calls `self.evaluate(request.text)`. Old adapters therefore keep evaluating text, but silently discard context unless they override the method. Every wrapper in a request-aware dispatch chain must forward the request; overriding only an inner implementation is insufficient.

`scope_ide::language::InteractiveSession::evaluate(&str)` creates a fresh `SourceId` and revision zero for each call, then calls its own `evaluate_request`. Its request method forwards to the installed session while preserving existing reset-required handling. It does not retain source snapshots, change buffer selection, create a session, or authorize capabilities.

The controller owns the boxed session. `install` validates identity before replacement, closes the previous session before installing a valid replacement, and closes rejected candidates best effort. An old-session close failure leaves the old session selected but reset-required and discards the candidate; this is not rollback. `clear` detaches even if close fails. Successful reset clears the reset requirement; unsuccessful reset does not. Providers must also release resources through RAII. Source context changes none of these lifecycle rules.

### Editable buffers

- `Document` owns editable text, saved text, an in-memory version, disk baseline, and a stable evaluation `SourceId`. Construction creates a fresh source identity. Edits and revert advance the version; save records the baseline without changing the source identity. Clone preserves identity: clones are not automatically independent logical sources. Hosts must not let divergent clones reuse one source/revision pair for different text.
- `App` owns one optional open `Document`. `open_document()` and `open_document_mut()` expose the live buffer. `open_path` loads a new document with a fresh source identity even for the same path. `revert_open_document` restores the saved in-memory text; it is not a historical resolver or disk reload.
- `ScratchBuffers` owns shell-local buffers and selection. Numeric scratch IDs are monotonic within that owner, not global identities. Each created buffer gets a fresh source identity at version zero. Open selects an existing destination-bound buffer without rereading it, or creates a new clean buffer from the file. Selection does not change identity.
- Successful scratch edits advance the version even for identical replacement text. Reload advances the version and retains source identity. Binding, saving, promotion, and Save As do not create a new evaluation identity; persistence is separate from editing. Close/discard removes the live buffer; reopening creates a new source identity.
- `ScratchBuffer::evaluate(&self, &mut InteractiveSession)` checks the language match and sends the complete current text, cloned source identity, and `RevisionId::from_version(self.version())`. It does not route text as shell commands, save, reset, switch languages, or mutate the document.

The generic shell's scratch/workspace buffer collection is distinct from `App`'s optional document. Code must access the actual owning collection, not assume a shell-selected scratch buffer is `App::open_document()`. No editable-buffer ownership moves into the language runtime; an adapter that needs historical definitions must explicitly retain text and cloned identities.

### Lifecycle and host retention policy

Runtime availability and historical snapshot retention are separate decisions. The core mandates neither a history store nor snapshot cleanup at particular runtime transitions. Hosts must explicitly document retention scope, bounds, expiry, and visibility. Retaining snapshots never retains execution, editing, or filesystem authority.

| Event | Runtime/handle availability | Host-owned snapshot policy |
| --- | --- | --- |
| Successful reset | Same installation, fresh runtime state; invalidate state-dependent work. Forth's existing handle observes a new interpreter. | Host may retain or evict old revisions; reset does not mandate historical cleanup. |
| Successful replacement, including same-language replacement | Old session closed; callbacks bound to the old installation become unavailable, not rebound. | Host may preserve exact old snapshots for history or expire them explicitly. |
| Language switch | Successful switch is replacement; unrelated language callbacks are unavailable. Rejected/unsupported selection preserves the existing installation. | Retention need not follow language selection; visibility remains host-authorized. |
| Deselect/clear | Controller detaches session even if close fails; old installation callbacks unavailable. | Host chooses whether history survives deselection. No implicit new resolver authority. |
| Shell exit, EOF, or I/O error | Owned controller is dropped and closes best effort; callbacks cannot target a later shell. | Shell-local history may drop with its owner; an explicitly external host store may outlive it. No mandated persistence or cleanup. |
| Failed reset or partially effective close/replacement | Preserve controller failure/reset-required semantics; do not advertise fresh state or retarget stale callbacks. | Do not invent revisions or silently replace retained text; host policy still governs expiry. |

## Exact text versus shell framing

“Exact” means the text supplied to the evaluation API. Neither `EvaluationRequest` nor buffer evaluation trims, tokenizes, rejoins, normalizes line endings, or interprets `:scope` inside that text. Even text resembling a shell command is literal language input when evaluated as a buffer.

It does **not** mean every shell preserves the original input stream byte-for-byte:

- In `scope-ide/src/cli.rs`, noninteractive `BufRead::read_line` acquisition removes one trailing LF if present, then one trailing CR if present (including a lone trailing CR). The resulting line is the submission. Blank/whitespace-only lines are skipped. There is no automatic multiline pending-source assembly in this loop. Routing recognizes the explicit `:scope` namespace before evaluation; a source submission otherwise reaches `session.evaluate(source)` without trimming. Scratch capture copies the last submitted source string and language, not the original stream framing.
- `scope-terminal::read_from` removes a trailing LF and, only in that case, a preceding CR. A lone CR without LF remains. It returns an acquired `Outcome::Submitted` string; it is not the same normalization rule as the generic CLI loop above.
- Downstream `scope-forth/src/shell.rs` uses that terminal helper for buffered input, then additionally removes all trailing CR/LF characters with `trim_end_matches(['\r', '\n'])`. In Forth REPL mode it appends each acquired command to pending text followed by one LF. Internal pasted newlines survive, but trailing framing is reconstructed. Evaluation receives that assembled pending text, not the raw input bytes. Outside REPL mode multiline commands are rejected. REPL controls are handled before source assembly, and cancel/EOF discard pending input rather than evaluate it.

Thus an exact historical snapshot must preserve the final evaluated string. Capturing raw transport lines instead, joining lines with spaces, or reconstructing text later can invalidate spans and change line-comment behavior. Direct API and buffer callers can preserve CRLF, final-newline presence, and whitespace exactly because they bypass shell framing.

## Downstream findings: scope-forth

### `src/interactive.rs`

`ForthRuntimeHandle` holds an `Rc<RefCell<ForthInteractiveSession>>`; its installed wrapper and the provider-created session expose the same language contract through different construction paths. The handle offers read-only interpreter inspection; reset replaces the interpreter and close makes it absent. It is typed runtime access, not a general buffer handle or agent grant.

Neither `InstalledForthSession` nor `ForthInteractiveSession` overrides `evaluate_request`. Both therefore lose request provenance through the default text-only path. The inner evaluator enforces a 64 KiB submission limit, drains output, and converts execution errors to diagnostics. Its current error-kind heuristic suppresses several potentially historical ranges to `(0, 0)` because it cannot identify the original source. That is not historical-location support.

The existing `evaluate_buffer` bridge reads `App::open_document()`, checks buffer/session language agreement, and calls `session.evaluate(document.text())`. It evaluates unsaved in-memory text without saving or resetting, but does not yet pass the document's available source identity/version. Its text-only-protocol comment no longer describes what the upstream API can support; it still describes this downstream implementation's limitation.

### `src/interpreter.rs`

`Definition` already retains full original `Arc<str>` text, definition/body spans, and an optional `scope_meta::SourceRef` origin. Body spans refer to that retained text, not a later caller. `evaluate_source` retains a logical locator and opaque revision through that meta origin without loading a file.

`ExecutionError` contains only kind, word, and byte span, so it cannot by itself identify which submission or retained definition owns the span. Execution frames carry cloned definitions, providing a natural place to preserve original evaluation identity during nested calls. References resolve at call time; a name-only provenance map would be unsafe across redefinition and nested execution. Trace origin metadata is not a replacement for an exact language source/revision pair, and diagnostics should not require tracing to be enabled.

### `src/shell.rs`

This is a separate language-specific human shell, not the generic Scope shell. It uses `ForthSession`, calls `evaluate_repl` for assembled input and `evaluate_action("run")` for the document action, and has its own commands and pending-input policy. Merely upgrading the generic adapter will not automatically reroute these paths.

`forth source NAME` distinguishes static active-buffer inspection from runtime dictionary inspection. The runtime path labels retained source current only when the active document's path and complete text match the retained definition; otherwise it labels it historical. It displays retained text and does not read a locator from disk. This is useful existing behavior, but it is not an implementation of `scope_language::SourceResolver` or general diagnostic navigation.

### `GENERIC_SHELL_SESSION_DESIGN.md`: confirmed and corrected assumptions

The downstream design records inspection against an older local Scope commit. Confirmed by the inspected APIs and downstream implementation: `run_with_session` owns the supplied `App` and controller; generic REPL and explicit buffer evaluation share that controller; embedding and typed inspection need no downcast, singleton, replay, or second interpreter. Human host-command extension and general historical navigation are still proposals. Installing a session does not register tools or grant agent authority.

Correct its statements that the language contract accepts only text, source identity is unavailable upstream, and original-source diagnostic links are blocked by missing upstream fields: `EvaluationRequest`, `evaluation_source_id()`, and `Diagnostic::locations` now provide those fields. The downstream adapter still needs to forward and retain them, so generic historical parity is not established merely by the upstream API change. Also qualify its ownership table's “host-created Scope App” buffer description: the generic runner creates and uses its own `ScratchBuffers` for selected shell workspace/scratch text, distinct from `App::open_document()`. Its retirement gates remain relevant for migrating semantic review, trace commands, and compatibility-shell behavior; source fields alone do not retire the legacy shell.

### `tests/unified_buffers.rs`: inspected assertions, not execution results

The installed-runtime test uses real `InteractiveSession::install` and `cli::run_with_session` APIs. Its assertions cover shared stack/dictionary across initial REPL evaluation, scratch-buffer evaluation, and later REPL input; rejected identity closes only the candidate; unsupported selection preserves the active session; EOF makes the original typed handle report absent; and a separately constructed runtime stays isolated. Another test asserts reset clears the observed runtime, same-language replacement closes the old handle's runtime, and clear deselects the controller.

The binary-driven tests assert unsaved edited source is evaluated, reopening an already-open path selects its retained shell buffer rather than rereading disk, reset preserves source buffers, cancelled drafts are not committed, failed evaluation can retain earlier definitions, dirty close refuses, and save is explicit and nonexecuting. These assertions confirm the intended ownership and dispatch assumptions in the design, but do not assert request source/revision pairs, original-definition `locations`, historical resolution after edits, a namespaced host-command registry, or installation-generation callback guards. Reading them is not evidence that they pass against the current checkout.

## Minimal illustrative downstream adaptation (not applied)

Keep the change small and preserve both adapter construction paths:

1. Add an `evaluate_request` override to `InstalledForthSession` that forwards the complete request to the inner session, not just its text:

   ```rust,ignore
   fn evaluate_request(
       &mut self,
       request: scope_language::EvaluationRequest<'_>,
   ) -> Result<EvaluationResult, SessionError> {
       self.0.0.borrow_mut().evaluate_request(request)
   }
   ```

2. Add the corresponding inner override. Retain a `SourceSnapshot` containing `request.source_id.clone()`, `request.revision_id`, and `Arc::from(request.text)` wherever definitions need source ownership. Extend definition/frame/error provenance to carry the original pair alongside existing retained text and spans. Keep legacy `evaluate` working without inventing provenance for old definitions. This requires interpreter plumbing; simply setting every diagnostic's location to the current request would be incorrect.
3. Change the buffer bridge to dispatch the already-owned document identity and version:

   ```rust,ignore
   session.evaluate_request(scope_language::EvaluationRequest {
       text: document.text(),
       source_id: document.evaluation_source_id().clone(),
       revision_id: scope_language::RevisionId::from_version(document.version()),
   })
   ```

4. When an error has an original snapshot and validated original span, build its diagnostic from those, not from the caller's text. This illustrative helper uses only the public source types; selecting the correct snapshot is the interpreter's responsibility:

   ```rust,ignore
   fn historical_diagnostic(
       snapshot: &scope_language::SourceSnapshot,
       span: std::ops::Range<usize>,
       message: &str,
   ) -> Option<scope_language::Diagnostic> {
       let range = scope_language::LineIndex::new(&snapshot.text)
           .range_for_span(span)?;
       let mut diagnostic = scope_language::Diagnostic::error(range, message)
           .with_source("forth");
       diagnostic.locations.push(scope_language::SourceLocation {
           source_id: snapshot.source_id.clone(),
           revision_id: snapshot.revision_id,
           range,
       });
       Some(diagnostic)
   }
   ```

For example, evaluate `: broken 1 . mystery ;` as source A/revision 0, then call `broken` from source B. The `mystery` error must locate A/revision 0 using A's retained text, even after A is edited, closed, or rebound to a destination. If call-site provenance is also retained, B can be an additional related location, not a replacement for A. Without valid provenance, emit a legacy/unlocated diagnostic rather than guess. Host snapshot retention and interpreter definition retention may have different lifetimes; an emitted location can subsequently resolve to `UnknownOrExpired`.

These fragments illustrate the boundary, not a complete interpreter patch. They do not install a resolver, add history retention, modify downstream files, or promise historical navigation today.

## Proposed staged human and host surface

The following is a concrete sequencing proposal. Existing generic commands such as `:scope buffer eval`, `:scope buffers`, and `:scope buffer <id>` remain existing behavior; the `source` commands and host-handle API below are new proposals.

### Stage 1: explicit human evaluation

Use `:scope buffer eval` for complete selected in-memory buffer evaluation in the existing matching active session. Do not save, create/select a runtime, reset it, or invoke commands found inside the buffer. Keep language mismatch and no-session failures explicit. Use `:scope session <language-id>` separately when the human wants a session transition. Preserve `:scope` namespacing so source words cannot accidentally invoke host commands.

Add `:scope source status` to report whether exact-revision retention is configured, its bounds, and unavailable/expired state without creating a store implicitly. Human execution is an explicit action; background analysis, completion, source lookup, and opening a document must remain nonexecuting.

### Stage 2: typed host handles and stale guards

Introduce host-local distinct types, for example `HostBufferHandle` and `HostSessionHandle`, rather than accepting raw integer IDs, paths, or model-authored locators as authority. A handle should bind the host/controller owner, object identity, and lifecycle generation. Keep these types separate from descriptive `SourceId`/`RevisionId`.

- Resolve a document handle through the live owning `App` and its `open_document()` accessor; verify source identity and expected version immediately before borrowing/evaluating. A reopened document must not satisfy a stale handle merely because the path or numeric ID matches.
- Resolve scratch handles through the owning `ScratchBuffers::get`, without silently selecting another buffer. Check owner and generation as well as the numeric ID.
- Bind runtime handles to a controller installation generation. Replacement, clear, close, and controller destruction invalidate old handles. Treat successful reset as a new runtime-state generation for operations that depend on retained definitions; reject stale work rather than retarget it. Failed or partially effective transitions must not make old work appear fresh.
- For delayed operations carry expected buffer revision and runtime generation, then revalidate both at dispatch. Check language match too. A descriptive request is not itself a stale-operation guard.
- Expose narrow operations such as read snapshot, inspect runtime, and explicitly authorized evaluate, not unrestricted mutable `App` or interpreter access. Borrow live text only for synchronous dispatch, or retain an immutable exact snapshot with its pair for queued work. Never keep references into a mutable buffer across edits.

The current controller does not expose this generation-based host-handle API. The downstream read-only runtime handle is a useful precedent, but does not implement these stale guards by itself.

### Illustrative human-command extension signatures

The following is a proposed API shape, not existing exported Rust or a compilable patch. Supporting handle, lifecycle, and error types would be defined by the extension. `run_with_session` should remain a compatibility entry point using an empty registry.

```rust,ignore
pub trait HumanCommand {
    fn namespace(&self) -> &'static str; // e.g. "forth", registered as :forth
    fn run(
        &mut self,
        args: &str,
        context: &HumanCommandContext<'_>,
        output: &mut dyn std::io::Write,
    ) -> Result<(), HumanCommandError>;
}

pub struct HumanCommandContext<'a> {
    pub app: &'a scope_ide::app::App,
    pub buffers: &'a scope_ide::scratch::ScratchBuffers,
    pub active: Option<ActiveInstallation>, // identity, language, generation
    pub runtime_state_generation: RuntimeStateGeneration,
    pub sources: Option<&'a dyn scope_language::SourceResolver>,
}

pub struct HumanCommandRegistry { /* namespaced human callbacks only */ }
impl HumanCommandRegistry {
    pub fn register(
        &mut self,
        binding: InstallationBinding,
        command: impl HumanCommand + 'static,
    ) -> Result<(), HumanCommandError>;
}

pub fn run_with_host(
    app: scope_ide::app::App,
    session: scope_ide::language::InteractiveSession,
    commands: HumanCommandRegistry,
    input: impl std::io::BufRead,
    output: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>>;
```

The runner must lend `context.buffers` from the **actual** `ScratchBuffers` created inside its shell loop, not create a parallel collection. `app` separately exposes the actual App document through `open_document()` when a command explicitly needs it. Shared borrows permit source inspection, not arbitrary buffer mutation or direct controller replacement. Do not expose `&mut App`, `&mut ScratchBuffers`, or `&mut InteractiveSession` as a blanket command capability. Future mutation requires a narrow host-approved operation with stale checks.

A concrete typed callback binding can retain a `ForthRuntimeHandle` in a `ForthCommands` implementation and register it with an `InstallationBinding { installation_id, installation_generation, language_id }` issued for the installed adapter. These proposed binding fields are not a language-name-only lookup. Before invoking the callback, the runner compares the binding with the active controller installation; mismatches fail unavailable/stale. The callback then uses the captured typed handle's read-only `inspect`, without downcasting or re-entering the controller. A successful language switch/replacement makes the old callback unavailable even if the new language has the same ID. Switching back requires a new adapter/handle and binding. Unsupported selection leaves the old callback available. Reset preserves installation binding and makes ordinary stack/dictionary inspection see the fresh runtime, while invalidating cached definition/trace work through the separate runtime-state generation. Failed reset leaves state-dependent commands unavailable until recovery; never claim fresh state merely because reset was attempted.

Dispatch order should be: recognize built-in `:scope` commands; recognize reserved registered host namespaces such as `:forth`; only then route remaining input to language source. Thus `:forth stack`, `:forth dictionary`, `:forth trace show`, and `:forth source NAME` are human commands even in Forth evaluation mode. Unknown subcommands or an unavailable/stale `:forth` binding produce command errors, never fall through to source evaluation. Match namespace boundaries (`:forth` or `:forth `), not arbitrary prefixes such as `:forthish`. Raw buffer evaluation bypasses this shell dispatch, so a buffer containing `:forth stack` is still literal source. Runtime mutation such as trace configuration, if added, needs a separately narrow typed operation and lifecycle checks rather than turning read-only `inspect` into mutable interpreter access.

This registry has no `ToolProvider` conversion and performs no tool registration, discovery publication, or agent dispatch. It is a human-shell seam only.

### Stage 3: separate agent authority

Do not register human evaluation commands as agent tools automatically. Session installation, language registration, possession of a source ID, and source resolution do not grant agent execution. A future agent evaluate operation needs a separate explicit host policy, selected runtime, bounded input/output/steps, and stale checks. Inspection and source resolution remain read-only; proposal generation, human acceptance, saving, and runtime execution are distinct authorities. An agent-provided command string must never bypass typed dispatch into the human command parser.

### Stage 4: read-only historical navigation

Propose `:scope source show <diagnostic-id> [location-index]` to resolve the selected explicit location, and `:scope source where <diagnostic-id>` to list known locations and availability. Diagnostic IDs should be host-local handles into retained evaluation results, not serialized `SourceId` pointers. Human output must distinguish an exact current revision from a historical read-only snapshot.

The host consults its authorized `SourceResolver`; failure prints an explicit unavailable/expired result. A historical viewer must not replace the active buffer, open a filesystem path, save, evaluate, or reconstruct history from current disk contents. If users later want editable historical text, require a separate explicit copy-to-new-buffer operation with a fresh source identity. Do not turn source navigation into a way to revive stale editable-buffer handles or acquire agent authority.
