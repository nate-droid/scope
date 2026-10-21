# Terminal-first interaction investigation

Status: investigation only, 2026-10-22. No runtime interfaces changed. Commands marked **proposed** are not runnable today. This document describes the checked-in implementation, not an assumed common shell/TUI/agent host.

The cooked-editor description below records the investigation baseline. TTY
`:scope edit` now uses a [raw multiline editor](../scratch-editing.md);
`:scope edit --commands` retains the legacy workflow. See the
[source-duplication diagnosis](SCRATCH_EDITOR_DIAGNOSIS.md).

## 1. Recommendation

Make the existing IDE shell the primary human command stream. Keep language input routed through `InteractiveSession::route`, Scope commands under `:scope`, and the existing input editor, history, and completion. Add a narrow semantic read command using the existing inspection bridge before doing layout work. Recommend **B: terminal plus one optional contextual view**, with an inline-only fallback. Source, detailed results, and proposal review are mutually exclusive contents of that view, not permanent panes.

The shared foundation is source-bound operations and explicit authority, not a universal dispatcher. Reuse `LanguageService::semantic_operations`, `SemanticToolProvider`, source targets, and existing edit controllers. Human confirmation and agent permission remain different policies. In particular, wiring a human command does not wire an agent capability.

Three concrete gaps dominate:

1. Human semantic invocation exists in the TUI but not the stock shell; its bridge supports only argument-free document reads.
2. The normal terminal agent composes Scope tools and optional interactive symbol inspection, **not** language semantic tools.
3. “Current source” has several unrelated owners. In particular, bare shell `open` and `:scope open` do not open the same owner. Layout alone cannot fix this.

## 2. Evidence and current architecture

Paths below are repository-relative. Line locations are useful starting points, not stable API identifiers.

| Layer | Actual implementation and relationship |
| --- | --- |
| CLI / shell | `crates/scope-ide/src/cli.rs`: CLI parsing near 1725; shell loop near 406; explicit buffer interception near 493; command parser near 1260; completion near 1373. One bespoke Scope command parser, not an OS shell. |
| Input | `crates/scope-terminal/lib.rs` supplies line buffers, history navigation, paste handling, and a public single-line reader. `crates/scope-ide/src/shell_input.rs` builds a separate multiline terminal editor using shared primitives. They are not identical readers. |
| Interactive language | `crates/scope-language/src/interactive.rs` defines owned session/provider contracts; `crates/scope-ide/src/language/interactive.rs` owns active session lifecycle and routes commands versus source. Registry interactive providers are separate from document analysis services. |
| App document | `crates/scope-ide/src/app/mod.rs` owns workspace, highlighted file, one open `Document`, language registry, and workspace proposals. File highlight is not open-document selection. |
| Shell buffers | `crates/scope-ide/src/scratch.rs`; shell-local `ScratchBuffers` owns selected scratch/workspace buffers independently of App. Buffer evaluation uses exact in-memory source/version. |
| Language operations | `crates/scope-language` metadata and validated dispatcher; `crates/scope-ide/src/tool/semantic.rs` binds a language service to a `SourceTarget`, edit scope, optional selection, controller, and call-aware policy. No new semantic registry is needed. |
| Human semantic bridge | `crates/scope-ide/src/semantic_inspection.rs:40–183`: discovery, immutable capture, freshness/catalog rechecks, explicit policy, existing semantic provider. `tui/semantic.rs` presents it. |
| TUI | `crates/scope-ide/src/tui/mod.rs`, `panes.rs`, `quick_open.rs`: pane-based navigation and modal inspection, not a command terminal. Public render/input APIs allow host-owned loops. |
| Agent composition | `cli.rs:186–218` builds `AgentSession` with `ScopeToolProvider::for_session()` and optional `InteractiveInspectionProvider`; uses default limits and dispatch `read_only_policy`. `agent.rs::CellAgent::run_app` uses `AgentSession::for_app`, also not semantic composition. |
| Other operations | `tool/operations` and `document/session` support a capability-backed operation registry and `DocumentSession`. Optional `tui/workbench.rs` uses these, not the App buffer. These are parallel abstractions, not automatically language semantic operations. |
| Script | `crates/scope-script/src/repl.rs`, `host.rs`, `completion.rs`: separate interpreter/REPL; `RegistryHost` projects an existing `OperationRegistry` into `scope.operations()` / `scope.invoke(...)`. It does not register or dispatch language semantics by itself. |

