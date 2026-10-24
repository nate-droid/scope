# T2b: opt-in terminal agent semantics

## Host composition API

Enable `scope-ide/agent` (which includes `tools`). No TUI feature or external model is required. Register a `LanguageService` in the existing `LanguageRegistry`, build an App with that registry, and explicitly open a document. Call:

```rust,ignore
use scope_ide::agent::AgentLimits;
use scope_ide::cli::{run_terminal_agent_model, TerminalSemanticConfig};

// app owns an explicitly opened document; model implements DecisionModel.
// authorize is a host-owned FnMut(SemanticAuthorization<'_>) -> bool.
let allowed = vec!["inspect".to_owned(), "candidate".to_owned()];
let response = run_terminal_agent_model(
    &mut app,
    None, // no interactive-session inspection provider
    &mut model,
    "Inspect this source and evaluate a disposable candidate",
    AgentLimits::default(),
    Some(TerminalSemanticConfig {
        allowed_operation_ids: &allowed,
        authorize: &mut authorize,
    }),
)?;
```

This exact call pattern is executed by `run` in
`crates/scope-ide/src/cli/agent_semantics_tests.rs`. That file supplies a complete
fixture service and injectable deterministic model and is the executable host
example. A configured `CellAgent` also implements `DecisionModel`; backend
configuration is unchanged. Hosts need not copy session/provider internals.

The existing terminal composition point was `cli::run_cli_agent_model`:
`AgentSession::builder().app(app).provider(ScopeToolProvider::for_session())`,
plus optional interactive inspection. It now delegates through the public API
with `None`. Existing CLI commands and shell behavior remain opt-out by default;
there is deliberately no new CLI flag or ambient authorization. Downstream hosts
opt in by calling the public terminal-run API, not by registering a language alone.

## Selection, source lifetime, and scope

`Some(config)` requires the existing App document and selects its matching
registered service with `App::open_language`. No document and no matching service
return distinct denial context. There is no neutral-language, highlighted-file,
filesystem, or selected-scratch fallback. Empty catalogs install no semantic tools.

`DocumentSource` borrows the original document for the whole synchronous run.
The binding includes its document identity, exact unsaved text, version, language,
whole-document permitted scope, and an isolated `SourceEditController`. The host
policy compares the invocation to the captured source and operation descriptor.
The existing provider performs argument validation, pre-invocation freshness and
post-invocation identity/text/version/language checks. No alternate revision model
or disk read is introduced. Exclusive borrowing prevents normal App document
mutation or reopening during a run. Reopening the same path between runs creates
a different identity; a policy bound to the old identity refuses it.

## Catalog and authority

The entire native catalog is validated by `SemanticToolProvider::new`, including
excluded operations: empty and duplicate IDs fail closed. Projection advertises
only explicitly allowlisted language-local IDs with `Read` effect and `Document`
target. Names remain `language.<language-id>.<operation-id>`. Descriptions,
argument schema, purpose, constraints metadata, and output descriptions are
forwarded unchanged. Projection also denies hidden dispatch, rather than merely
hiding definitions.

Semantic mode intentionally omits **all** generic workspace/document providers.
This is a narrower surface than ordinary terminal mode, not replacement of its
implementation. Optional explicitly supplied interactive inspection remains
available. No edit, acceptance, shell, filesystem-write, or proposal tool is
installed. The isolated controller cannot register proposals from a Read result.

There are two independent policy boundaries: the ordinary session read-only
policy and the semantic host callback. The callback must independently approve
operation identity, effect, source identity/revision/text, arguments, and permitted
scope. The bridge enforces the captured source, descriptor, allowlist, Read effect,
and document scope before consulting the callback. The callback can impose
additional argument restrictions or deny candidate evaluation. Discovery grants
nothing. T1 human confirmation is neither consumed nor consulted by this API.
A semantic Read declaration is not an OS sandbox or a Scope validation certificate.

## Parameterized calls and feedback

Use the existing provider envelope, not a new terminal grammar:

```json
{
  "expected_version": "1",
  "expected_source": "disk unsaved é",
  "arguments": {"candidate": "disposable candidate"}
}
```

The initial model context supplies the exact bound text and version. The native
argument validator retains required fields, unknown-argument rejection, string
and integer validation, and target restrictions. Parameterized operations are
not subjected to T1's argument-free restriction.

The existing agent protocol preserves structured tool results and errors:
a successful dispatch may contain native `status: rejected` plus diagnostics.
Authorization denial, invalid arguments, stale preconditions, provider projection
failure, model transport failure, and budget termination remain distinct from
that native outcome. The bridge does not reinterpret domain status as transport
failure or grant certificates from observations.

`ToolValue` supports signed 64-bit integers, not arbitrary JSON numbers. Bounded
representable fixture values pass faithfully. Floating values and unsigned
integers above `i64::MAX` produce the provider's explicit `internal_failure`
projection result: no coercion, truncation, or stringification is added. The
provider may retain a native observation internally, but this terminal API does
not deliver otherwise unrepresentable native values to the model. A future
lossless numeric transport needs a separate protocol design. Existing message,
conversation, decision, and tool budgets are unchanged.

## Deterministic trace and regression coverage

`discovery_and_live_unsaved_calls_preserve_document_disk_and_authority` runs the
real agent loop with no network:

1. Initial catalog contains `inspect` and parameterized `candidate`; excluded edit
   and selection operations are absent. Candidate-purpose metadata survives.
2. Model invokes `inspect` with the exact unsaved snapshot envelope.
3. Model invokes `candidate` with its required string argument.
4. Structured feedback includes domain rejection and native diagnostics.
5. Model completes; document identity/version/text and disk contents are unchanged.

Additional bridge tests exercise explicit policy denial, hidden operations,
missing/wrong/unknown arguments, stale text/version, no document, no matching
service, empty catalog/allowlist, malformed IDs, unsupported numbers, model
transport errors, all six run-budget stops, reopened identity and conflicting App
highlight. Shell scratch selection is not accepted by this API; shell-local
scratch state cannot redirect it. Existing source-provider and T1 suites cover
post-execution freshness, identity replacement, human confirmation isolation,
proposal rejection, and language-native transport errors.

## Limits

This is a synchronous, explicitly called host composition API, not automatic
semantic enablement for every built-in shell agent command. Semantic mode does
not retain generic filesystem readers. The caller supplies policy and limits
per run. Native providers remain trusted in-process implementations; semantic
effect declarations do not sandbox malicious language services.
