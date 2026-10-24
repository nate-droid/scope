# T1 — Read-only semantic inspection from the shell

Implemented 2026-10-22. T1 adds a command-stream presentation of the existing SIO1 inspection bridge. It does not add an evaluator, operation registry, dispatcher, agent permission, source owner, revision model, or contextual view.

## Actual commands

```text
:scope inspect
:scope inspect operation ID
:scope inspect operation "JSON-quoted operation ID"
:scope inspect confirm TOKEN
:scope inspect cancel
:scope inspect result
:scope inspect result --full
```

- `inspect` discovers eligible operations, without executing them.
- `inspect operation ID` captures a request and displays its review and a confirmation token, without executing it. A new capture replaces the old pending capture. A failed capture also clears the older pending request.
- `inspect confirm TOKEN` explicitly authorizes the matching pending request once. The token is a shell-local request handle, not a capability, revision, or reusable authorization grant. A mismatched token does not authorize or dispatch; the matching pending request remains available. A matching token consumes the request even when execution fails. Replays report no pending inspection.
- `inspect cancel` drops the pending request without source changes; it does not delete the last completed result.
- `inspect result` displays the last confirmed invocation's summary and current freshness. `--full` displays its complete structured feedback. The retrieval handle is **last result**, not a growing result registry. The next matching confirmation replaces it, including a failed or stale invocation. Discovery, capture and cancellation do not replace it.

The `operation` subcommand disambiguates operation IDs from shell controls. Operations named `result`, `confirm`, `cancel`, or `operation` are not reserved. Bare IDs must be one whitespace-delimited token; JSON-string syntax handles whitespace, quotes, backslashes and controls using the existing serde JSON parser. This accepts an operation **identifier**, not operation arguments or arbitrary JSON requests. Discovery prints complete JSON-quoted IDs so they can be copied into the capture command without lossy abbreviation.

`LanguageAction` remains separate from `SemanticOperationDescription`. Listed language actions are not automatically invokable through `inspect`.

## Exact command routing and input

The existing `InteractiveSession::route` and `ShellCommand` parser handle these commands. With **no active language session**, both `inspect ...` and `:scope inspect ...` work. With a language session active, only the namespaced form is a Scope command; bare `inspect confirm 1`, `yes`, etc. are ordinary language source. There is no global confirmation reader or new input mode.

Namespace recognition is unchanged: `:scope` or `:scope ` at the beginning of the submitted input. Leading whitespace before the namespace or a tab in place of that separating ASCII space does not create a new routing rule.

The command declaration supplies ordinary name completion. The existing completion function additionally offers `operation`, `confirm`, `cancel`, `result`, and `result --full`. Operation IDs are obtained from explicit discovery, not a second registry or speculative provider execution during completion. Existing history, multiline editing, completeness checks, paste policy and redirected-input behavior remain in place. Confirmation commands are ordinary history entries under existing interactive history policy; recalling one cannot bypass the pending-token and snapshot checks.

The existing `tools` feature is now enabled by default so ordinary CLI builds contain the semantic bridge. This feature adds no new dependency and does not enable `agent` or `tui`. A deliberately minimal `--no-default-features` build still compiles and runs the legacy shell; inspection reports that `tools` is required. Embeddings that disable default features must explicitly enable `scope-ide/tools` to use inspection.

## Source owner, identity and language

T1 always targets the **App-owned open document**. It never inspects the highlighted file, selected scratch buffer, saved-disk copy, or active evaluator submission in its place.

Discovery, capture and invocation output identify:

- App document ownership;
- the existing tagged `source_edit::SourceId::Document(DocumentId)`;
- document path;
- document version;
- the selected registered semantic service's language ID (discovery/capture also show its name).

If a shell buffer is selected, output says that buffer is **not the inspection target**, even when its destination matches the App document's path. Its contents/version are not substituted. Results retain their original display path; opening another source does not relabel old feedback with the new path.

Open App source with the existing **bare** command:

```text
open relative/path
```

No document produces an actionable diagnostic with that command. If an evaluator is active, first use `:scope session none`, then bare `open relative/path`. This is an acknowledged existing limitation, not a T1 source-selection redesign. `:scope open relative/path` still opens/selects a separate shell workspace buffer and does not establish the App inspection target. Neither command's behavior changed.