### Stock versus embedding

The default IDE language registry installs `SimpleLanguage` analysis. It does not supply a stock interactive evaluator or automatically install semantic agent tools (`language/mod.rs:32–107`). A service with no operations legitimately discovers an empty catalog; illustrative languages are not built-ins.

The stock shell can be entered with `cargo run -p scope-ide -- --workspace PATH open` (`shell` is an alias). CLI families are `list` (default), `capabilities`, `authority`, `meta`, `show`, `open`/`shell`, feature-gated `agent`/`ask`, and feature-gated `tui`.

An embedding can register services/providers, use `run_with_session`, or compose `AgentSession` providers. `run_with_session_and_command` supplies a trusted read-only host callback invoked only by `:scope host NAME --selection LINE:BYTE..LINE:BYTE`. It receives a borrowed live App document and validated selection, not ambient workspace/runtime authority. This is not a stock semantic command or generic operation catalog.

### Input semantics already worth retaining

With no active language, ordinary text is a Scope command. With a language active, ordinary text is language source; `:scope COMMAND` always escapes to Scope. Prefix recognition is literal (`:scope` or `:scope ` at byte zero), not arbitrary whitespace normalization. Bare `quit` becomes source in a language session.

The IDE editor supports multiline completeness, Ctrl-J newline, Ctrl-S submission of invalid (but not incomplete) input, Ctrl-C draft cancellation, empty Ctrl-D EOF, and completion menus. Enter in a completion menu accepts without executing. Ctrl-P/N recall history; Up/Down first navigate multiline input. History records exact nonblank submissions, including failures; persistence is opt-in through workspace-authorized `SCOPE_HISTORY_FILE`, bounded by `SCOPE_HISTORY_LIMIT` (default 1,000) and storage limits. Redirected/embedded input bypasses terminal history and reads physical lines. Session completeness/completion/inspection must not evaluate source.

Command completion shares the parser's name declaration but is currently predominantly command-name completion, not path/argument/semantic-ID completion. Extend that existing route, not a second parser.

Scope Script uses the simpler `scope-terminal` reader with single-line submission, its own history and completion. Its standalone CLI uses `NoHost`: `scope.operations()` is empty and `scope.invoke` has no authorized provider. Capability examples explicitly embed a host (`examples/registry.rs`, `examples/workspace.rs`). `RegistryHost` exposes real descriptors/schemas, requires host policy and expected `DocumentSession` revision, permits reads only, and bounds retained calls/results. Script's operation registry and document session are **not** the semantic provider and App revision model. Do not route the shell through Script merely to get command invocation.

## 3. Human workflow map

“Embedding” means possible with explicit native host configuration, not automatically present in every host. Agent entries require the `agent` feature and configured model infrastructure; the TUI requires `tui`.

