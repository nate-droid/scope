# T2A — Explicit source context

T2A exposes the existing source owners and their command targets without merging them. It supplies a borrowed, read-only resolver for an explicitly requested owner and an observational shell report. It does **not** implement a new source-selection command, change T1 inspection routing, install an agent semantic provider, or create a source store or revision scheme.

This records the implementation in `crates/scope-ide/src/source_context.rs`, `crates/scope-ide/src/cli/context.rs`, `crates/scope-ide/src/cli.rs`, and the proposal labels in `crates/scope-ide/src/cli/source_proposals.rs`. It follows [T1 shell semantic inspection](T1_SHELL_SEMANTIC_INSPECTION.md) and the source/session and agent-composition boundaries in [Terminal-first interaction](TERMINAL_FIRST_INTERACTION.md). The broader T2 selection adapter proposed by T1 is not completed by this observational slice.

## Actual commands and routing

```text
:scope context
context
```

`context` accepts no arguments; `context extra` is rejected. The namespaced form works with or without an interactive language session. The bare form is a Scope command only without a session; with a session it is language source under the existing routing rules. Context appears in ordinary command completion and help; no new input mode is introduced.

The report shows:

- App document and selected shell buffer independently, including tagged identity, owner version, path/destination, dirty state, buffer name, and path-resolved semantic service;
- the independent interactive session language;
- whether both distinct source owners exist;
- current inspection, agent document-context, agent-edit, source-proposal, workspace-proposal, and buffer-evaluation routing;
- feature-disabled, evaluation-language-mismatch and session-reset-required explanations.

The report ends by stating that it is observational: no execution, renewed authorization, or freshness reset. A displayed service or target is not proof that an operation is eligible, configured, authorized, or available through an agent.

Existing owner-specific commands remain unchanged:

| Command or workflow | Actual owner / target |
| --- | --- |
| Bare `open`, `show`, `replace`, `save`, `revert`, `language`, `outline`, `diagnostics` without a session | App-owned open document. Bare `open` without a path uses the App's selected file. |
| `:scope open`, `:scope buffer <id>`, `:scope show`, `:scope edit`, `:scope save`, and scratch commands | Separate shell-owned `ScratchBuffers`; workspace buffers also use scratch identities. Selecting one does not open an App document. |
| `:scope inspect ...` | T1 App document only; requires `tools`, eligible operations and captured-request confirmation. |
| Ordinary `agent <request>` | Existing App document context plus separately available workspace tools; requires `agent`. This is not selected-buffer semantic composition. |
| `:scope agent edit "instruction"` | Existing shell-buffer edit workflow, subject to host configuration and authorization; not the App document or T1 inspection. |
| `source-propose <JSON>` | Selected shell buffer at proposal creation. |
| `source-proposal`, `source-diff`, `source-accept`, `source-reject` | Retained proposal identity and baseline, not a newly selected source. Acceptance revalidates and applies in memory; it does not save or evaluate. |
| `proposal`, `proposal-agent`, `proposal-review`, `proposal-accept`, `proposal-discard` | Separate isolated approved-path workspace snapshot, not either in-memory source. |
| `:scope buffer eval` / `:scope scratch eval` | Selected buffer's in-memory source, only with a matching active session; never switches languages automatically. |

With a session active, unprefixed input is language source. To use bare App `open`, first issue `:scope session none`. In particular, `:scope open` is **not** the namespaced equivalent of App `open`. T2A adds no `context select`, owner-switching command, or inspection-target override. Existing buffer selection commands still select only within the buffer owner.

## Explicit resolution, not owner precedence

`SourceContext::resolve(app, buffers, kind)` requires a `SourceKind` supplied by its caller:

| Requested kind | Result |
| --- | --- |
| `SourceKind::None` | `Ok(SourceContext::None)`, even when both owners have sources. |
| `SourceKind::App` | App context if a document is open; otherwise `SourceContextError::NoAppDocument` (`no App document is open`). |
| `SourceKind::Buffer` | Selected buffer context; otherwise `SourceContextError::NoSelectedBuffer` (`no buffer is selected`). |

A missing requested owner never falls back to the other. Resolution neither selects a buffer nor opens a document. `None` is an explicit absence of source, not an invitation to choose one.

`HostSourceContext::derive(app, buffers, session)` aggregates both independently resolved owners as optional contexts and borrows the independent `InteractiveSession`. It converts missing-owner resolution errors into absent fields for display. It does not designate an authoritative or preferred target. `targets_differ()` is true exactly when both owners exist: their ownership differs regardless of equal paths, text, or numeric versions. With either owner absent it is false, not a claim of source equality.

