# Downstream Guide

Scope supports external Rust language services through compile-time composition. You do not need to edit Scope's built-in language code. Dynamic plugin loading is not supported.

## Prerequisites and current limits

Use a local Scope source checkout. Cell is fetched from https://github.com/nate-droid/cell at tag `v0.0.1` through Scope's workspace dependencies; no sibling Cell checkout is required. No registry release availability is assumed here. Scope is licensed under the [MIT License](LICENSE); dependencies, including Cell, are covered by their own licenses. This guide makes no MSRV guarantee.

No Wasm target is needed for these host integrations. [Building Scope with Cell](BUILDING_WITH_CELL.md) documents the Cell Git dependency, toolchain, and Scope build/test commands. For Cell's guest and component APIs, consult the documentation at the matching [Cell tag](https://github.com/nate-droid/cell/tree/v0.0.1).

The Scope path dependencies shown here and emitted by the scaffolder are for local development, not portable distribution. Scope dependencies remain local; use the same Cell Git URL and `v0.0.1` tag for direct Cell dependencies in standalone consumers.

The `tui`, `tools`, and `agent` features are opt-in; `scope-ide` defaults to no features. Enable `tools` for model-independent tool discovery and execution. The `agent` feature includes `tools` and the OpenAI capability. Default headless and TUI-only builds expose neither module; tools-only builds expose `tool` without compiling the OpenAI capability. Non-agent examples do not require an API key or invoke an agent.

## Normal launcher with Script inspection

Use `scope_ide::cli::run_with_registry_and_command(args, registry, &mut command)`
to compose a downstream `LanguageRegistry` and `ReadOnlyCommand` through the
standard CLI pipeline. Existing `run_with_registry` and
`run_from_args_with_command` remain supported. Registration grants no additional
workspace or semantic authority.

The shell supports `:scope semantic-target ID` (IDs from `:scope buffers`) to bind
an independent live scratch, `:scope semantic-target` to report it, and
`:scope semantic-target none` to clear it. Switching the active scratch does not
change the binding. `:scope script run` calls
`ReadOnlyCommand::invoke_script_with_target(source, target, output)` with immutable
invocation-local scratch borrows. Existing callbacks default to `invoke_script`.
A closed explicit target fails resolution; it never falls back to disk or the
App document. Source selection is not semantic authorization.

See the [SI6 contract and Path DX2 handoff](crates/scope-script/docs/SI6.md) and
[normal-launcher example](crates/scope-script/examples/semantic_target_cli.rs)
for exact signatures, a verified invocation sequence, revision semantics,
authorization constraints, and realistic unsaved-source regression coverage.

## Model-independent harness discovery

With `features = ["tools"]`, use `scope_ide::tool::AgentSession` to register
Scope and downstream `ToolProvider`s. `session.catalog()` returns an owned
`SessionCatalog` snapshot of the tools actually registered in that session:

```rust
let mut session = scope_ide::tool::AgentSession::for_app(&mut app)?;
let catalog = session.catalog();
if let Some(entry) = catalog.lookup("scope.document.read") {
    // Present entry.definition, entry.provider, and entry.output_description
    // to your own decision adapter.
}
let discovery_payload = catalog.to_value();
```

`to_value()` exports a dependency-free `ToolValue` object with a `tools` array.
Each entry has `definition` (name, description, effect, inputs), `provider`, and
`output_description`. Input definitions retain name, kind, description, and
requiredness. Effects and kinds use lowercase strings; unavailable metadata is
`Null`. Output descriptions are advisory prose, not validated output schemas.
Providers can override the default `provider_name()` and
`output_description(tool_name)` methods without changing their tool definitions.

The catalog describes exposed tools, not all installed Cell interfaces and not
permission to execute. It is a snapshot; request a fresh catalog when needed.
Provider metadata and effect declarations are trusted host metadata, not proof
of isolation or behavior. No tool calls or model requests occur during discovery.

For argument-aware authorization, call `invoke_with_call_policy` or
`invoke_observed_with_call_policy`. Their policy receives `(&ToolDefinition,
&ToolCall)` immediately before dispatch, allowing path- or argument-specific
checks without cloning the proposed call. The existing definition-only policy
APIs remain available. Unknown tools return `ToolError::UnknownTool` without
calling policy; rejected known calls return `ToolError::Denied` without invoking
the provider. Observed dispatch reports both outcomes without recording payloads.

Downstream code can supply model adapters, approvals, budgets, and completion
criteria. Scope's `agent` feature also provides a bounded decision loop through
`CellAgent::run_session`; `CellAgent::answer` remains a single-shot API that does
not dispatch tools. See [Harness Guide](HARNESS_GUIDE.md) for the model-driven
loop and a deterministic scripted example.

## Refinement-aware semantic edit budgets

Candidate evaluation is a purpose, independent of side effects. Mark a read-only
operation with
`operation.with_purpose(SemanticOperationPurpose::CandidateEvaluation)?`.
This stores `scope_operation_purpose` in the existing constraints object; the
semantic adapter exposes normalized `purpose` catalog metadata. Missing or unknown
purpose retains legacy exploration accounting. Keep returning typed `Evaluated`
evidence and declaring `semantic_operation_produces_evaluation` as before: purpose
alone never establishes validation, grants authorization, or permits writes.
Non-object constraints cannot be annotated with the builder; migrate them to an
object explicitly rather than silently replacing them.