| Workflow | Actual action / command | Ownership and authorization | Presentation | Stock shell | Stock TUI | Embedding |
| --- | --- | --- | --- | --- | --- | --- |
| 1. Open document | Bare `open PATH`; explicit `:scope open PATH`; TUI Files Enter/`o` or Ctrl-P picker | Bare opens App document through workspace read authority; explicit opens/selects shell buffer. Reopening App allocates a new identity. TUI refuses switching away from dirty source; that guard is not intrinsic to `App::open_path`. | Shell status; TUI source pane | Yes, two owners | Yes, App owner | App or buffers, explicitly |
| 2. Inspect contents | Bare `show`/`cat`, `lines START END`; `:scope show`; scroll Document pane | App versus selected shell buffer respectively; unsaved in-memory text, not necessarily disk | Inline text or wrapped pane | Yes | Yes | Yes |
| 3. Evaluate source | Select `:scope session ID`; ordinary source; `:scope buffer eval` for selected buffer | Host-registered evaluator required. REPL submissions get fresh language source IDs/revision zero; buffers retain source ID/version. Buffer language must match active session. No implicit save or reset. Native provider is trusted, not sandboxed by session API. | Session output, value, diagnostics/error | Framework yes; stock CLI has no evaluator registered | No stock evaluation action | Yes, register/create provider |
| 4. Invoke semantic read | F8 (Ctrl-I where distinguishable), select, Enter capture, Enter authorize | App document, selected path-resolved service. Only `Read` + document target + **no arguments**, even optional ones excluded. Exact captured request and current catalog rechecked. | Modal feedback/native structured JSON | No generic command; `:scope host` requires embedding callback | Yes for eligible registered operations; stock language may expose none | General provider with explicit scope/policy |
| 5. Diagnostics / structured details | `diagnostics`/`diag`, `outline`/`symbols`; TUI Language pane; F8 result | App analysis is distinct from semantic read feedback and interactive evaluation diagnostics. Semantic result binds identity/text/version/language. | Inline summaries; Language pane; scrollable modal CURRENT/STALE | Analysis yes; generic semantic result no | Yes; no generic expandable tree | Native results available; host renders |
| 6. Agent question | `agent REQUEST` / `ask REQUEST`; CLI equivalent | App context + Scope tools, optional active session symbol inspection. Read-only dispatch; no implicit selected scratch-buffer semantic context. Remote model may receive source; existing limits apply. | Shell text / structured tool feedback to model | Yes with configuration | No default agent pane/action | Compose session/model/providers |
| 7. Candidate / proposal | `agent edit "JSON-quoted instruction"`; `source-propose JSON`; `proposal PATHS...`, `proposal-agent` | Distinct agent edit lifecycle, selected-buffer proposal controller, and isolated workspace snapshots. A proposed candidate is not active source. Configured semantic edit branch requires strict host criteria, capabilities/budgets, policy; not unevaluated fallback. | Proposal IDs, status/review/diff | Yes, with prerequisites; not a universal language-operation command | No default proposal workflow; optional Workbench is separate | `edit_lifecycle::run_semantic_edit`, semantic provider/controller or operation Workbench |
| 8. Review / authorize edit | `source-proposals`, `source-proposal ID`, `source-diff ID`, `source-accept ID`, `source-reject ID`; workspace `proposal-review`, `proposal-accept`, `proposal-discard` | Source acceptance rechecks matching target and changes memory, not disk/evaluator. Save is separate. Workspace acceptance writes a snapshot's changes sequentially, can partially succeed, refuses dirty App source, and does not silently reload an open buffer. | Diff and explicit status | Yes, separate lifecycles | Not in default panes; embedding Workbench supports confirmations | Explicit controller/target authority |

Shell source editing also includes `replace`, `dirty`, `save`/`write`/`w`, `revert`; explicit buffer `edit`, `save`, `save-as`, `reload`, `reload-discard`, `close`, `discard`. `:scope edit` is a transactional cooked-mode editor (`cli/buffer_editor.rs`): `:at`, `:insert`, `:delete`, `:show`, `:done`, `:cancel`. Commit changes memory only; EOF/cancel drops the draft. This is another interaction mode, not a new terminal framework.

Coordinates differ today: buffer editor columns are one-based Unicode scalars; legacy replacement uses one-based UTF-8 byte columns; host selections/proposal ranges use zero-based UTF-8 byte columns. Future command help must state units, not silently translate user intent.

## 4. Capability connections that are missing

