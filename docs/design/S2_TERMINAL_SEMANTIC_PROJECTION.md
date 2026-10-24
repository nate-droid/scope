# S2 — Terminal semantic argument projection

## Scope and public API

This API supplies an explicitly selected terminal-only projection. The host binds the invocation source; the model supplies only registered language arguments. The raw semantic provider envelope is unchanged; the provider also offers a separate [projected-argument opt-in](../AI2_PROTOCOL_ROBUSTNESS.md).

With `scope-ide/agent` enabled, `scope_ide::cli` exports:

```rust
pub fn run_terminal_agent_model_with_projected_arguments(
    app: &mut App,
    interactive: Option<&InteractiveSession>,
    model: &mut impl DecisionModel,
    request: &str,
    limits: AgentLimits,
    config: TerminalSemanticConfig<'_>,
) -> Result<AgentResponse, AgentError>;
```

`TerminalSemanticConfig` is mandatory for this entry point and unchanged:

```rust
pub struct TerminalSemanticConfig<'a> {
    pub allowed_operation_ids: &'a [String],
    pub authorize: &'a mut dyn for<'b> FnMut(SemanticAuthorization<'b>) -> bool,
}
```

There is no implicit grant and no new config field that would break existing struct literals. An empty allowlist grants no semantic operations.

Legacy behavior remains the default. `run_terminal_agent_model(..., None)` uses the existing generic terminal surface; `run_terminal_agent_model(..., Some(config))` uses the existing semantic envelope. Only calling `run_terminal_agent_model_with_projected_arguments` selects projected arguments. This does not introduce a CLI flag or globally change direct `SemanticToolProvider` consumers.

## Downstream composition

The following helper accepts an already-configured application and decision model. Operation IDs and authorization are supplied by the host rather than invented by the example:

```rust
use scope_ide::{
    agent::{AgentError, AgentLimits, AgentResponse, DecisionModel},
    app::App,
    cli::{
        TerminalSemanticConfig,
        run_terminal_agent_model_with_projected_arguments,
    },
    tool::semantic::SemanticAuthorization,
};

pub fn inspect_with_projected_arguments(
    app: &mut App,
    model: &mut impl DecisionModel,
    request: &str,
    limits: AgentLimits,
    allowed_operation_ids: &[String],
    authorize: &mut dyn for<'a> FnMut(SemanticAuthorization<'a>) -> bool,
) -> Result<AgentResponse, AgentError> {
    run_terminal_agent_model_with_projected_arguments(
        app,
        None,
        model,
        request,
        limits,
        TerminalSemanticConfig {
            allowed_operation_ids,
            authorize,
        },
    )
}
```

Before calling the helper, the host must:

1. Register its language service using `App::builder(workspace).language(service)`, `.languages(services)`, or `.language_registry(registry)`, then `.build()`. Registration uses `&'static dyn LanguageService`. The CLI also has `run_with_languages`, but registration alone does not select the new projected runner.
2. Open the intended document through its application workflow. The runner uses `App::open_document()` and `App::open_language()`; the latter resolves the registered service for that document. It does not let the model select a different document or service. Missing documents or matching services fail closed rather than falling back to generic tools.
3. Supply a configured `DecisionModel`, including backend endpoint, credentials, model selection and transport policy as applicable. This API accepts a model; it does not select or construct a backend.
4. Choose language-local operation IDs from that service's catalog and implement a deliberate authorization policy. Do not treat discovery metadata or a model request as a grant.

`None` in the helper disables optional interactive inspection. A host that already owns an `InteractiveSession` may pass `Some(&session)` directly to the public runner. The caller also owns `AgentLimits`; a non-completed stop becomes `AgentError::Stopped`.

## Selection, schemas and dispatch

Production implementation: `crates/scope-ide/src/cli/agent_semantics.rs`, re-exported by `crates/scope-ide/src/cli.rs`.

The runner borrows the actual open in-memory document for the run, including unsaved text. It does not substitute a disk snapshot or a cloned document. It captures an invocation baseline and exposes only operations satisfying all three conditions:

- The language-local operation ID is explicitly allowlisted.
- The registered effect is `Read`.
- The registered target requirement is `Document`.

Filtering applies to discovery and dispatch. Hidden calls are denied before reaching the semantic provider. Generic writes, proposal tools and filesystem tools are not added to this semantic surface; optional interactive inspection is separate.

