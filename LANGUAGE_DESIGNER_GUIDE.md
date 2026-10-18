# Building a Language on Top of Scope IDE

Scope is a small semantic development environment for experimental languages. A language can live in its own Rust crate, supply document analysis to Scope, and optionally expose domain tools to agents or component-backed execution through Cell.

**Today:** language services are registered at compile time. Dynamic `LanguageService` plugins are not supported. Native services and tool providers are trusted host code, **not sandboxed Cell guests**.

For complete integration details, see the [Downstream Guide](DOWNSTREAM_GUIDE.md); for agent policy and execution, see the [Harness Guide](HARNESS_GUIDE.md). For language-provided, revision-aware operations and edit proposals, see [Discoverable revision-bound operations](#discoverable-revision-bound-operations) and [reviewable headless edits](HARNESS_GUIDE.md#reviewable-headless-edits). For in-memory source-buffer proposals, see [structured source edit review](INTERACTIVE_SESSIONS.md#structured-source-edit-review).

## 1. Mental model

```text
Workspace        Capability-bearing access to project files
Document         Editable text for one open file
LanguageService  Detects files; reports actions, symbols, diagnostics
App / CLI / TUI  Presents semantic information to humans
ToolProvider     Optional callable domain operations for agents
Cell             Explicit host capabilities and component execution
```

Keep your parser, resolver, checker, or proof kernel independent of Scope. Implement `LanguageService` as a thin adapter from your semantic model to Scope's language-neutral types. Prefer analyzing a supplied document snapshot over reading files from the host.

## 2. Scaffold a language

From the Scope repository:

```sh
cargo run -p xtask -- new-language ../scope-calc-language \
  --extension calc --id example.calc --name Calc --with-runner
```

The generator creates a standalone crate depending on `scope-language`, starter tests, and (with `--with-runner`) a Scope host binary. Implement one small feature first: detecting `.calc`, finding `def` declarations, and reporting a malformed definition.

```sh
cargo test --manifest-path ../scope-calc-language/Cargo.toml
```

Use [Downstream Guide](DOWNSTREAM_GUIDE.md) for checkout layout, generated runner requirements, and host dependencies.

## 3. Implement `LanguageService`

The required methods describe identity, path matching, and document analysis. `actions` is optional; `diagnostics` and `outline` project from `analyze` by default.

```rust
use scope_language::{
    LanguageAction, LanguageAnalysis, LanguageDocument,
    LanguagePath, LanguageService,
};

pub struct CalcLanguage;

impl LanguageService for CalcLanguage {
    fn language_id(&self) -> &'static str { "example.calc" }
    fn language_name(&self) -> &'static str { "Calc" }

    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("calc")
    }

    fn analyze(&self, document: LanguageDocument<'_>) -> LanguageAnalysis {
        // Your parser/checker owns the semantics.
        analyze_calc(document.text())
    }

    fn actions(&self, _: LanguageDocument<'_>) -> Vec<LanguageAction> {
        vec![LanguageAction { id: "calc.check", label: "Check file" }]
    }
}
```

`analyze_calc` is a function you provide. Return a `LanguageAnalysis` containing `symbols` and `diagnostics`. Share a parser pass where possible. Do not call the default `diagnostics` or `outline` implementations from `analyze`: they call `analyze` themselves.

Scope falls back to a neutral language service when none matches. Actions currently **describe** operations; they are displayed, but not automatically executed.

### Headless source sessions

For analysis without an `App`, UI, or agent, use
`scope_ide::document::DocumentSession::open(workspace, document_id, path)` with an
explicitly authorized `Workspace` (prefer `Workspace::read_only` for inspection).
`snapshot()` returns owned text, identity, path, and an opaque **session-local**
revision. Retain that revision with derived results and call `snapshot_at(&revision)`
before treating them as current. External changes or read failures invalidate the
snapshot; `reload()` explicitly accepts the current source, and `close()` releases
the session's retained grant. Filesystem failures preserve typed Cell errors.

Sessions add no language registration, write, or execution authority. Revision
checks are observations, not atomic save guarantees; changes after validation or
reverted between reads may go undetected. See [Headless document sessions](README.md#headless-document-sessions)
for the API example and full contract.

### Reviewable agent edits

Headless `DocumentSession` editing separates `propose`, `review`, `authorize`, and
`apply`. Proposals are immutable full replacements tied to a base revision; reviews
retain a readable diff, provenance, and diagnostics computed from the exact
`proposal.candidate()` snapshot. Nothing writes until the host explicitly chooses
`EditDecision::Allow` and applies with a writable workspace grant. Stale candidates,
denial, and save failures return typed errors.

With `tools`, `OperationRegistry::propose_edit` converts a validated string result
into a proposal and retains the structured operation call/result. Invocation approval
is **not** apply approval; no apply tool is registered. Persistence uses confined,
**best-effort** revision checking, not atomic compare-and-swap or rollback. This is
separate from the existing shell's multi-file proposals and unsaved `App` buffers.
See [reviewable headless edits](HARNESS_GUIDE.md#reviewable-headless-edits).

### Source positions and diagnostics

- `Position` is zero-based; columns are **UTF-8 byte offsets**, not characters or UTF-16 units.
- `TextRange` is half-open: `[start, end)`.
- Use `scope_language::LineIndex` to convert parser byte spans into positions, from the **same immutable source snapshot**.
- `LineIndex` returns `None` for invalid offsets, split UTF-8 characters, or reversed ranges. Do not silently clamp them.
- Keep diagnostics precise, with useful severity and messages. Use stable, broadly meaningful `SymbolKind` values.

Test non-ASCII input, LF/CRLF, EOF, invalid spans, and stale parser results. For `LineIndex` behavior and edge cases, consult `scope-language`'s API/tests rather than reimplementing offset conversion.

## 4. Compose and run your language

A downstream binary can register a language without modifying Scope:

```rust
scope_ide::cli::run_with_languages(
    std::env::args().skip(1),
    [&scope_calc_language::LANGUAGE as &'static dyn scope_language::LanguageService],
)?;
```

Alternatively, register from an embedding host:

```rust
let app = scope_ide::app::App::builder(workspace)
    .language(&scope_calc_language::LANGUAGE)
    .build()?;
```

Matching is ordered: built-in services are registered first and downstream services appended. For custom precedence, supply a `LanguageRegistry` through `AppBuilder::language_registry`.

Run the generated binary (its actual name comes from the scaffold):

```sh
cargo run --bin calc-scope -- <workspace> open
```

In the shell, try `open main.calc`, `language`, `outline`, and `diagnostics`. To use the TUI, enable `scope-ide`'s **dependency** feature `tui` in the runner crate, then run:

```sh
cargo run --bin calc-scope -- tui <workspace>
```

For language-specific panes, implement `scope_ide::tui::Pane` and register it with `Tui::builder().with_default_panes().pane(...)`. For keyboard controls, drawing, event-loop ownership, and embedding, see [Custom TUI panes](DOWNSTREAM_GUIDE.md#custom-tui-panes) and [Redrawing from your own event loop](DOWNSTREAM_GUIDE.md#redrawing-from-your-own-event-loop).

## 5. Grow semantics before infrastructure

A useful progression is:

1. Recognize file paths; parse declarations and return an outline.
2. Return syntax and name-resolution diagnostics.
3. Add symbol tables, type/proof checking, and structured semantic queries.
4. Expose descriptive document actions such as `check`, `show-goals`, or `normalize`.
5. Add optional tools, UI panes, or Cell-backed transformations only when the language needs them.

For a theorem prover, keep proof validity in a small trusted kernel, not in the UI or agent. For other languages, keep the parser/checker usable without opening Scope.

Prefer operations such as `definition(symbol_id) -> location` or `check_proof(term) -> diagnostics` over generic file or command execution.

## 6. Optional agent tools

`LanguageService` answers document-oriented semantic questions. A separate `scope_ide::tool::ToolProvider` exposes named, structured operations that an agent session may call. Enable `scope-ide`'s `tools` feature (or `agent` for optional chat integration).

A language might expose:

```text
calc.evaluate(expression) -> { value }
glyph.proof.check(path) -> { status, goals, diagnostics }
glyph.rewrite(path, range, rule) -> { replacement, explanation }
```

Define each tool with a namespaced name, input definitions, and its actual `ToolEffect` (`Read` or `Write`). Implement `invoke`, or `invoke_with_app` when the operation needs access to the session's live `App` and unsaved document buffer.

A host composes domain providers with `ScopeToolProvider` in an `AgentSession` and discovers them through `session.catalog()`. **Discovery is not authorization:** the host chooses its call-aware policy, approves arguments, budgets execution, and enforces its workspace grants. A tool definition's effect and description are metadata, not a security boundary.

Do not expose generic `run_command`, `read_any_file`, or other ambient-authority escape hatches. For the complete provider/session API, the snapshot/edit contract, proposals, decision loop, and approval examples, see the [Harness Guide](HARNESS_GUIDE.md).

### Discoverable revision-bound operations

For headless source queries, implement `scope_ide::tool::operations::OperationProvider`
(`tools` feature). Register it in an `OperationRegistry` and discover `(OperationId,
OperationDescriptor)` pairs through `descriptors()`. IDs have separate provider and
local-action components, so different providers can publish `inspect` without collision.
Descriptors declare closed recursive input/output schemas, `ToolEffect`, execution
characteristics, and host-defined permission names; values reuse `ToolValue`.

Invoke with a `DocumentSession`, its expected `Revision`, structured arguments, and
an explicit call-aware authorization policy. The registry rejects unknown operations,
malformed arguments, stale revisions, and denied calls before provider dispatch;
it validates bounded outputs and rechecks the revision before returning a structured
result carrying the document identity and revision. A host can permit read-only
queries while denying write or execution permissions. **Discovery grants nothing**:
permission names must be interpreted by the host policy and existing capability grants
must still be enforced. Providers receive only an immutable snapshot, not an `App`.

This is a headless contract, not automatic `AgentSession`/chat tool registration or
unsaved-buffer integration. Native providers are not sandboxed; payload limits do not
bound their runtime or allocations, and revision checks do not roll back side effects.
See [`operations.rs`](crates/scope-ide/src/tool/operations.rs) for the public API and
neutral provider tests.

### Interactive workbench

Enable both `tools` and `tui` to present revision-bound operations and edits with
`scope_ide::tui::WorkbenchPane`. The pane consumes the headless
`scope_ide::workbench::Workbench` controller; it does not use unsaved `App` buffers
as a second source of document state. Construct the controller with an explicitly
authorized `DocumentSession` and your `OperationRegistry`, then compose it:

```rust,ignore
let controller = scope_ide::workbench::Workbench::new(session, registry);
let pane = scope_ide::tui::WorkbenchPane::with_verification_report(
    controller,
    host_operation_policy,
    host_verification_report,
    host_candidate_diagnostics,
);
let tui = scope_ide::tui::Tui::builder().pane(pane).build();
```

The host supplies a call-aware `OperationAuthorization -> bool` policy, an
`OperationResult -> VerificationReport` interpreter (check name, outcome, and
limitations), and diagnostics computed from the exact candidate `DocumentSnapshot`.
There are no permissive default callbacks. Native providers remain trusted host code.

Keyboard workflow: **F1** refreshes; **F2** edits structured arguments; **F3/F4**
select an operation; **F5** chooses inspect/propose/verify; **F6** queues without
execution; **F7** explicitly approves invocation; **F8** cancels queued work;
**F9** reviews; **F10/F11**, then **Enter**, accept/reject the reviewed edit;
**F12** abandons input or confirmation. Page keys scroll. Arguments support bounded
JSON objects, arrays, strings, integers, booleans, and null (not floating-point
numbers). The standard TUI reserves `q`, Escape, and Tab; use `\u0071` for a literal
`q`, or route input through `WorkbenchPane::handle_input` in a custom event loop.

Approval still delegates to registry authorization and workspace capabilities:
accepting an edit with a read-only session fails rather than granting write access.
Stale proposals cannot be applied; refresh failure hides unavailable source and
retained evidence. Verification distinguishes not-run, passed, failed, inconclusive,
skipped, unavailable, cancellation, denial, and errors, and remains bound to the
checked revision. A passed check is not edit authorization. Cancellation only removes
queued work; synchronous provider execution cannot be interrupted or rolled back.

The same controller can be driven headlessly with `request`, `approve_with_verification`,
`review_proposal`, and `decide_edit`. Hosts may append agent messages with `push_message`;
this does not automatically register operation providers in `AgentSession` or connect
the pane to remote chat. Existing CLI commands and default TUI panes are unchanged.

## 7. Optional typed execution through Cell

`scope-types` models typed transformations and graph connections. `scope-execution` binds those declarations to **explicitly supplied WebAssembly components** and executes pipelines through Cell. This is separate from native `LanguageService` registration: declaring an action does not execute a pipeline.

A host must independently supply:

- a registry of exact nominal type/transformation identities and port definitions;
- a component `Binding` for each executable transformation;
- compatible payload schemas and envelope ABI;
- external inputs and planning grants; and
- a configured Cell with the required installed host capabilities.

`Manifest` JSON can share a graph, including incomplete wiring, without distributing component implementations or authority. Import and resolve it against trusted local declarations and bindings before execution. **Planning grants, installed capability names, and valid graphs do not themselves authorize guest operations.** Execution preflights bindings and interfaces, but prior guest effects are not rolled back after runtime failure.

Start with the [Downstream pipeline quickstart](DOWNSTREAM_GUIDE.md#execute-a-typed-pipeline-through-cell), the [portable composition guide](DOWNSTREAM_GUIDE.md#export-and-import-portable-compositions), and [`scope-execution` reference](crates/scope-execution/README.md).

## 8. Host capabilities and logging

Scope's `Workspace` exposes capability-bearing file operations; access must still be explicitly granted. `capabilities` reports installed Cell interfaces, while `authority` and descriptor APIs describe configured policy. These surfaces are **advisory**, not permission checks. Operations can still be denied at runtime.

Native logging is separately opt-in through `scope_ide::logging` (feature `logging`). A trusted host constructs `(LoggingBinding, LoggingController)`, retains the controller, explicitly authorizes a Cell `Logger`, and passes only cloned binding handles to language adapters. Bindings deny by default. Revocation affects later admissions but does not cancel calls already admitted. A successful receipt is not a durability guarantee. Inspection and analysis never create a grant.

For working examples, see [`native_logging.rs`](crates/scope-ide/examples/native_logging.rs) and [`logging.rs`](crates/scope-ide/src/logging.rs). For workspace watching, capability composition, and guest imports, use the [Downstream Guide](DOWNSTREAM_GUIDE.md) and [Building with Cell](BUILDING_WITH_CELL.md).

**Important trust boundary:** native Rust adapters and providers can use ambient host APIs unless independently constrained; read-only document parameters do not sandbox them. Cell guest components begin with no ambient authority and require explicit matching imports and host installations. Future Cell-backed language-service plugins are a design goal, not a current feature.

## 9. Checklist

- [ ] Scaffold a standalone crate and verify its generated tests.
- [ ] Implement `LanguageService` identity, file matching, and `analyze`.
- [ ] Produce useful symbols and precise diagnostics.
- [ ] Test UTF-8 byte spans, malformed programs, and source-version consistency.
- [ ] Register through a downstream runner and test shell/TUI detection.
- [ ] Keep your semantic engine independent of Scope and host I/O.
- [ ] If adding tools, test catalog discovery, call-aware denial, and host authority.
- [ ] If adding Cell execution or logging, test absent grants and failure paths; never infer permission from discovery.

## References

- [Downstream Guide](DOWNSTREAM_GUIDE.md): registration, TUI, workspace and component integrations.
- [Harness Guide](HARNESS_GUIDE.md): agent tools, approval, snapshot/edit, proposals and bounded decision loops.
- [`scope-language`](crates/scope-language/src/lib.rs): document, diagnostics, spans, symbols, service trait.
- [`scope-execution`](crates/scope-execution/README.md): typed component execution.
- [Building with Cell](BUILDING_WITH_CELL.md): guest capabilities and component composition.