- Shell analysis commands and session `describe` / `effect` / `where` are not semantic operation discovery/invocation. `LanguageAction` display is not semantic dispatch either.
- F8 already composes the semantic provider, but cannot accept operation arguments, selections, or proposing operations. Reuse its constrained bridge first; do not imply all operations work.
- Shell buffer selection and App document selection are independent. The agent uses App context, while source-proposal commands use selected buffers. A successful `:scope open` does not prove the normal agent sees that source.
- App has no persistent source-edit controller. Inspection creates a temporary one because the provider requires it, but read results cannot register edits. General proposal-enabled hosting needs a retained controller, not that temporary object.
- Workbench and Script can invoke their operation registry, but do not thereby receive language semantic providers. Their verification/candidate-diagnostic callbacks must not be discarded to make composition look uniform.
- Rich semantic JSON does not fit every agent `ToolValue`: floats and unsigned values above `i64::MAX` cannot be represented. The provider retains validated native results and the TUI displays them; stock model feedback can still fail conversion. “Structured results reach the model” must be tested, not inferred from successful native display.

## 5. Proposed command-first interaction

### Narrow initial vocabulary (proposed, not implemented)

Add `:scope inspect` to the existing Scope command enum/parser and completion:

- `:scope inspect` lists eligible read operation IDs, descriptions, target/effect, language, and the **App document** identity/version/path; empty catalog is explicit.
- `:scope inspect OPERATION_ID` captures that operation/source and prints a concise authorization request. Invocation requires an explicit confirmation bound to that capture; source/catalog changes invalidate it. Do not insert an unscoped global yes/no handler that consumes language source.
- `:scope inspect result` prints the retained summary and freshness; `:scope inspect result --full` prints validated native details. Exact syntax and reserved-ID handling must be settled in parser tests before implementation; these are a design sketch, not final API promises.

First implementation targets App to reuse `InspectionBinding::capture_app/invoke_app` without pretending scratch selection is unified. Help and every capture must say “App document”; when selected shell buffer differs, show the mismatch and do not fall back silently. The command is useful with bare `open` and downstream registered semantic services; stock no-operation languages get discovery, not invented `check`/`goal`/`evaluate` operations.

This is deliberately narrower than a final consistent-source interface. The next source-selection work should make `App` versus selected buffer explicit and eventually present one host-selected source handle to all commands. It must not silently change existing `:scope open` semantics in milestone one.

### Generalization after the narrow bridge

Keep `:scope` routing and extend the same command parser with metadata-driven ID/argument completion. JSON object arguments can use existing serde/schema validation rather than an expression grammar; the envelope parser must preserve the raw JSON tail, not split strings on whitespace. Selection comes from an explicit validated source range, not guessed cursor state.

Read catalog entries directly from the selected language service. Render original effect, purpose, schema, target and opaque constraints. Do not maintain renamed copies such as universal `check` and `goal` commands. Unsupported operations remain discoverable with a reason, but are not invokable by an interface that cannot satisfy their target/policy.

Simple feedback is inline: operation, source identity/revision, status, compact value/diagnostic counts, result handle, CURRENT/STALE. Full values, rationale, observations, evidence and diffs are available explicitly. Transport success is not semantic success: `unauthorized`, `blocked`, `revision_conflict`, `validation_failed`, and `rejected` must remain visible. Semantic observations are not automatically validation certificates or analysis overlays.

### View comparison

| Criterion | A. Inline terminal only | B. One optional context view | C. Source + detail views |
| --- | --- | --- | --- |
| Everyday commands | Direct, least UI | Equally direct with view closed | Command area competes with two views |
| Keyboard / focus | One input focus; scrollback inspection | Command focus by default; explicit enter/leave context | Terminal/source/detail traversal, stronger focus policy required |
| Small terminal | Best for short results; large JSON floods history | Closed by default; full-screen overlay at small sizes | Poor without collapsing into B |
| Goals / diagnostics | Full dump/range output; weak navigation | Reuse scrolling; future expand/collapse in one detail surface | Good simultaneous reference, expensive default |
| Proposal review | Inline unified diff works but loses context | Replace context content with source/diff/evidence, explicit confirmation | Best wide-screen comparison, not necessary baseline |
| Existing components | Reuses shell/output directly | Can reuse TUI paragraphs/modal rendering behind a host-owned loop | Reuses panes superficially, requires most layout/focus work |
| Cost | Lowest; viable first milestone | Smallest coherent richer destination | Highest; defer until evidence demands it |