`EditBudgets` defaults to 3 exploratory inspections, 2 candidate refinements,
1 initial proposal, 1 proposal correction, 6 model invocations, and no tolerated
duplicate calls. Marked evaluations have a total cap of `1 + refinements`;
`refinements` counts dispatched evaluations after unresolved candidate evidence,
not proposals or protocol retries. A failed evaluation at that cap returns
`RefinementBudgetExhausted` with `refinement_stopped=refinement_budget_exhausted`.
An extra exploration call returns `ExplorationBudgetExhausted`; reaching the
exploration limit does not hide evaluation tools or force the proposal phase when
an evaluator is available. Return `complete` to advance to proposal creation.
Languages without marked evaluators retain the existing automatic phase advance.

Every decision, including completion, discovery, suppressed duplicates and protocol
recovery, remains bounded by both `model_invocations` and transport `max_decisions`
(default 12). All dispatches and discovery calls share transport `max_tool_calls`
(default 8); malformed recovery remains bounded by `max_malformed` (default 2).
Global exhaustion returns `OperationBudgetExhausted`. No additional lifecycle or
retry mechanism is introduced. Two inspections, two evaluations, a completion and
a proposal require **7 model invocations**: explicitly configure at least 7 for
that workflow (and sufficient transport limits). Phase allowances never override
the global limits.

Failed marked evaluations retain a focused continuation containing the exact
candidate invocation, typed evaluation, provider observations, remaining refinement
opportunities, and next action, without unrelated inspection results or model
reasoning. Duplicate suppression still compares the entire call and relevant host
state: a revised candidate is distinct, an identical unchanged call is suppressed.
`EditRun`, `EditSummary`, and trace events separately expose inspections,
evaluations, refinements, proposal attempts, and proposal corrections; protocol
retries remain separately observable. Candidate success does not authorize or
apply a proposal, which still goes through host policy and normal validation.
Downstream exhaustive outcome/event matches and complete `EditBudgets` literals
must accommodate the new variants/field; prefer `..Default::default()` for budgets.

## Trust and filesystem authority

Registered `LanguageService` and `ToolProvider` implementations run as trusted native host code, not sandboxed Cell guests. Read-only document views and a read-only `Workspace` constrain their respective APIs; they do not prevent native implementations from calling ambient host APIs themselves. Keep analyzers focused on supplied document text and use Scope workspace operations for explicitly granted project access.

`Workspace::read_only(root)` denies writes through that workspace; `Workspace::new(root)` permits them. File discovery skips symlinks and special entries, recurses only into entries reported as directories, and propagates lookup failures. Explicit reads may follow confined relative symlinks, so discovery policy is not a blanket prohibition on symlink access. Cell enforces confinement for each operation; do not add a host-path canonicalize/check/write preflight.

`Workspace::capabilities()` lists installed interface names, not roots or write policies. Both workspace policies report `cell:fs/fs@0.1.0`.

## Isolated text proposals

Start the interactive shell (`scope-ide --workspace <root> open`) with feature
`agent` enabled for model calls. Approval is a human action, separate from model
output; a request does not approve paths. For example:

```text
proposal src/main.rs src/new.rs
proposal-agent Implement the requested change in these approved files
proposal-review
proposal-accept
```

Use `proposal-discard` instead to drop the isolated snapshot. Only one proposal
is active per shell; it is not persisted and is dropped on exit. Normal `agent`
commands remain read-only. The proposal provider registers only list/read/create/
replace tools, never acceptance, shell, build, or arbitrary native providers.
Review shows a changed-file listing and complete before/after text, not a unified
line diff. A budget stop or model error leaves the proposal available for review;
it is not a successful completion or approval.

Public APIs (no optional feature required for snapshots):

```rust
use scope_ide::{path::ScopePath, workspace::ProposalLimits};
let paths = vec![ScopePath::new("src/main.rs")?, ScopePath::new("src/new.rs")?];
let mut proposal = app.proposal(&paths, ProposalLimits::default())?;
let changes = proposal.changes()?;
// Human review and explicit approval occur here, outside the model.
let result = app.accept_proposal(&mut proposal);
// Always inspect BOTH result.applied and result.error.
```

With `tools`, install `ProposalToolProvider::new(&mut proposal)` in a fresh
`AgentSession::builder()` with **no baseline providers**. Hosts supply dispatch
policy explicitly; do not infer approval from model arguments. With `agent`,
`CellAgent::run_proposal` installs only that provider and uses the existing bounded
loop. `scope.document.create` takes `path`/`text` and refuses overwrite.
`scope.document.replace` keeps the existing zero-based half-open line/byte-column
edit convention, but takes an integer `expected_revision` returned by proposal
reads; it always edits the isolated snapshot, with no `save` argument. Proposal
revision numbers are not baseline disk revisions. `Proposal::read`, `create`,
`replace`, `changes`, `discard`, and `accept` also support model-independent hosts.