The service is resolved using `App::semantic_service_for_document`: the first registered path-matching language service, with existing neutral fallback. It is not selected from the active interactive session's language ID. Stock language services may advertise no eligible operations; the command reports that honestly rather than inventing operations.

## Eligibility and preserved semantic boundaries

T1 uses `semantic_inspection::discover_app` and `InspectionBinding::capture_app/invoke_app`. Eligibility is exactly the existing SIO1 subset:

- `SemanticOperationEffect::Read`;
- `SemanticTargetRequirement::Document`;
- no declared arguments, including optional arguments;
- no selection supplied or required;
- nonempty unique operation IDs throughout the catalog.

Unsupported operations are omitted by discovery and rejected on capture. Invalid catalog identities fail closed, including invalid entries otherwise excluded by effect. T1 adds no argument forms, selection construction, `ProposeChanges` tools, or edit acceptance.

`InspectionBinding` retains the exact identity, source text, version, language and complete operation descriptor. Invocation rechecks source and current descriptor/catalog, then uses `DocumentSource`, `SemanticToolProvider`, the existing permitted-document scope, empty arguments and the existing `SemanticAuthorization` callback. The provider retains its existing argument validation and pre/post-invocation source checks. The shell does not call `perform_semantic_operation` directly.

## Confirmation and authority

Capture prints the operation description, source binding, `Read / Document / no selection / arguments {}`, and exact scoped confirmation command. It does not execute the operation. A matching explicit confirmation invokes the existing bridge with approval for that call only; the bridge checks its captured fields before granting provider authorization. Cancellation, denial, invalid operations and stale confirmation dispatch nothing.

Example, with an embedding-registered operation `fixture.read`:

```text
scope> inspect
App document Document(DocumentId(1)) / path sample.fixture / version 0 / language fixture (...)
"fixture.read" — Read source [Read / Document / arguments {}]
...
scope> inspect operation fixture.read
Captured request 1: App document ... / binding CURRENT
...
Authorize once: :scope inspect confirm 1; cancel: :scope inspect cancel
scope> inspect confirm 1
Last inspection result: App document ... / binding CURRENT
Transport: ok; tool projection status: read_only; native read: read_only (authoritative observation)
...
Retrieve last result: :scope inspect result --full ...
```

This excerpt illustrates the implemented syntax/output structure; `fixture.read` is a test operation, **not a stock language feature**. Tokens are allocated per successful capture within that shell and never reused there. EOF simply drops host-owned pending/result display state.

Human confirmation establishes no agent authorization or provider composition. The normal terminal agent is unchanged. Native `Read` is a trusted provider declaration, **not an OS sandbox, cancellation guarantee or proof of absence of external effects**. Do not register untrusted native language services expecting command confirmation to sandbox them.

Read inspection supplies the bridge's isolated temporary edit controller, as before. It exposes no proposal registration/acceptance workflow. The validated dispatcher rejects change results from read operations. Inspection does not advance the document version, dirty source, save, evaluate, or install candidate definitions.

## Native results and terminal-safe presentation

Ordinary result output contains the source/operation binding, transport status, tool-projection status, a compact feedback preview limited to 240 rendered bytes plus an ellipsis, and the explicit last-result retrieval command. It does not dump long native values by default. Metadata descriptions are bounded separately. Explicit `--full` is intentionally unabridged and can be large.

Full feedback has two distinct fields:

- `native`: the existing validated native read observation, including the original nested JSON value and rationale; existing read-evaluation variants also retain their evaluation observations;
- `transport`: existing structured tool feedback, including semantic status and detailed errors/blockers. A bridge-level error is represented as `error` instead.

This is a display projection, not a second provider protocol. Native JSON is not converted through `ToolValue`. Floats and unsigned integers above `i64::MAX` remain intact. The existing agent-only projection can report `internal_failure` for such numbers even after a native read succeeds: output explicitly calls that **tool projection status**, and separately identifies the native read as the authoritative observation. T1 does not modify agent transport or coerce numeric values.

Blocked feedback and operation errors retain their structured fields. A native domain value such as `{"accepted":false,"status":"domain-rejected"}` remains domain feedback; it is not recast as transport failure or Scope validation evidence. Every result is labeled **not a Scope validation certificate**. No arbitrary `checked`, `passed`, or `valid` field authorizes edits or becomes a diagnostics overlay.