**Choose B**, staged through A. Do not embed a command editor under current global `q`/Esc/Tab interception. A future host loop must route text input to the existing editor first; only documented context controls act globally. Opening a context view must not run an operation. Closing it must restore the exact command draft/cursor. No permanent file list, diagnostics dashboard, goals pane or agent chat pane is required. Inline result/detail commands remain usable without ratatui.

## 6. Shared human and agent operations; separate authority

Reuse the semantic provider's actual operation catalog and validated dispatch. The common unit is a host-resolved service plus source target, exact source preconditions, arguments, scope, selection, and policy—not a tool name alone.

| Boundary | Required rule |
| --- | --- |
| Discovery | Metadata visibility grants no execution or external capability. Preserve schemas, effect/purpose/constraints rather than summarizing away restrictions. |
| Human read | Explicit captured-request approval, recheck identity/text/version/language/catalog; no implicit edit authority. |
| Agent read | Host selects exposed providers and allowed operation set; outer dispatch policy **and** semantic call policy must approve. Human approval does not carry over. |
| Candidate evaluation | Disposable candidate text is an argument/observation, never an automatic active document/session replacement. A read-purpose candidate evaluator can execute native code; policy must account for that. |
| Propose | Separately enabled `ProposeChanges` scope, host criteria/capabilities/budgets, candidate/contract validation and evidence rules. Provider registration creates a proposal, not an accepted edit. |
| Accept / persist | Host/human accept rechecks exact target revision; accept is distinct from save and evaluation. Neither source acceptance nor filesystem write capability is inferred from agent read permission. |

`Read` is a trusted native provider declaration, **not an OS sandbox guarantee**. Opaque operation constraints do not create an enforceable filesystem/network capability schema. Synchronous native operations can block or have external effects; no new Cell guarantees, capabilities, resource isolation, or cancellation claims follow from this design.

Preserve existing agent limits (`agent_loop.rs:32–55`): 12 decisions, 8 tool calls, 2 malformed decisions, 64 KiB/message, 256 KiB/conversation, 1,500 tokens/decision by default. These are count/byte limits, not wall-clock or billing bounds. Denied/unknown calls still consume limits.

## 7. Separate blocker: selected-source terminal agent composition

This is independent of terminal layout. The smallest generic change is an **opt-in, source-bound semantic provider composition path** in the terminal agent host, initially read-only:

1. Select the actual source owner before the run. For the existing agent this is `App::open_document`, not highlighted file or selected scratch buffer. Fail clearly when none is open. Once buffer-aware selection exists, resolve that explicit handle instead.
2. Resolve through `App::semantic_service_for_document` (first registered path match, neutral fallback), not active interactive language ID. Verify target/service language agreement.
3. Borrow the live document through `DocumentSource` and bind identity/text/version/path with existing source-target types. Do not reread saved disk text. Install `SemanticToolProvider` through `AgentSession::builder().provider(...)`, alongside only the existing explicitly intended providers.
4. Use a host-owned controller for the provider lifetime. For read-only runs it can be isolated and temporary. For future proposing runs retain it after dropping the agent/provider borrows so humans can review. `edit_lifecycle::run_semantic_edit` already demonstrates semantic session construction and borrow release before review.
5. Require an explicit agent semantic policy keyed to operation, arguments, effect/purpose, source and scope. `read_only_policy` alone is insufficient approval: it only checks advertised `ToolEffect`. Provider construction advertises operations including writes, so initially hide ungranted operations with a provider projection or clearly mark them unavailable **and** retain deny enforcement. Hiding is not authorization.
6. Keep the source stable for the run and recheck pre/post invocation. Existing Scope read tools may remain; document replacement stays denied. Reject provider name collisions through the existing builder.
7. `ToolResult` already feeds the agent loop. Preserve semantic statuses and nested structured values; do not flatten to prose. Test representable native JSON end-to-end with a deterministic model. Address the float/large-unsigned protocol gap separately, with a lossless bounded JSON transport design, before claiming arbitrary native results reach the model. Do not silently coerce numbers or bypass byte budgets.