## Borrowed state, identity and versions

`SourceContext<'a>` is a copyable descriptor containing `None`, a borrowed `App`, or a borrowed `ScratchBuffer` plus the App's language registry. Its accessors expose existing document/buffer references, text, path, dirty state, identity, version, semantic service and source interactive-language metadata. It owns no mutable source copy, controller, authorization, retained result, or revision counter. Derive it again after host transitions; it is not a durable captured binding.

The identity is `source_edit::SourceId::Document(DocumentId)` or `source_edit::SourceId::Scratch(u64)`. IDs are local to their owning App/buffer collection, not globally unique handles. Equal numeric IDs across variants or hosts do not identify the same source. Owner versions are used as supplied, not compared across owners or replaced with a global sequence. These edit identities are not evaluator `scope_language::SourceId` / `RevisionId`, another document abstraction's revision, or a disk persistence guard.

A path is display, destination and language-resolution context only. It grants no filesystem authority and cannot substitute for tagged identity. App and buffer can share a path while holding different unsaved text and versions. Reopening the App path creates a new document identity; text or path equality must not revive a prior binding. Existing inspection freshness and proposal preconditions remain authoritative. Reading context does not refresh either.

For App sources, semantic service resolution delegates to `App::semantic_service_for_document`. For buffers it uses the destination path and existing registry path selection, including neutral fallback. A pathless scratch has no semantic service through this accessor even when it has an interactive language ID. `interactive_language()` describes the source's interactive language, not the active session and not a promise that an evaluator provider is registered. A neutral semantic fallback is not an interactive evaluator.

## Examples

These are illustrative command sequences, not transcripts of validation performed for this document. `main.scope` denotes an already existing workspace file; exact allocated IDs and versions depend on prior shell actions.

### Two owners for the same path

Start without an interactive session:

```text
context
:scope open main.scope
:scope context
open main.scope
replace 1 1 1 1 app
:scope scratch replace 1 1 1 1 buffer
:scope context
```

Initially neither owner has a source. Namespaced open creates/selects only the shell buffer, so the App field remains `none`. Bare open then establishes a separate App document. The replacements insert distinct unsaved prefixes in the two owners; neither saves the file. Context now reports both identities, their independent versions and dirty states, and `App and buffer targets differ: true (ownership, not path equality)`. Inspection remains App-bound; source proposal creation remains buffer-bound.

```text
open main.scope
:scope context
:scope discard
:scope context
```

Reopening replaces App identity rather than transferring buffer edits. Discard explicitly drops the selected buffer, including its unsaved edits, without changing the App document or saving. Context observes those transitions without changing them.

### Session independence

```text
:scope session
:scope context
:scope session none
open main.scope
:scope inspect
```

The first command reports the active session. Context reports whether buffer evaluation is unavailable because no session exists or because buffer/session languages differ. Clearing the session enables bare App commands; it does not select a source or install semantic operations. Inspection can honestly report no eligible operations for the registered service.

### Proposal labels retain the baseline

Source-proposal output uses an owner-tagged label, for example:

```text
target: owner=shell-buffer source=Scratch(7) version=42 (proposal baseline; path is destination context only)
```

The formatter also supports document targets as `owner=app-document source=Document(7) version=3`. These are examples of label formatting, not globally meaningful IDs or proof that shell proposal creation targets App documents. The version is the proposal's supplied expected version, not the current selection's version. Selecting another buffer must not relabel or retarget a retained proposal. Review and acceptance continue through existing controller validation; owner/path display is not acceptance authority.

## Authorization and future source-bound agent composition

T2A installs **no** `SemanticToolProvider` in the normal terminal agent. `context` printing an agent document context or edit source describes existing routing and prerequisites, not a new provider registration or permission grant. Human T1 confirmation remains scoped to its captured invocation and grants no agent authority.

A future opt-in agent bind should consume an explicit owner choice, not infer authority from `HostSourceContext`, path equality, active session, highlighted file, or whichever source happens to exist:

1. Resolve the requested `SourceKind` and fail explicitly if it is missing. For an initial document-only bridge, request `App` explicitly and reject unsupported buffer scope rather than substituting App for Buffer.
2. Borrow the actual owner's live in-memory source through the existing source-target adapter (for App, `DocumentSource`). Bind tagged identity, exact text, version, language and path for the run; do not reread disk or make a new mutable authoritative source copy. Re-derive context after transitions, not during a supposedly stable binding as a way to silently retarget it.
3. Resolve and verify the registered semantic service separately from the evaluator session. Keep the source stable for the provider lifetime and preserve pre/post-invocation checks. Use existing owner-specific permitted-source scopes; a buffer choice must not accidentally receive document or workspace authority.
4. Explicitly compose only intended providers through the existing agent builder, rejecting name collisions. Start with eligible read operations and a host-owned isolated temporary controller. A later proposing flow needs a separately retained controller and release of agent/provider borrows before human review.
5. Require both outer agent dispatch approval and semantic authorization keyed to operation, arguments, effect/purpose, source and scope. Generic `read_only_policy` over advertised tool effects alone is insufficient semantic approval. Exclude ungranted operations from the exposed projection and retain deny enforcement; hiding alone is not authorization.
6. Preserve native structured statuses/results within existing agent budgets. Test deterministic model/tool round trips and address the documented float/large-unsigned transport gap before claiming arbitrary native JSON support. A source context does not solve transport fidelity.

Read metadata is a trusted native provider declaration, not an OS sandbox or guarantee against external effects. Source context adds no filesystem/network capability, cancellation, billing bound, save permission, proposal permission, or evaluator permission. Accept, persist and evaluate remain separate decisions.

## Limitations and next minimal integration

Current limitations are deliberate: observational aggregate only; no persistent owner selection; T1 remains App-only, synchronous, argument-free and read-only; active sessions still require clearing before bare App open; pathless buffers do not acquire semantic services from language IDs; context does not capture catalogs or authorization and does not invalidate/revalidate retained results itself. Feature labels report compile-time availability, not runtime provider installation or successful authorization. Existing save/conflict and workspace-proposal non-transactionality limitations remain unchanged.

The next minimal integration is a single opt-in, explicitly App-bound read-only terminal-agent composition path using this resolver, existing source adapters, explicit document scope and separate agent/semantic policy. Keep buffer-aware semantic execution and any user-facing owner-selection vocabulary as separate, tested extensions. Do not add panes, a new source store, revisions, automatic provider registration, proposal acceptance, or source/session switching to make the report appear unified.

Required integration coverage should include missing requested owner with the other present, explicit None with both present, same-path/different-owner text and versions, reopening identity, restored-text staleness, session mismatch, denied policy with zero dispatch, scope mismatch, provider collisions, unchanged source/disk, and retained proposal baselines after buffer selection changes.

## Validation status

Implementation inspection found focused tests in `source_context.rs` for explicit resolution/observational aggregation, owner transitions, same-path distinct identity/text/versions without mutation, and neutral semantics versus interactive language. `cli/context.rs` contains parser and actual shell context-transition coverage; proposal-label tests check tagged ownership and supplied versions. Their presence is not a claim that they passed.

Automated validation passed (all Cargo commands used `--offline`):

- `cargo fmt --all -- --check` and `git --no-pager diff --check`.
- `cargo test --offline -p scope-ide` and the full `--no-default-features` suite.
- `cargo test --offline -p scope-ide --features agent,tui --test shell_semantic_inspection --test embedded_shell --test source_edit_controller --test interactive_semantic_inspection`.
- `cargo test --offline -p scope-ide --features agent,tui --test agent_public_api` (2 tests).
- T1's unchanged `shell_inspection_walkthrough -- --nocapture` test; no evaluator/model/credentials/network required.
- `cargo test --offline -p scope-language -p scope-terminal` and `cargo test --offline -p scope-script` (including existing terminal/PTY regressions).
- `cargo clippy --offline -p scope-ide --lib --test shell_semantic_inspection --test embedded_shell -- -D warnings`.

New tests cover explicit resolution failures, no source, App-only/buffer-only/both owners, same-path divergent unsaved content, independent versions, new identity on reopen, buffer switching/discard, observational repeated context, actual parser/session routing, evaluation feedback and reset blocking, retained proposal-baseline labels, and stale inspection remaining stale across context reports. Existing inspection/proposal/source-edit tests remain regression coverage. No manual terminal walkthrough was performed.

Known combined-feature failures were rerun and remain separate from passing results:

- `cargo test --offline -p scope-ide --features agent,tui --lib`: 8 existing compilation errors in `intent_tests.rs` and `edit_lifecycle.rs`, involving ambiguous evaluator/edit `SourceId`, incompatible `SourceTarget::source_id`, and nonexistent evaluator `SourceId::Scratch`.
- `cargo clippy --offline -p scope-ide --features agent,tui --lib -- -D warnings`: 6 existing findings: argument counts at `edit_lifecycle.rs:597,1234,1256` and the unchanged `run_shell_owned` signature, plus nested-if findings at `edit_lifecycle.rs:724,1075`. No unrelated fixes or allowances were added.
