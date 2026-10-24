# Source-aware evaluation

## Status and boundaries

The implemented core carries exact in-memory evaluation text and descriptive source identity; it does not provide a historical-source database, a navigation UI, filesystem authority, or an agent execution grant. This reference covers the source types, dispatch, ownership, and host retention obligations rather than proposing additional commands or host-handle APIs.

Evidence in this checkout:

- `crates/scope-language/src/source.rs`: source identity, revision, request, location, snapshot, and resolver contracts; publicly re-exported by `crates/scope-language/src/lib.rs`.
- `crates/scope-language/src/lib.rs`: `Diagnostic::locations` and diagnostic constructors.
- `crates/scope-language/src/interactive.rs`: optional request-aware dispatch with a text-only default.
- `crates/scope-ide/src/language/interactive.rs`: session ownership, request forwarding, routing, and reset/close behavior.
- `crates/scope-ide/src/document/mod.rs`, `crates/scope-ide/src/app/mod.rs`, and `crates/scope-ide/src/scratch.rs`: editable-text ownership and identities.
- `crates/scope-ide/src/cli.rs` and `crates/scope-terminal/lib.rs`: shell framing and buffered input acquisition.

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