No Path-specific service, operation names, or theorem logic belongs in this bridge. A downstream language registers its generic service and the host elects whether its operations are agent-visible. Language registration alone must not trigger permission.

## 8. Source and session consistency

Use a small host-owned **context descriptor**, not a new document store or revision counter:

| Context field | Owner / representation |
| --- | --- |
| Workspace | Existing App `Workspace` and its granted capabilities |
| Current source | Explicit tagged reference to existing App document or selected shell buffer; highlight/path alone is insufficient |
| Identity / text / revision | Existing source target snapshot; App document identity/version or buffer identity/version. No new global sequence. |
| Language session | Existing `InteractiveSession`, independent of path-resolved analysis service; expose mismatch, never auto-reset on open |
| Last semantic result | Host-retained native/structured result plus original captured binding and operation/arguments; a UI result handle is not a source revision |
| Agent context | Derived immutable description of the selected source at run start, plus explicit provider/policy set; no separate mutable source copy treated as authoritative |
| Pending proposal | Existing controller proposal ID, original target/base, candidate, checks/evidence and status; owner/lifecycle displayed |

Existing identities differ intentionally: `scope_language::SourceId` / `RevisionId` describe evaluator submissions; `source_edit::SourceId` tags edit targets; `DocumentSession::Revision` belongs to another document abstraction; disk revision guards persistence. Adapt at existing boundaries, do not declare these tokens interchangeable.

Opening another App document invalidates old bindings even if text/path match. Editing the source marks retained inspection stale; do not rerun automatically. Results can remain visible as historical feedback, but must never regain “current” from text equality alone. Freshness is not renewed authorization. A hypothetical candidate never changes current source, dirty state, session definitions, or saved disk. Candidate evidence must retain its existing bindings rather than acquiring a new host-invented revision.

Saving uses existing persistence/conflict checks. Scratch revision-checked atomic replacement is not an atomic compare-and-swap against concurrent external writers. Workspace proposal acceptance is not transactional across files. These limitations remain visible.

## 9. Documentation-level interaction prototype

**Illustrative only. This transcript is not a runnable shell session.** A proposed native fixture language `fixture-notes` handles `.notes`; one no-argument document `Read` operation `count_lines` returns `{ "lines": 2 }` for exact text `red\nblue\n`. No language evaluator, Path, model service or theorem engine is needed. A deterministic decision-model stub can return the candidate string `red\ngreen\n`; that is untrusted suggestion data, not an edit tool call.

```text
# Existing bare open selects the App document (no language session active).
open sample.notes
  App document D, version 0, clean

# PROPOSED commands and output below; fixture is a design, not a built-in.
:scope inspect
  source: App document D @0; fixture-notes
  count_lines — Read / Document / no arguments
:scope inspect count_lines
  review captured request D @0, exact source, arguments {}
  [explicit confirmation bound to this capture]
  count_lines: ok; lines=2; result R1 CURRENT
:scope inspect result --full
  R1: source D @0 CURRENT; native value {"lines":2}

# Existing agent syntax, but semantic composition/policy below is PROPOSED.
agent Suggest replacing blue with green; do not edit the document.
  host exposes explicitly authorized fixture-notes read tools for D @0
  deterministic model calls count_lines using exact source preconditions
  candidate C1: "red\ngreen\n"; suggestion only; D remains @0

# PROPOSED candidate presentation, not automatic proposal registration.
  context review: original / candidate diff
  - blue
  + green
  no source edit authorized; no save or evaluation performed
```

A candidate string is not a controller proposal. If the human subsequently submits it using existing `source-propose JSON`, that currently targets the selected **shell buffer**, not App D. This prototype intentionally does **not** pretend those targets match. A later source-selection adapter must retain the App-bound candidate/base and route submission to the correct existing controller before an App proposal acceptance workflow can be offered. A stricter alternative test can use an explicit buffer target throughout a programmatic host.