The snapshot contains only exact approved files, not a recursive workspace copy.
Absent files can be approved for creation, but their parent directories must
already exist. Unapproved paths, traversal, symlink leaves/parents (including
dangling links), special files, and path components `.git`, `target`, `build`,
and `node_modules` are rejected. Defaults: 64 paths, 256 KiB per file, 2 MiB
aggregate text; both original and proposed text are retained (up to twice the
aggregate bound, plus review/tool output copies). Oversize, invalid UTF-8 and
NUL-containing binary files are rejected, never silently omitted. No deletion,
directory creation, rename, binary patching, or permission-change support.

Acceptance compares original text/absence for **every changed path before any
write**, then checks again before each write. New files use exclusive creation;
replacements are sequential writes, not atomic compare-and-swap or multi-file
transactions. `Acceptance.applied` includes paths opened for mutation even if a
write later fails; `error` reports conflicts/I/O failures, including partial apply.
Successfully applied paths are rebased in the retained proposal on partial failure;
a partially written path remains conflicted and needs human inspection. Discard
never rolls back already-applied writes. Same-content external edits are not
distinguished from the original baseline.

This is a trusted-host workflow using host filesystem APIs for explicit human
acceptance, not a sandbox against concurrent hostile filesystem mutation.
Symlinks are checked at snapshot and acceptance, but these pathname checks are
**not race-resistant**: a leaf, parent, or root-ancestor rename/symlink swap
between check and open can redirect reads or writes outside the approved root.
Exclusive new-file creation prevents overwriting an existing final entry, not
parent-directory redirection. Hard links are not detected; replacement truncates
the existing inode and can affect other links. Use only a trusted filesystem,
without concurrent hostile mutation. Content changes between the last check and
write can also be overwritten. A read-only baseline cannot be accepted. App acceptance
refuses dirty active buffers and never silently replaces any open buffer; reopen
accepted files to refresh them. Disk contents, including pre-existing user edits,
are the snapshot baseline; unsaved editor text is neither copied nor overwritten.

## Structural workspace snapshots

`scope_ide::workspace::WorkspaceMeta` builds bounded, typed directory/file graphs
through the existing Cell-confined workspace. It is available without optional
features and supports path lookup, child/parent navigation, source locators,
revision-aware references, and explicit invalidation or optional Cell watching.
Run `scope-ide --workspace <path> meta` for a read-only structural description.
See the [scope-meta guide](crates/scope-meta/README.md) for the underlying graph
contracts and the generated `scope_ide::workspace::WorkspaceMeta` API documentation
for adapter bounds, truncation, and freshness controls. Generate it with
`cargo doc --workspace --all-features --no-deps --locked`.
Graph metadata grants no source-access authority.

## Inspect and edit source files

Start the interactive shell with `cargo run -p scope-ide -- open <workspace>`.
Source inspection and editing use the same documents and Cell-confined workspace
as downstream integrations; no language-specific parser is required to edit text.

```text
files
open src/main.rs
lines 1 10
outline
diagnostics
replace 1 1 1 1 // Reviewed through Scope\n
show
dirty
save
```

`replace <start-line> <start-column> <end-line> <end-column> <text>`
uses **1-based lines and UTF-8 byte columns**, with a start-inclusive,
end-exclusive range. Equal endpoints insert text; omitted text deletes the range.
Positions inside a UTF-8 character and invalid ranges are rejected without
changing the document. Replacement text is unquoted; `\n`, `\t`, and `\\`
encode newline, tab, and backslash. Exactly one separator after the final
coordinate is consumed; additional leading and trailing spaces are literal.