For each exposed tool, the private `LanguageArguments` decorator derives input definitions from the registered argument metadata, preserving names, descriptions, kinds and requiredness. It replaces metadata's `invocation_schema` with the original envelope schema's `arguments` subschema. Required fields, types and `additionalProperties: false` remain language-schema constraints. Namespaced tool names, operation descriptions, effects, targets, purpose, constraints, candidate-evaluation hints and output descriptions remain owned by the underlying provider.

The entire model argument map becomes the internal envelope's `arguments` object. The adapter supplies `expected_version` as the original revision string and `expected_source` as the original text. It invokes `SemanticToolProvider`, not the language service directly, retaining provider validation and authorization.

For example, an operation with one registered string argument `candidate` is called with `{"candidate":"disposable source"}`, not with `{"expected_version":"…","expected_source":"…","arguments":{"candidate":"…"}}`. Argument-free operations receive `{}`. Envelope-looking keys cannot override host preconditions: they remain language arguments, rejected if undeclared. If a language explicitly registers such a name, it still has no host-binding authority.

## Binding and source freshness

The baseline is captured once; it is not refreshed between calls. Terminal authorization requires the same captured source identity, revision, text and language, as well as membership in the selected operation descriptions, before invoking the host callback. The existing semantic provider supplies its preconditions, scope enforcement and post-execution checks. A stale target must not silently rebind to a new source.

The adapter constructs this informational binding:

```text
owner: app-document
source_id: debug-formatted source identity
original_revision: original revision as a string
language: selected language ID
role: invocation_baseline
```

`role: invocation_baseline` means exactly that: it describes the source against which invocation was bound. It is not proof of current source state, successful execution, authorization, or a durable identity token. In particular, a stale/error result carrying this binding does not attest that the current source still matches it.

The adapter attaches `host_binding` to discovery metadata and object-valued feedback, including structured failures. It is a separate host field, outside the language-owned `value` and evaluation data. The model never supplies or echoes it. Source identity is tagged and run-local; this concise label does not include a display path. Hidden calls rejected by session dispatch have no semantic observation or binding feedback.

## Feedback and authorization

Structured `invalid_arguments` feedback is projected along with discovery: it receives the language-only `invocation_schema`, retry guidance to submit only language arguments, and removal of the legacy instruction that language fields belong inside an `arguments` envelope. The model is not asked to reconstruct or preserve source preconditions. Other status, diagnostic and domain-outcome meanings are retained; a candidate's domain rejection is not converted into an invocation failure.

The allowlist and host callback are separate gates, alongside the agent's read-only policy. The callback receives `SemanticAuthorization`, including the real source, operation description and validated language arguments. A false decision remains unauthorized; malformed arguments do not become authority-bearing requests. In particular, `Read` candidate evaluation still requires an explicit host decision about execution authority. Read declarations are trusted native contracts, not process isolation.

## Limits and transport

- The selected service is the same borrowed service object for the run, not a registration-generation token. There is no new registry or durable service identity protocol.
- Operation selection captures catalog descriptions, while language dispatch can reread the catalog. This assumes stable catalog behavior; projection does not close the existing stateful-catalog gap or prove that a later description equals the authorized one.
- Existing check ordering is retained. Construction checks language compatibility, terminal policy binds captured language, and provider postchecks detect stale output. There is no new atomic transaction or immediate post-authorization freshness check for malicious interior-mutating native code.
- The model still receives source context: the runner includes original revision and source text in the request. Projection removes model bookkeeping, not source disclosure. Hosts must approve backend privacy, network and retention implications.
- This is a tool-provider/catalog projection in the existing decision loop, not a new wire protocol, backend adapter, native function-calling integration or sandbox. Backends and transports must preserve the existing catalog/action/feedback contract; endpoint compatibility and live behavior need separate validation.
- S2 does not redesign proposal acceptance, general editing, candidate execution isolation, or transport authentication.

## Tests and validation status

The regression coverage now lives in `crates/scope-ide/src/cli/agent_semantics_tests.rs`:

- `projected_language_arguments_preserve_host_authority_and_source` uses the production public runner with an `App`, registered fixture service, unsaved document and deterministic decision model. It covers language-only discovery, argument-free calls, candidate outcomes, host denial, malformed/missing/extra arguments, forged envelope-looking fields, hidden operations, callback inputs, and unchanged document/disk contents. This is a production-path fixture, not a live model/backend test.
- `projected_language_arguments_stale_identity_revision_text_and_language_fail_closed` uses a synthetic interior-mutable `SourceTarget` and the private projection/provider composition to induce stale identity, revision, text and language. It exercises stale-target checks without claiming those mutations are possible through the public runner's exclusive `App` borrow.
- Existing filtering and hidden-dispatch tests remain relevant to the unchanged authority boundary.