Existing runnable evidence, rather than this sketch: `crates/scope-ide/examples/interactive_semantic_inspection.rs` registers a deterministic fixture, drives real F8/Enter/Enter through `TestBackend`, edits unsaved source and shows CURRENT/STALE/CURRENT with native JSON preservation. `tests/embedded_shell.rs` verifies exact live source/selection and shared session/buffer behavior. A future combined prototype should add a deterministic `DecisionModel`, assert tool catalogs/policies and candidate non-mutation, and require no credentials/network.

## 10. Complexity inventory and migration

### Existing command and mode inventory

Beyond the CLI families above, the shell parser declares these families (aliases grouped):

- `help`/`?`, `quit`/`exit`; `files`/`ls`, `capabilities`, `authority`, `refresh`.
- `open`, `show`/`cat`, `lines`, `language`/`lang`, `outline`/`symbols`, `diagnostics`/`diag`.
- `replace`, `dirty`, `save`/`write`/`w`, `revert`.
- `agent`/`ask` (including `agent edit`), `proposal`, `proposal-agent`, `proposal-review`, `proposal-accept`, `proposal-discard`.
- `source-propose`, `source-proposals`, `source-proposal`, `source-diff`, `source-accept`, `source-reject`; explicit-only `host` callback.
- Namespaced session controls `session`, `session-reset`, `describe`, `effect`, `where`.
- Namespaced buffer controls `scratch` (new/capture and buffer management), `buffers`, `buffer` (including eval), `edit`, `save-as`, `reload`, `reload-discard`, `close`, `discard`, plus overridden `open`/`show`/`save`.

Modes include no-language command routing versus active language routing; multiline draft and completion menu; cooked transactional buffer editor; pending proposal/review; TUI normal/picker/semantic confirmation/result states. Optional Workbench adds queued invocation, approval, execution, review, accept/reject confirmations, and operation argument editing. These are not all one modal machine.

Default TUI has **three permanent panes**: Files, Document, Language (`with_default_panes`). Files gets 30% width; Document/Language split the right side 70/30. There is no default terminal or agent pane. Language actions are listed, not executable there; analysis can rerun on redraw. The document pane scrolls text but is not an editor.

| Input surface | Keys / focus transitions |
| --- | --- |
| Global TUI | Tab/Shift-Tab cycle panes; Ctrl-P opens picker; F8/Ctrl-I opens semantic modal; `q`/Esc requests quit. Modal dispatch takes precedence. |
| Files | Up/Down or k/j selects; Enter/o opens and focuses Document; r refreshes. Picker Enter opens/focuses Document; Esc cancels. |
| Document / Language | Up/Down or k/j; PgUp/PgDn; Home/End scroll. |
| Semantic modal | Up/Down selects and cancels pending capture; Enter captures then Enter approves; Esc cancels pending confirmation before closing; PgUp/PgDn/Home scroll. |
| Shell | Text editing/history/completion keys described above; no pane focus. Editor and evaluation are synchronous; no in-flight terminal cancellation loop is wired. |
| Optional Workbench | Own approval/review modes and F8 cancellation, but global TUI F8 currently intercepts it; global q/Esc/Tab also conflict with text entry. It is not a drop-in command pane. |

Duplicated interactions include file picker versus `files`/`open`, source pane versus `show`, Language pane versus analysis commands, F8 inspection versus programmatic semantics, independent buffer/App open-save paths, and several proposal systems. Humans cannot use arbitrary argument/selection semantic operations through stock inspection, evaluate through stock TUI, or invoke Script's host operations without embedding. The terminal agent cannot use selected-language semantics merely because the TUI can.

### Migration without removing working interfaces

1. Add shell discovery/read/detail using existing behavior and explicit App target labels; preserve legacy commands and TUI F8. Share capture/authorization/result rendering logic rather than duplicate validation.
2. Introduce explicit source-selection adapters and opt-in semantic agent composition; maintain compatibility names while warning about target mismatch. No automatic save, evaluator switch, or proposal migration across owners.
3. Only then prototype B behind an opt-in host loop. Files becomes command/picker; language summary becomes command/result; source/detail/review becomes one optional context surface. Keep existing pane builder and standalone TUI for embeddings. Resolve key precedence and Workbench collisions deliberately, not by hiding documented controls.