Edits remain in memory until `save`; `revert` restores the last saved text.
`outline` and `diagnostics` inspect the current buffer using registered language
services (their usefulness depends on the services installed by the host).
Saving uses Cell's revisioned existing-file replacement, not truncation or file
creation. External changes produce a conflict and retain the unsaved buffer.
Revision checks are best-effort optimistic detection, **not atomic
compare-and-swap**. Final symlinks cannot be saved, and platform support and
metadata-preservation limits follow the [Cell save contract](BUILDING_WITH_CELL.md#save-edits-to-an-existing-file).

## Choose an integration path

### 1. Language service only

Depend only on `scope-language`, implement `LanguageService`, and test your analyzer directly using `LanguageDocument`. This path needs neither `scope-ide` nor Cell and is appropriate for parser, outline, and diagnostic development.

For a crate at `examples/downstream`, the local dependency is:

```toml
[dependencies]
scope-language = { path = "../../crates/scope-language" }
```

Expose a service value with static lifetime, such as `pub static LANGUAGE: CalcLanguage = CalcLanguage;`, for later host registration. Keep parsing and name analysis in the library rather than in the UI or runner.

### 2. Headless app through `AppBuilder`

Add `scope-ide` and construct a workspace with the authority your host needs. Register your library's static service:

```rust
let app = scope_ide::app::App::builder(workspace)
    .language(&LANGUAGE)
    .build()?;
```

`App::builder` returns an `AppBuilder`. The app provides document and semantic operations without requiring the terminal UI or an agent. The checked-in headless example uses a read-only workspace and defaults to its fixture directory using `CARGO_MANIFEST_DIR`, so its default workspace does not depend on the process working directory.

### 3. Reuse the CLI with `cli::run_with_languages`

In your own binary, register the same library service and forward arguments without the executable name:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    scope_ide::cli::run_with_languages(
        std::env::args().skip(1),
        [&LANGUAGE as &'static dyn scope_language::LanguageService],
    )
}
```

This exposes the standard Scope CLI and shell with your language compiled in. Running the stock `scope-ide` binary instead will not register your service.

Both default composition paths register built-ins first and append external services. The first matching service wins, so an external service cannot override a matching built-in through append-only registration. A headless host can supply a custom `LanguageRegistry` through `AppBuilder::language_registry` for explicit ordering. Unmatched documents use neutral semantic fallback.

### 4. Typed component execution without the IDE

Use `scope-types` for descriptions, validation, and discovery alone, or add
`scope-execution` and Cell to execute those descriptions as WASM components.
This path does not require `scope-ide`, a `LanguageService`, a TUI, an agent,
or an API key. See the [execution quickstart](#execute-a-typed-pipeline-through-cell)
below. Native language services and providers remain trusted host code; registering
them does not turn them into sandboxed transformations.

## Combined document analysis

Hosts that need both diagnostics and outline can call `LanguageService::analyze`
(or `App::open_analysis` for the open document). The returned `LanguageAnalysis`
contains `diagnostics` and `symbols`. Implementing `analyze` is required;
parser-backed services should parse once and produce both outputs. The built-in
language pane uses this combined operation directly.

The separate `diagnostics` and `outline` methods default to one-way projections
of `analyze` and remain available for consumers that need only one output.
Services may override either for a cheaper single-output path. `analyze` must
not call the default projections, which would recurse. This is a source-breaking
trait change: services that previously implemented only `diagnostics` and
`outline` must add `analyze`, preferably sharing their parser pass, and may
remove redundant projection overrides. Combined analysis does not cache results
across calls or redraws.

`LanguageDocument::version()` is local to a document lifetime, not a globally
unique snapshot revision. Reopening the same path can restart at version zero
with different text, so `(path, version)` alone is not a safe cross-document
cache key. Standalone consumers should verify source text or supply their own
snapshot identity; host-managed caches also need document lifetime identity.

## Adapting parser byte spans

`scope-language::LineIndex` provides dependency-free checked adapters for common
parser outputs. Build one index per immutable source snapshot and reuse it for
all diagnostics and symbols; rebuild it when the document changes. Construction
is O(bytes), byte-to-position lookup is O(log lines), and position-to-byte lookup
is O(1).

```rust
use scope_language::{Diagnostic, LineIndex, Symbol, SymbolKind};

let source = "def café\n";
let index = LineIndex::new(source);
let parser_span = 4..9; // Or parser_node.start_byte()..parser_node.end_byte().
let range = index.range_for_span(parser_span.clone())
    .expect("parser span must match this source snapshot");
let diagnostic = Diagnostic::warning(range, "unused definition");
let symbol = Symbol {
    name: source[parser_span.clone()].to_owned(),
    kind: SymbolKind::Function,
    range,
};
assert_eq!(index.span_for_range(symbol.range), Some(parser_span));
assert_eq!(diagnostic.range, symbol.range);
```

`position_for_byte` and `byte_offset` convert individual endpoints;
`range_for_span` and `span_for_range` convert half-open ranges. All return `None`
for invalid input (out of bounds, inside a UTF-8 character, or reversed ranges),
without clamping. Handle `None` according to your analyzer's error policy rather
than silently dropping or relocating spans. Empty and multiline spans are valid.
Convert inclusive ends to exclusive ends in the parser adapter, with checked
arithmetic and the parser's encoding rules; these helpers expect UTF-8 bytes,
not UTF-16 units, character indices, or one-based locations.

Lines and columns are zero-based; columns count UTF-8 bytes. Only LF starts a
new line. LF's byte offset is the preceding line's final column, while the byte
after LF is exclusively `(next_line, 0)`; positions cannot spill into the next
line. CR is an ordinary byte, so CRLF permits positions before CR and between CR
and LF, with no normalization. EOF is valid; a trailing LF creates an empty
final line. An empty document has only `(0, 0)`. Every accepted offset and
position roundtrips exactly. These canonical adapter rules are independent of
the IDE's display-oriented line counting; use the same index for both directions.
No parser dependency or serialization feature is required.

## Agent integration

For your own harness, enable `features = ["tools"]` and use `scope_ide::tool` without a model dependency. The [Harness Guide](HARNESS_GUIDE.md) includes a standalone scripted consumer, execution policy and events, and the revision/identity preconditions now required by `scope.document.replace`. Existing callers must copy `expected_revision` from reads and, for active documents, also supply `expected_document_id` and `expected_version`.

App saves now use Cell's checked replacement saves for existing files: external changes produce a structured conflict and leave the dirty buffer intact. This is best-effort conflict detection, not atomic compare-and-swap, and currently requires Unix. `Workspace::save_file` remains the legacy unchecked create/write API; callers must opt into `save_file_checked` for revision checks.

Enable chat and semantic agent tools explicitly on the host dependency:

```toml
scope-ide = { path = "../../crates/scope-ide", features = ["agent"] }
```

Use `features = ["agent", "tui"]` for both integrations. This exposes `scope_ide::agent` and `scope_ide::tool` and enables CLI/shell `agent` (`ask`) commands. Without the feature, those commands and CLI agent options return an actionable enablement error; ordinary app, shell, and language behavior is unchanged. A downstream `--features agent` flag requires a local forwarding feature (`agent = ["scope-ide/agent"]`); it is not inherited automatically.

In the Scope workspace, run `cargo run -p scope-ide --features agent -- --workspace . agent "summarize this project"`. The default endpoint is local; public OpenAI use requires a host-provided API key. Requests send workspace context to the configured endpoint; see [Agentic features](README.md#agentic-features) for endpoint and key options.

## TUI integration

The TUI is optional. Enable it on the `scope-ide` dependency in your host crate;
for a crate at `examples/downstream`, that would be:

```toml
scope-ide = { path = "../../crates/scope-ide", features = ["tui"] }
```

A runner using `cli::run_with_languages` then supports the TUI command with the
same registered services as its shell:

```sh
cargo run --manifest-path examples/downstream/Cargo.toml --bin calc-scope -- tui examples/downstream/fixtures
```

This command requires the dependency feature change above; the checked-in
consumer and generated runners do not enable TUI support by default. A
standalone crate does not automatically inherit a Cargo feature named `tui`;
`--features tui` requires a locally defined forwarding feature.

For direct embedding, construct an app with your services and use
`scope_ide::tui::run(app)?`, or customize its panes:

```rust
let app = scope_ide::app::App::builder(workspace)
    .language(&LANGUAGE)
    .build()?;

scope_ide::tui::Tui::builder()
    .redraw_interval(std::time::Duration::from_millis(100))
    .with_default_panes()
    .pane(MyLanguagePane)
    .build()
    .run(app)?;
```

`MyLanguagePane` is your implementation of `scope_ide::tui::Pane`, not a built-in
Scope type. The default panes are **Files**, **Document**, and **Language**.
`with_default_panes()` appends them; `pane(...)` appends a custom pane. Without
`with_default_panes()`, the builder contains only the panes you add.

The built-in runtime defaults to a 250 ms redraw/polling target. The
`redraw_interval` setting above changes the input polling timeout, not a
guaranteed cadence or a simulation clock. Input can cause earlier redraws;
rendering and input handling can delay them. Zero enables non-blocking polling
and may consume substantial CPU. Caller-owned event loops are unaffected.

### Redrawing from your own event loop

`Tui::run` owns terminal setup and the blocking event loop. For embedding or
background-driven updates, keep your `Tui`, `App`, and Ratatui `Terminal` and
redraw explicitly:

```rust
// `terminal` is a Ratatui Terminal initialized and owned by your host.
let mut tui = scope_ide::tui::Tui::builder()
    .with_default_panes()
    .pane(MyLanguagePane)
    .build();
tui.draw(&mut terminal, &app)?;

// In your event loop, after receiving a key:
if tui.handle_key(key, &mut app) == scope_ide::tui::InputOutcome::QuitRequested {
    break; // The host decides whether to honor quit intent.
}
tui.draw(&mut terminal, &app)?;

// Also call draw after background updates to app or shared pane state.
```

The host owns raw mode, alternate-screen setup, cleanup, event polling, resize
handling, and exit policy. `handle_key` returns `Ignored` for release/repeat
or unconsumed events, `Handled` for consumed events, and `QuitRequested` for
`q` or Escape outside a modal, before pane dispatch. Modal input is captured
first: `q` becomes quick-open text and Escape dismisses the picker, both returning
`Handled`. The host needs no access to internal modal state. Draw
before the first key and after resizes so pane scrolling uses current geometry.
For direct frame access, use
`terminal.draw(|frame| tui.render(frame, &app))?` instead of `tui.draw(...)`.

A redraw renders every pane into a complete frame. Ratatui writes only changed
cells and clears stale content automatically. Custom panes should render their
full current content each time, rather than writing to stdout or skipping
unchanged content. No explicit invalidation call is needed.

### Navigation and current limits

- **Tab / Shift-Tab** cycles pane focus; built-in panes highlight their focused
  border.
- **Ctrl-P** opens a workspace-path picker with case-insensitive substring
  filtering. Type to filter, use **Up / Down** to select, **Enter** to open, and
  **Esc** to cancel. `q` is query text while the picker is open.
- In **Files**, **Up / Down** or **k / j** moves the visible selection,
  **Enter / o** opens it, and **r** refreshes discovery.
- In **Document** and **Language**, **Up / Down** or **k / j** scrolls,
  **PageUp / PageDown** moves a page, and **Home / End** jumps to a boundary.
- Successful file opening focuses **Document**, if present, and resets pane
  scrolling. Outside the picker, **q / Esc** quits.

File-pane open and refresh failures appear in the footer. Quick-open failures
stay in the picker for retry. Both built-in TUI opening paths refuse to replace
a dirty document; this is a UI guard, not a new restriction on `App::open_path`.
Custom native panes remain responsible for safe document replacement.

The TUI is still a single-document browser: there is no text editing,
multi-document buffer/tab switching, project-content search, or navigation from
diagnostics and outline entries to source. Language actions are displayed but
not executed.

### Custom TUI panes

Implement `Pane::id`, `Pane::render`, and optionally `Pane::handle_key`:

- `render` receives its assigned `Rect` and a read-only `TuiContext`. Use
  `ctx.app()` for document and semantic data, `ctx.is_focused()` for focus styling,
  and `ctx.scroll()` for the runtime's per-pane vertical scroll offset.
- `handle_key` receives a `KeyEvent` and `TuiContextMut` **only while the pane is
  focused**. Scope no longer tries each pane in order until one consumes the key.
  Returning `false` does not forward the key to an unfocused pane.
- Use `ctx.scroll()` and `ctx.set_scroll(offset)` to update the stored `u16`
  offset. Clamp it to your rendered content. `ctx.viewport()` reports the pane's
  last rendered rectangle, including any borders your pane draws; account for
  those borders when computing page size and visible content.
- Focus cycling, quick-open, and quit are runtime controls. While quick-open is
  visible, it receives input instead of panes. The runtime also handles the
  built-in Files pane's open and refresh commands.

The public `FilesPane`, `DocumentPane`, and `LanguagePane` remain unit structs;
focus, scroll offsets, and viewport geometry are owned by `Tui`, not by those
structs or `LanguageService`. Existing custom panes should not rely on seeing
keys while another pane is focused. Give custom panes distinct IDs; the runtime
recognizes `scope.files` and `scope.document` for built-in navigation behavior.

For standalone pane tests, `TuiContext::new(app)` supplies unfocused, zero-scroll
rendering context. `TuiContextMut::new(&mut app)` has a default empty viewport
and no runtime scroll storage, so `set_scroll` does not persist an offset there.
Test runtime-managed focus and scrolling through a built `Tui` as appropriate.

## Execute a typed pipeline through Cell

### Try the working demonstration first

From the Scope checkout root:

```sh
cargo run -p scope-execution --example text_pipeline -- 'Hello component world'
cargo test -p scope-types -p scope-execution
```

The example runs real WASM components and prints uppercase text
`HELLO COMPONENT WORLD`, word count `3`, and completion order
`["z-uppercase", "a-count"]`. The order follows dependencies, not node names.
Tests demonstrate rejection of incompatible types, missing planning grants,
uninstalled guest imports, cycles, incorrect export ABIs, invalid outputs, and traps.
The fixtures are compiled from WAT using a dev dependency: no guest compiler or
Wasm target is needed for this demo (Cargo dependencies must be available).
Uppercase and word counting use ASCII rules, not Unicode casing or segmentation.

### Validate a downstream-owned transformation

The separate `examples/downstream` Cargo workspace also binds its own component
using only public APIs, without changes to Scope's implementation or reuse of
Scope's component test helpers. From the Scope root:

```sh
cargo test --manifest-path examples/downstream/Cargo.toml --test wasm_binding --offline
```

The downstream-owned [`reverse.wat`](examples/downstream/fixtures/reverse.wat)
reverses opaque bytes inside WASM. Its
[`wasm_binding.rs`](examples/downstream/tests/wasm_binding.rs) test declares
`consumer:ReverseBytes@2026-10` with `consumer:Bytes@v7` ports and binds the
non-default `reverse-bytes` export. It checks empty, odd/even-length, and non-UTF-8
payloads, selected output and artifacts, and rejection of an output type-version
mismatch without publishing artifacts or recording successful completion.

This demonstrates that downstream hosts can supply their own compatible
transformations, not just Scope's demonstration components. Compatibility means
a **WebAssembly component**, not an arbitrary core Wasm module, with the
[port-envelope ABI](crates/scope-execution/README.md#component-abi), matching named
ports and exact nominal types, an agreed payload encoding, and imports supported
by the host's Cell. The export name is host-selected through `Binding.export`;
it need not be `transform`. Payload-schema validation remains the downstream
adapter's responsibility, and host planning grants do not install runtime imports.

The fixture needs no guest compiler or Wasm target: `wat` compiles it during the
test. Offline execution requires cached Cargo dependencies, including the tagged
Cell Git source. Its small allocator is for small test inputs, not production workloads.

### Add the host dependencies

For a local consumer located at `scope/examples/downstream`, add:

```toml
[dependencies]
scope-types = { path = "../../crates/scope-types" }
scope-execution = { path = "../../crates/scope-execution" }
cell = { git = "https://github.com/nate-droid/cell", tag = "v0.0.1" }
```

Adjust the Scope paths for your consumer's actual location. Scope dependencies
remain local; Cell is fetched by Cargo without a sibling checkout. No `scope-ide`
feature flags are needed. `scope-types` uses Serde and `serde_json` for portable
manifests but has no runtime or WASM dependencies; execution adds the Cell/Wasmtime
runtime.

### Describe, bind, and execute

This host function accepts two already-built **component** binaries, not core
Wasm modules. They must export `transform` with the port-envelope ABI and implement
the declarations below. The runnable repository demo supplies such components;
for your own guests, see the [component ABI](crates/scope-execution/README.md#component-abi)
and [Cell build guide](BUILDING_WITH_CELL.md).

```rust
use std::collections::BTreeMap;
use scope_execution::{Binding, Bindings, ExecutionResult, Value, execute};
use scope_types::{
    Capabilities, Connection, Endpoint, Graph, Pipeline, Port, Source,
    Transformation, TypeId,
};

pub fn run_text_pipeline(
    uppercase_component: Vec<u8>,
    word_count_component: Vec<u8>,
    text: &str,
) -> Result<ExecutionResult, Box<dyn std::error::Error>> {
    let ty = |name: &str| TypeId::new(name, "1");
    let describe = |name: &str, output: &str| Transformation {
        id: ty(name),
        inputs: vec![Port::new("in", ty("Text"))],
        outputs: vec![Port::new("out", ty(output))],
        requires: Capabilities::new(),
    };
    let pipeline = Pipeline {
        graph: Graph {
            inputs: BTreeMap::from([("input".into(), ty("Text"))]),
            nodes: BTreeMap::from([
                ("uppercase".into(), describe("Uppercase", "Text")),
                ("count".into(), describe("WordCount", "Number")),
            ]),
            connections: vec![
                Connection {
                    from: Source::Input("input".into()),
                    to: Endpoint::new("uppercase", "in"),
                },
                Connection {
                    from: Source::Output(Endpoint::new("uppercase", "out")),
                    to: Endpoint::new("count", "in"),
                },
            ],
        },
        output: Source::Output(Endpoint::new("count", "out")),
    };
    let bindings: Bindings = BTreeMap::from([
        (ty("Uppercase"), Binding {
            component: uppercase_component,
            export: "transform".into(),
        }),
        (ty("WordCount"), Binding {
            component: word_count_component,
            export: "transform".into(),
        }),
    ]);
    let inputs = BTreeMap::from([("input".into(), Value {
        ty: ty("Text"),
        bytes: text.as_bytes().to_vec(),
    })]);
    let cell = cell::Cell::new()?; // No installed host capabilities.
    let granted = Capabilities::new(); // Host-approved planning permissions.
    Ok(execute(&cell, &pipeline, &bindings, inputs, &granted))
}
```

Inspect `result.errors` before treating a run as successful; a returned
`ExecutionResult` is not itself proof of success. On success, `result.output`
contains the selected Number value, `result.artifacts` includes both nodes' typed
outputs, and `result.completed` records dependency order. The demo's `Number@1`
encoding is an eight-byte unsigned little-endian integer; validate the payload
length before decoding. Scope validates nominal type tags, not payload semantics.

Descriptions and executable bindings are separate: one binding is keyed by the
transformation's exact name/version and can be used by multiple graph nodes.
Port types also match exact names and opaque version strings, not compatibility
ranges. Define and share payload encodings with your component authors.
`Registry::discover` finds shortest single-input type chains; it is not a semantic
optimizer. Build an explicit graph when a same-type step such as Text → Uppercase
→ Text must run: type discovery alone cannot express that requirement.

### Export and import portable compositions

Use `scope_types::Manifest` to share a composition without sharing its host or
implementation configuration. Version 1 is a JSON document containing `version`,
`inputs` (names to nominal types), `nodes` (graph-local names to transformation
identities), `connections`, and `outputs` (names to sources). Sources are tagged
as `{"kind":"input","value":"input"}` or
`{"kind":"output","value":{"node":"count","port":"out"}}`.

Export an existing pipeline with a named result:

```rust
use scope_types::Manifest;

let json = Manifest::from_pipeline(&pipeline, "result").to_json()?;
```

For multiple named results, use `Manifest::from_graph(&graph, outputs)`, where
`outputs` is a `BTreeMap<String, Source>`. Graph outputs select node outputs or
external inputs; an identity composition needs no nodes. Export/import does not
require graph validity: missing nodes, dangling endpoints, unconnected inputs,
and unresolved transformation identities remain inspectable and round-trip.

In the receiving session, populate the existing `Registry` with trusted local
`Transformation` declarations and provide local `Bindings` separately. Importing
never registers declarations, downloads components, or grants authority. The
following fragment uses host-supplied `registry`, `bindings`, `granted`, `inputs`,
and `cell`; no knowledge of the originating domain is needed by Scope:

```rust
let manifest = scope_types::Manifest::from_json(&json)?;
let resolution = manifest.resolve(&registry, &granted)
    .with_implementations(bindings.keys());

for diagnostic in &resolution.diagnostics {
    eprintln!("{diagnostic}");
}
// Read-only inspection is available even when the composition is invalid.
let graph = resolution.graph();
let outputs = resolution.outputs();

let pipeline = resolution.pipeline("result")
    .map_err(|diagnostics| format!("composition is not ready: {diagnostics:?}"))?;
let result = scope_execution::execute(&cell, &pipeline, &bindings, inputs, &granted);
```

Resolution reuses graph validation without loading or executing components.
Diagnostics distinguish:

- `MissingTransformation`: the exact transformation identity is not registered.
- `MissingImplementation`: a registered node has no local binding identity;
  this optional check is enabled by `with_implementations(bindings.keys())`.
- `Graph(Diagnostic::TypeMismatch { .. })` and other graph diagnostics: invalid
  connections, missing inputs, duplicate sources, or cycles.
- `Graph(Diagnostic::MissingCapability { .. })`: an ungranted planning requirement,
  not a determination of Cell's runtime policy for a particular operation.
- `Output`: an invalid named result reference; `UnknownOutput`: an absent selected
  result name.

`pipeline("result")` succeeds only when the whole resolution has no diagnostics,
including all named outputs. Keep the original `Manifest` for editing/re-export:
its unresolved references are preserved, while `resolution.graph()` contains only
locally resolved declarations. Binding presence does not certify component links,
export ABI, or runtime permission; the executor and Cell still perform their checks.
The receiving adapters and components own the nominal payload contracts.

Manifests contain no component bytes, executable code, binding paths, host paths,
capability grants, or transformation port/requirement declarations. Requirements
come from the local registry, and grants come from trusted host policy. Names are
opaque identifiers, not paths to resolve or code to run. `from_json` and `to_json`
reject unsupported format versions. Unknown fields are reserved for optional
extensions and are ignored, not retained; required new semantics need a new version.

See the [`scope-types` manifest reference](crates/scope-types/README.md#portable-composition-manifests)
for the format example and API details. Regression coverage includes round trips,
resolution failures, and an export → import → local resolution → Cell execution run:

```sh
cargo test -p scope-types --test manifest
cargo test -p scope-execution --test execution portable_composition_resolves_locally_and_executes_in_cell
```

### Keep planning approval separate from runtime authority

`Transformation::requires` declares requirements. The `granted` set is supplied
by the host, never copied from those requirements or inferred from advisory
capability descriptors. To use an imported capability, the host must separately
install the appropriate implementation on the **same Cell** passed to `execute`.
A planning grant neither installs an import nor overrides its filesystem or other
policy. Interface installation alone does not prove permission for a particular
operation; Cell's capability implementation enforces that at runtime.

The executor validates the whole graph, external input types, and selected output,
then preflights every component link and export ABI without instantiating guests.
Failures here leave no completed nodes or artifacts. It then instantiates and calls
nodes in dependency order, checking each complete output set before publishing it.
Instantiation/start traps, call errors, or malformed outputs stop execution; earlier
successful artifacts remain available. This is **not transactional**: prior guest
side effects are not rolled back.

The initial API is synchronous and accepts `Cell<()>`, with a fresh store and
instance per node. There is no compiler, shell/process execution, automatic action
or agent-tool dispatch, retry policy, persistent guest state, or dynamic language
plugin loader. Downstream hosts choose when to call it and how to present errors.
For guest contracts and language design, see
[Executing Typed Pipelines Through Cell](LANGUAGE_DESIGNER_GUIDE.md#executing-typed-pipelines-through-cell).

## Checked-in standalone consumer

`examples/downstream` is a separate Cargo workspace with its own `[workspace]`, not a member implicitly relying on Scope's workspace configuration. Its dependencies are:

```toml
[workspace]

[dependencies]
scope-language = { path = "../../crates/scope-language" }
scope-ide = { path = "../../crates/scope-ide" }

[dev-dependencies]
scope-types = { path = "../../crates/scope-types" }
scope-execution = { path = "../../crates/scope-execution" }
cell = { git = "https://github.com/nate-droid/cell", tag = "v0.0.1" }
wat = "1"
```

The library defines `CalcLanguage` and static `LANGUAGE`. Its shared analyzer extracts `def` names and diagnoses missing names; both host binaries use that library service. The fixture is `examples/downstream/fixtures/main.calc`.

From the Scope root, run the headless consumer:

```sh
cargo run --manifest-path examples/downstream/Cargo.toml --bin headless
```

Or start the Calc-aware Scope shell:

```sh
cargo run --manifest-path examples/downstream/Cargo.toml --bin calc-scope -- examples/downstream/fixtures open
```

Then inspect the fixture:

```text
open main.calc
language
outline
diagnostics
```

The CLI fixture argument above is relative to the Scope root. The headless binary's default fixture root is manifest-relative instead.

## Validate and extend

From the Scope root:

```sh
cargo run -p xtask -- check-downstream
```

The check tests and runs the standalone language consumer and scripted harness, and tests a freshly generated language crate with its runner, exercising integration outside Scope's workspace. The language CLI runners are smoke-tested with `--help`; the harness runs its approved-edit and denied-edit scenarios using temporary fixtures. These checks make no model requests. Pass `--offline` after `check-downstream` to use cached dependencies only. Generated crates use Scope's resolved dependency versions and a shared build directory, and are removed on success (retained on failure for inspection).

This does not imply that Scope crates are published or that their local path dependencies are portable. Cell is fetched from its tagged Git source.

For scaffolding, API details, and analyzer design, continue with the [Language Designer Guide](LANGUAGE_DESIGNER_GUIDE.md). Run Scope host tests from the Scope root:

```sh
cargo test --workspace
cargo test -p scope-ide --features tui
```

The standalone-consumer checks cover native host integration, including read-only write denial and path rejection, plus the downstream-owned WASM binding test described above. `check-downstream` runs that test through the consumer's `test --all-targets` invocation; the scripted harness itself still does not execute typed pipelines. `cargo test --workspace` also runs the `scope-execution` tests for real component execution, type/capability rejection, export ABI checks, and missing-import link failures. For focused validation, run `cargo test -p scope-types -p scope-execution` and the pipeline demo above.

Scope still does not load WASM components as `LanguageService` plugins. For your own transformation guests, follow [Building Projects with Cell](BUILDING_WITH_CELL.md), keep WIT dependencies synchronized, and test both successful execution and missing-capability rejection. Add payload-schema, malformed-output, and runtime-denial tests for the authority your host installs.