All provider-controlled displayed text is escaped before terminal output. Full JSON uses valid JSON escapes for remaining C1/DEL/bidirectional/line-separator controls as well as serde's C0 escaping, preserving parseable original values without ANSI injection. Metadata and concise previews use the shell's existing bounded terminal escaping. Host formatting newlines are not provider control bytes.

## Freshness

The shell retains the original `InspectionBinding` and path with pending requests/results. At command-loop boundaries and inspection commands it observes source identity/text/version/language and descriptor/catalog equality through the existing capture API. This is metadata inspection, **not operation execution**. Once an observed binding becomes stale it stays stale. Confirmation also rechecks through the existing bridge immediately before dispatch.

- Unsaved App edits are used for fresh invocation, not disk text.
- Editing and then restoring text does not restore the old version or authorization.
- Reopening the same path, even with identical text, changes identity and invalidates prior bindings.
- A changed descriptor or invalid catalog invalidates a pending confirmation and marks retained feedback stale.
- Old feedback remains available with `STALE`, but is not automatically rechecked.
- A fresh capture and confirmation are required for a new `CURRENT` observation.

The current/stale label means freshness of the retained binding, not semantic acceptance, a renewed authorization grant or a new revision scheme. Existing document versions/identities remain the only source preconditions.

## Runnable deterministic walkthrough and tests

The executable demonstration is an integration test in `crates/scope-ide/tests/shell_semantic_inspection.rs`:

```sh
CARGO_INCREMENTAL=0 cargo test --offline -p scope-ide \
  --test shell_semantic_inspection shell_inspection_walkthrough -- --nocapture
```

It registers a small deterministic fixture language in an embedding, opens the repository package's `Cargo.toml` through actual shell `open`, discovers/captures/authorizes `fixture.read`, displays concise/full feedback, inserts unsaved text using existing App-owned shell `replace`, observes `STALE`, and captures/confirms again to observe `CURRENT`. It prints the transcript and asserts result-header transitions, two dispatches, changed in-memory source and increased version. The workspace is read-only; no source file is written. The fixture language is not Path, needs no evaluator/model/credentials/network, and intentionally returns numeric/control-character edge cases.

The 25 integration tests exercise actual parser/routing plus a focused direct-bridge denial check:

- discovery/capture without execution, empty/missing source, matching registered language;
- App versus selected scratch, exact unsaved text/path/version and unchanged host observations;
- scoped confirmation, wrong/single-use tokens, cancellation/replay, active-language namespacing;
- unsupported effects, optional arguments and selection targets; invalid/duplicate catalog IDs;
- reserved, quoted, whitespace, control-containing and long operation IDs;
- denied authorization with zero dispatch; stale identity/version/restored text and changed catalogs;
- native success, blocked feedback, domain rejection, structured operation errors;
- bounded summary, full lossless JSON and escaped C0/C1/DEL/bidi values/metadata;
- CURRENT/STALE transitions and fresh reinvocation without automatic mutation or save.

Existing inspection tests additionally cover wrong-effect results, source-text/version/language/identity mismatches and no mutation. Existing shell tests cover parser/completion and session lifecycle; no new terminal input behavior was introduced. Scope Script's existing PTY suite remains a regression check for shared terminal history/editing, not proof of manual IDE-shell terminal interaction.

## Remaining limitations

T1 is synchronous, read-only, document-only and argument-free. It retains just one pending capture and last result. Full output is not paged. Discovery does not install new languages. Source and semantic output are not validation certificates or agent permissions. History/completion use the existing infrastructure; language-specific operation-ID completion is not added.

The principal usability limitation remains the separate App and shell-buffer owners: `:scope open` does not select inspection source, and a language session must be cleared to use legacy bare App `open`. T1 exposes that distinction instead of silently unifying sources.

[Explicit source context](T2A_EXPLICIT_SOURCE_CONTEXT.md) reports the separate
owners without unifying them. [Opt-in terminal agent semantics](T2B_TERMINAL_AGENT_SEMANTICS.md)
requires its own host composition and authorization; human inspection grants do
not transfer to that API.