## 11. Risks and tests

- Source confusion is the largest safety risk: test App/buffer mismatch, highlighted-not-open file, reopen same path/new identity, unsaved source, language mismatch, stale capture/catalog, and candidate non-mutation.
- Authorization: discovery must never dispatch. Denial, invalid arguments, unsupported effects/targets and stale preconditions must fail before native execution. Test human grants do not grant agent reads/writes; exposing a read tool does not expose candidate/proposal/accept/save authority.
- Structured feedback: test blocked/rejected statuses, native JSON numeric edge cases, output bounds, escaped terminal control text, explicit full-detail output and freshness after edits. Lossless native display is not lossless model feedback.
- Proposal lifecycle: retain candidate/base/evidence, validate exact bindings, require independent acceptance/save, detect conflicts, preserve existing strict policies and partial-workspace-write reporting.
- UX: PTY-test actual shell completion/draft restoration/history and future focus routing. `TestBackend` tests do not prove real terminal ergonomics. Check narrow widths and closed context fallback. Preserve redirected/embedding behavior.
- Runtime: synchronous providers/model calls can block; no wall-clock cancellation guarantee. A future scheduler is separate work, not a prerequisite for a read-only shell bridge.

### Validation performed for this investigation

- `cargo test --offline -p scope-terminal -p scope-script` — passed, including Script CLI/PTY suites and terminal tests (terminal: 23 passed). No new fixture/test code was added.
- `cargo test --offline -p scope-ide --features agent,tui --test interactive_semantic_inspection --test embedded_shell` — passed: 4 TUI semantic interaction tests and 15 embedded-shell tests.
- `cargo test --offline -p scope-ide --features agent,tui` — attempted, blocked compiling existing library tests (8 errors): ambiguous/conflicting evaluator versus edit `SourceId` references in `src/intent_tests.rs` and `src/edit_lifecycle.rs` test code; incompatible `SourceTarget::source_id` signature and nonexistent `scope_language::SourceId::Scratch`. No runtime Rust files were changed in this investigation; these are not regressions introduced here. Consequently the full shell/TUI/agent unit suite is **not** reported passing.
- `cargo test --offline -p scope-ide --features agent,tui --test source_edit_controller --test agent_public_api --test source_evaluation --test cross_provider_discovery` — passed: 10 controller, 2 agent public API, 3 source evaluation, and 2 cross-provider tests.
- `cargo run --offline -p scope-ide --features tui --example interactive_semantic_inspection` — passed; existing deterministic fixture assertions and rendered frames completed. This validates the existing modal example, not the proposed command transcript.
- `git --no-pager diff --check` — passed for tracked changes. The new document was also checked directly for trailing whitespace/conflict markers; no runtime source changes were made.

## 12. Small implementation milestones (not started)

1. **Everyday human improvement: read-only shell inspection.** Add one `:scope inspect` command family to the current parser/completion, reuse `InspectionBinding`, explicit App source labels/mismatch feedback, exact request authorization and inline/native detail output. Argument-free document reads only; empty discovery is honest. Acceptance: deterministic registered language, unsaved source, deny/no-dispatch, stale source/catalog, retained full result; no agent or layout changes.
2. **Consistent source context and opt-in agent reads.** Adapt existing source owners without a new revision/store; explicit selected target visible in commands and model context. Install a target-bound semantic provider with a separate host agent allow policy, retain existing budgets, and test structured feedback. Do not enable proposals or fix protocol precision with coercion. This removes the generic composition blocker independently of layout.
3. **One contextual view and deliberate proposal review.** Opt-in B using existing render/input primitives, mutually exclusive source/detail/diff content, tested focus/draft preservation and narrow-width fallback. Integrate one existing proposal lifecycle with retained controller and explicit acceptance only after source consistency is proven; do not merge workspace writes into source acceptance.

Stop here: no framework, parser language, registry, agent framework, Cell capability, permanent dashboard, Path-specific logic, or implementation milestone is introduced by this investigation.
