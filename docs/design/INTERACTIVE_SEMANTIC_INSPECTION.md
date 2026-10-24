# Interactive semantic inspection

The experimental TUI lets a human discover and explicitly authorize a registered language's read-only semantic operation against the actual open editor `Document`. No model, network service, or credentials are needed. This is inspection, not an editing interface or a validation certificate.

## Original gap and reused interfaces

Previously the ordinary TUI could show language actions, diagnostics, and outlines, but offered no complete semantic discovery → authorization → invocation → result workflow. Semantic operations remained available mainly through native integrations and tool hosts. This bridge does not merge `LanguageAction` with `SemanticOperationDescription`.

| Responsibility | Existing interface reused |
| --- | --- |
| Active editor document | `App::open_document()` / `open_document_mut()` (one open document today) |
| Language selection | The App-owned `LanguageRegistry` through `App::semantic_service_for_document()`; first registered matching service, neutral fallback |
| Catalog | `LanguageService::semantic_operations()`; no second operation registry |
| Identity and snapshot | `DocumentSource`, `SourceTarget::snapshot()`, tagged `source_edit::SourceId::Document`, document `u64` version and exact in-memory text |
| Authorization | `SemanticAuthorization` supplied to `SemanticToolProvider`; the confirmation policy checks every captured field and `{}` arguments |
| Dispatch | `SemanticToolProvider` invokes the free validated `scope_language::execute_semantic_operation` dispatcher |
| Feedback | Existing semantic variants and structured `ToolResult`; an additive native-result observation preserves JSON outside the agent value subset |
| UI | Existing `Tui` modal input dispatch and ratatui paragraphs, not a shell or alternate input editor |

The App has no persistent source-edit controller. The provider requires one even for reads, so inspection supplies an isolated controller solely to satisfy that API. It never exposes proposal acceptance; the validated dispatcher rejects change results from read operations before proposal registration. No document edit or revision advance is performed.

## Workflow

Start the TUI with `cargo run -p scope-ide --features tui -- tui .`, then open a file handled by a registered language that exposes an eligible operation. Registration is native Rust integration; arbitrary files do not automatically gain semantic operations.

1. **F8** opens the semantic inspection modal. Opening and rendering discover operations but do not execute them. **Ctrl-I** is an alias only where the terminal distinguishes it from Tab; F8 avoids that ambiguity.
2. **Up/Down** selects an operation and cancels any pending confirmation.
3. **Enter** captures the selected operation and current source snapshot for review.
4. **Enter again** authorizes exactly that captured request. A changed snapshot is rejected rather than silently authorizing new source.
5. **Esc** cancels a pending confirmation; when none is pending, it closes the modal.
6. **PgUp/PgDn** scroll the modal by ten lines; **Home** returns to the top.

Results show operation, language, source identity, and observed document revision, with **CURRENT** or **STALE** status. Editing the buffer does not rerun the operation. Press Enter twice to capture and authorize a fresh read. A stale result remains historical feedback, not evidence about the new buffer.

## Eligibility and authority

The initial interface includes only operations whose effect is `Read`, target requirement is `Document`, and `arguments.is_empty()` is true. Even an operation with only optional arguments is excluded. Selection-targeted operations, edits, and operations needing arguments are outside this interface. Duplicate or empty operation IDs fail closed.

Inspection reuses the existing semantic bridge/provider (`SemanticToolProvider`) rather than adding a separate language dispatcher. Capture binds source identity, text, revision, language, and operation description; invocation rechecks the binding and catalog before consulting authorization. Discovery is not permission. Confirmation grants one concrete request with empty arguments, not blanket authority for the provider.

Operation `constraints` are opaque provider metadata. There is no standard external capability schema here that the host can interpret as an enforceable filesystem/network policy. This is a documented limitation, not a capability negotiation mechanism.

Native `Read` execution is synchronous trusted Rust code. The effect declaration and confirmation are **not an OS sandbox**, resource bound, cancellation mechanism, or proof that a provider has no side effects. A slow provider can block the UI. Register only trusted language services; Cell's sandbox guarantees do not automatically apply to them.

## Source context and feedback

The bridge reads the live editor buffer, including unsaved edits. `SourceTarget::language_path()` defaults to `None` for existing/custom targets. The editor document adapter supplies the document's actual Scope path as language context, not filesystem authority; it does not invent a filename from the language ID.

The editor `Document` revision is the local buffer version used for snapshot freshness. It is **not `DocumentSession`**, a separate session abstraction, nor a disk revision or interactive evaluator history. Reopening a file can also invalidate a result through source identity even if its text matches.

Full native read JSON is preserved, including floating-point numbers and unsigned integers above `i64::MAX`. The legacy agent `ToolValue` representation cannot encode those values and may report a conversion error (`internal_failure`). When a valid native read result exists, the TUI's native display is authoritative; the conversion failure does not mean that native read failed. Existing agent protocol conversion behavior is unchanged. Native results are retained only after bridge result/contract checks.

Read values, rationale, evaluation/check feedback, blocked responses, and errors are displayed as invocation feedback. They are not promoted into Scope validation certificates or diagnostics overlays. This feature adds no diagnostics overlays and does not mutate, save, or propose source edits.

## Runnable example

```sh
cargo run -p scope-ide --features tui --example interactive_semantic_inspection --offline
```

[`interactive_semantic_inspection.rs`](../../crates/scope-ide/examples/interactive_semantic_inspection.rs) registers a tiny fixture language with exactly one document read operation, creates a temporary on-disk workspace, and opens its file through `App` into a real `Document`. It drives `Tui` with F8, Enter, Enter using ratatui's `TestBackend` and prints the frame. It then applies a `TextEdit` to the buffer, prints the stale frame without dispatching another read, and presses Enter twice to print a fresh result containing unsaved source.

Assertions check current/stale/current display, dispatch counts, preservation of a large unsigned JSON value, dirty-buffer state, and unchanged disk contents. A cleanup guard removes the temporary workspace on normal return or unwinding. No terminal raw mode, model configuration, network call, or credentials are used. The example assumes a writable system temporary directory and available build dependencies; `--offline` requires those dependencies to be cached.

The `tui` feature implies `tools`, not `agent`; it does not enable the optional agent/model dependencies. The example's Cargo entry requires `tui` without changing that feature definition.

## Remaining limitations and narrow follow-ups

This intentionally synchronous document-only workflow has no argument forms, selection invention, scheduling, editing authority, diagnostics, or highlights. The stock TUI remains a document browser; embedding hosts can supply unsaved editor changes, as exercised in tests and the example. Built-in languages need not advertise any operations; an empty list is normal. Native services must be trusted; no new capabilities are granted or external services configured by this command. Opaque constraints cannot serve as a generic enforceable external-capability declaration.

A small follow-up could improve navigation of large result objects with a generic expandable tree. Automatic diagnostics, argument entry, and asynchronous execution require separate designs and are not part of SIO1.
