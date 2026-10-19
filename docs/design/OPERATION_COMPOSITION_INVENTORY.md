# S1 — Scope Operation and Composition Inventory

## Status and recommendation

**Research only. Recommendation: narrow scope.** Scope has the foundations for a small, host-controlled composition experiment today. It does not need another tool registry or executor to establish feasibility. It does not yet offer uniformly typed, discoverable composition across language semantic operations, document operations, agent tools, and component transformations.

The smallest useful next step is a **single-document, two-call, read-only integration fixture using existing APIs**, with explicit result-to-argument wiring and host authorization at each call. Include focused discovery and negative cases. Do not begin with a scripting language, automatic candidate reconstruction, cross-document edits, or a universal operation abstraction.

### Evidence convention

- **Verified** means observed in repository implementation, not a claim that tests were run or that provider declarations are trustworthy.
- **Assessment** means a conclusion drawn from those implementations.
- **Proposal** means a future experiment, not an existing feature or approved implementation.
- Source references below are relative to the repository root. Line ranges identify the inspected implementation; symbol names remain the primary reference as files evolve.

This inventory makes no production, Cell, dependency, or downstream-language changes. No recommendations are implemented.

## 1. Current architecture and relevant API references

### 1.1 The existing surfaces are related, but not interchangeable

| Surface | Discovery / contract | Execution and authority | Source reference |
|---|---|---|---|
| Language services | `LanguageService::semantic_operations()` returns language-local semantic descriptions | Immutable `LanguageDocument`; validated semantic dispatch returns values, evidence, blockers, or proposed edits, not direct editing authority | [`scope-language/src/lib.rs`](../../crates/scope-language/src/lib.rs), lines 132–263; [`semantic.rs`](../../crates/scope-language/src/semantic.rs), lines 7–170, 297–384 |
| Revision-bound document operations | `OperationRegistry::descriptors()` returns separate provider/action IDs and recursive input **and output** schemas | `invoke()` requires a `DocumentSession`, expected `Revision`, and explicit call-aware policy; validates data before dispatch and accepted results afterward | [`tool/operations.rs`](../../crates/scope-ide/src/tool/operations.rs), lines 18–141, 184–336 |
| Agent tools | Installed `ToolProvider`s produce `ToolDefinition`s and optional metadata/output prose; `AgentSession::catalog()` snapshots actual installed tools | `ToolCall`/`ToolValue`; call-aware policy is available, but backward-compatible `invoke()` permits registered tools by default | [`tool.rs`](../../crates/scope-ide/src/tool.rs), lines 19–62, 91–209, 450–595 |
| Language-to-agent adapter | `SemanticToolProvider` exposes `language.<language_id>.<operation_id>` plus semantic metadata and invocation schema | Bound to one host-resolved source target, selection, review controller, scope, and mandatory policy; returns proposals for host review | [`tool/semantic.rs`](../../crates/scope-ide/src/tool/semantic.rs), lines 59–195, 213–299, 426–529 |
| Headless workbench | `Workbench::operations()` exposes registry descriptors | Queues a request, requires approval plus registry policy, retains proposals/reviews and revision-bound verification evidence | [`workbench.rs`](../../crates/scope-ide/src/workbench.rs), lines 18–107, 161–195, 285–419, 423–479 |
| Typed transformation graphs | Nominal `TypeId`, typed ports, `Transformation`, `Graph`, `Pipeline`; validation diagnoses wiring and planning permissions | `scope-execution::execute()` runs explicitly bound component exports through the supplied Cell; it is not a dispatcher for document operations | [`scope-types/src/lib.rs`](../../crates/scope-types/src/lib.rs), lines 1–140, `Graph` implementation; [`scope-execution/src/lib.rs`](../../crates/scope-execution/src/lib.rs), lines 1–108 and `execute()` |
| Metadata discovery | Installed graph providers and inventories; find/inspect/related/source tools | Navigation references are not source authority; source resolution is separately host-installed | [`tool.rs`](../../crates/scope-ide/src/tool.rs), `MetaDiscoveryProvider`; [`tool/meta_surface.rs`](../../crates/scope-ide/src/tool/meta_surface.rs), lines 48–158, 214–334 |

**Verified:** recursive operation schemas already exist. Saying “Scope has no typed operation output contracts” would be wrong. Saying “all language operations have those contracts” would also be wrong: the semantic and registry interfaces currently represent different contracts.

**Assessment:** these surfaces provide reusable boundaries, not evidence of a single automatically bridged operation universe. The inspected registry, semantic adapter, and graph executor do not themselves translate language semantic descriptions into registry schemas or graph bindings.

### 1.2 Language services and semantic operations

**Verified:** `LanguageService: Sync` supplies language identity/path matching, analysis, diagnostics, outline, semantic discovery/dispatch, candidate validation, and contract validation. Services operate on borrowed document text/path/version. Combined analysis can share parser work; results are not implicitly cached. `LanguageRegistry` installs services and resolves paths, with a neutral fallback (`crates/scope-ide/src/language/mod.rs`, lines 25–107).

A `SemanticOperationDescription` contains:

- stable language-local `id` and prose `description`;
- named arguments with requiredness, prose, and top-level JSON kind;
- `Read` or `ProposeChanges` effect;
- `Document` or `Selection` target requirement;
- optional opaque JSON `constraints`.

Purpose is independent of effect. `purpose()` reads the reserved `constraints.scope_operation_purpose` key, defaulting to exploration when absent or malformed. `semantic_operation_produces_evaluation()` separately advertises evaluation support. Neither is authorization or proof that a particular result establishes success.

`execute_semantic_operation()` checks that the operation is advertised, validates arguments, requires a selection when declared, and rejects change proposals returned by a read operation. Nested arguments and opaque constraints remain implementation responsibilities. The free function is reusable independently of trait-method overrides; the implementation hook is not the safe discovery/validation entry point. Core dispatch does not itself supply host authorization, live revision checks, or range validation of every proposed edit.

References: `crates/scope-language/src/semantic.rs`, lines 7–150, 355–384; `crates/scope-language/src/lib.rs`, lines 179–263.

### 1.3 Registry contracts, validation, authorization, and execution

**Verified:** `OperationSchema` is a closed recursive structural schema: null, boolean, signed-integer transport values, string, homogeneous array, and object with declared fields and a required-field set. Unknown object fields are rejected. The root input must be an object. There are no schema variants for floating point, union, enum, nominal identity, or refinements.

`OperationDescriptor` carries action identity, description, input/output schemas, `ToolEffect` (`Read`/`Write`), `OperationExecution` (`Immediate`/`Blocking`), and host-defined permission names. Execution requirements and permission declarations do not schedule work or grant capabilities.

Registration validates identifiers and schema metadata, rejects duplicate providers/actions, copies descriptors into the registry, and commits atomically with cumulative metadata budgets. IDs and field/permission names use a restricted ASCII identifier grammar, not arbitrary JSON keys. Invocation proceeds as follows:

1. Resolve valid provider/action IDs.
2. Bound input depth, node count, and text/key bytes; validate the input schema.
3. Obtain the expected live document snapshot.
4. Call mandatory policy with **call arguments, descriptor, and validated snapshot**.
5. Invoke trusted provider code synchronously.
6. Bound and validate output; revalidate the source before accepting the result.
7. Return `OperationResult { document_id, revision, value }`.

No default authorization policy exists on this registry path. The host must explicitly interpret every declared permission, effect, target, and relevant argument. An effect label alone does not establish purity or prevent execution side effects.

Default limits are depth 32, 16,384 value nodes, 1 MiB value bytes, 16,384 cumulative metadata nodes, 256 KiB metadata bytes, and 1 KiB provider diagnostic text; configured depth is capped at 128. These bound accepted data/validation, not provider runtime, allocations before return, panics, or external effects.

Reference: `crates/scope-ide/src/tool/operations.rs`, lines 1–8, 18–141, 193–336, 381–475.

### 1.4 Document targets and revision models

**Verified:** two relevant editing paths exist; their tokens must not be conflated.

- **Capability-backed document session:** `DocumentSession::open(workspace, document_id, path)` opens one confined persisted document. `DocumentSnapshot` owns ID, `ScopePath`, text, and an opaque session-local `Revision`. Token equality is pointer identity, not a numeric version or persisted save token. Reads recheck persisted source; changed/unreadable source invalidates the accepted state until explicit reload. Successful unchanged reload can preserve its token. References: `crates/scope-ide/src/document/session.rs`, lines 12–33, 66–185.
- **In-memory source target:** `SourceId` distinguishes scratch and document identities; `SourceSnapshot` borrows language/text and numeric version. `SourceTarget` exposes transactional source editing without save or evaluation. `resolve_edit_target()` honors explicit user selector, command target, active target, then unique eligible candidate; ambiguity is an error, not a guess. Selection is context, **not range authorization**: `PermittedEditScope` currently permits one whole document. Reference: `crates/scope-ide/src/source_edit.rs`, lines 12–132.

The semantic adapter requires exact `expected_version` (decimal u64 string), exact `expected_source`, and nested operation `arguments`. It validates scope, source, policy, and selection before dispatch and rechecks source afterward. Its `LanguageDocument` currently uses an empty path; path-sensitive services cannot assume their real file path arrives through this adapter (`tool/semantic.rs`, lines 213–299, 463–475).

**Assessment:** a target name or path in operation arguments is not authority to open another document. An external caller must stay bound to host-resolved targets and retain the correct token model for its chosen entry point.

### 1.5 Proposal and validation lifecycle

**Verified — persisted document path:** `OperationRegistry::propose_edit()` interprets a validated string output as an entire replacement, then calls `DocumentSession::propose()`. It retains the original call and revision-bound result as structured provenance. It does not parse, analyze, authorize application, or save (`tool/editing.rs`, lines 44–103).

The document lifecycle is:

`invoke → propose → review exact candidate → explicit host decision → authorize exact candidate → apply with checked save`

`EditProposal` has immutable base/candidate snapshots and a fresh candidate revision. `review()` attaches caller-supplied candidate diagnostics and display diff; it does not run analysis or automatically reject diagnostics. `EditAuthorization` has private fields and is bound to that exact candidate. `apply()` checks authorization, base freshness, and workspace checked-save behavior. Saving is best-effort conflict detection, **not atomic compare-and-swap** (`document/editing.rs`, lines 8–84, 110–209).

`Workbench` separates invocation approval from edit acceptance. Verification success is host interpretation of validated provider output, retained for one revision, never permission to save. The host supplies candidate diagnostics. Cancellation only removes queued work, not an in-flight invocation (`workbench.rs`, lines 1–6, 61–107, 285–419, 423–479).

**Verified — semantic source path:** `SemanticToolProvider` prepares/structurally previews returned semantic edits, uses supplied candidate validation or calls the language service when its status is `NotEvaluated`, rechecks source and policy, and registers non-rejected proposals with `SourceEditController`; it does not apply them (`tool/semantic.rs`, lines 426–529). Application to a `SourceTarget` is in-memory; it is not the persisted document session's save lifecycle.

Static candidate validation, named contract observations, and aggregate semantic evaluation are distinct evidence. Default language candidate validation is `NotEvaluated`; default contract validation supplies no observations. Provider acceptance does not establish behavioral correctness, and none of this evidence overrides host decisions (`scope-language/src/lib.rs`, lines 242–263; `semantic.rs`, lines 172–278).

### 1.6 Values, constraints, and errors

| Interface | Inputs / successful outputs | Constraints and failure representation |
|---|---|---|
| Registry operations | `ToolArguments` object; recursive `OperationSchema`; revision-bound `ToolValue` output | Typed `OperationError`: identity/duplicates/metadata, unknown operation, invalid input/output, stale source, document failure, unauthorized, provider failure. `OperationValidation` gives fixed reasons without unbounded reflected keys. Provider-local failure is bounded message text, not a domain error schema. |
| Language semantic operations | JSON arguments; `SemanticOperationResult` variants: read-only values, contract observations, semantic evaluation, changes, validated changes, blocked | Top-level argument kinds; opaque constraints; typed semantic errors for arguments, unsupported operation, source diagnostics, ambiguity, internal failure. `SemanticBlocker` adds category, recoverability, missing dependencies, diagnostics, next-action guidance. Ordinary values have no per-operation output schema. |
| Agent tools / semantic adapter | `ToolValue`: null, bool, i64, string, array, object; tool inputs describe top-level kinds | `ToolError` represents transport/dispatch failures. Semantic outcomes can instead be successful tool results with statuses such as blocked, rejected, unauthorized, revision conflict, or invalid arguments. `Ok(ToolResult)` is not semantic success. |
| Transformation execution | Nominally typed byte values and endpoint artifacts | `ExecutionError` distinguishes validation, binding, input, runtime, output failures. Fail-fast execution retains already completed artifacts; it is not transactional rollback. |

References: `tool.rs`, lines 212–297, 373–401; `tool/operations.rs`, lines 49–159; `scope-language/src/semantic.rs`, lines 172–353; `tool/semantic.rs`, lines 192–195, 751–775; `scope-execution/src/lib.rs`, lines 5–63.

**Verified numeric mismatch:** semantic JSON accepts numbers including nonintegral values and unsigned integers, while the agent protocol uses i64. The semantic adapter cannot carry arbitrary floats/large unsigned results as `ToolValue`. This is an actual transport restriction, not a missing filtering helper.

## 2. Existing reusable interfaces and composition feasibility

### 2.1 Programmatic composition is already possible

**Verified:** a Rust host can discover registry descriptors, invoke operation A, inspect its validated `OperationResult.value`, put a selected value into operation B's `ToolArguments`, and invoke B with its own policy and expected revision. B independently validates those arguments. No model, parser, graph executor, or new registry is required. Core semantic dispatch and `AgentSession::invoke_with_call_policy()` also support explicit sequential calls.

**Assessment:** this is runtime-checked structural composition, not whole-plan static typing. The registry has input/output contracts but no plan/wiring API or schema assignability API. The host must handle result projection, optional fields, error propagation, and source freshness. Merely having equal-looking schemas does not prove domain compatibility. Successful writes require a new accepted snapshot and invalidate old evidence; composing proposals does not implicitly apply earlier candidates.

The repository also already has **nominal typed graph composition**: `scope-types` validates named/versioned ports, missing/multiple inputs, type mismatch, cycles, and planning capabilities. `scope-execution` consumes `Bindings` of component bytes/export names, with Cell retaining runtime authority. Those bindings are not `OperationProvider`s, and `TypeId` plus opaque bytes is not the same contract as `OperationSchema` plus document revision. Reusing graph concepts does not make document operations executable through that path without an explicit, separately justified bridge.

### 2.2 Reuse rather than replace

- Use `OperationId` components; do not parse concatenated agent tool names into registry identities.
- Use registry schemas and `invoke()` for bounded, authorized document calls.
- Use `LanguageService` and validated semantic dispatch for downstream-owned language knowledge.
- Use `SourceTarget`, target resolution, and the existing semantic adapter for in-memory proposals.
- Use `SessionCatalog` for actual installed agent tools, and the existing focused edit discovery entry point rather than another catalog.
- Use `DocumentSession`/`Workbench` for explicit persisted replacement review and checked save.
- Use existing graph validation/component execution only when the task really concerns that component/port ABI.

### 2.3 What an external language would need for safe invocation

**Assessment — required integration responsibilities, not new abstractions:**

1. A host-provided view of installed operations and their actual descriptors; discovery is not authorization.
2. Explicit host target resolution, workspace grants, and retained expected-state tokens. An out-of-process language needs host-owned handles for opaque `Revision`s; serializing a number or reconstructing a path cannot recreate one.
3. Exact value conversion for the chosen API, bounded inbound decoding before allocation where needed, and no silent numeric conversion.
4. A host callback that authorizes each call using arguments, target snapshot, effect, and all permission declarations. Avoid permissive `AgentSession::invoke()` as the external boundary.
5. Dispatch through the validated API, not direct provider hooks; preserve the semantic adapter's source/scope checks or equivalent host checks when using core semantic dispatch.
6. Explicit handling of typed errors, blocker/result status, source invalidation, and verification limitations. Do not equate invocation success with passing evaluation.
7. Separate proposal review and host acceptance. Scripts/model output must not fabricate authorization receipts or choose the host edit decision.
8. A trusted-provider policy and execution budget appropriate to the host. Current native invocation is not isolation, cancellation, or rollback. More powerful guest execution remains subject to Cell authority.

For an in-process experiment, these can be ordinary host code around existing methods. Stable wire serialization, capability-handle lifetime, authentication, and protocol error contracts would need decisions for a remote language; the current APIs are not themselves a complete remote protocol.

## 3. Gaps blocking typed composition

| Finding | Classification | What it actually blocks |
|---|---|---|
| Semantic descriptions have only top-level input kinds and no operation-specific output schema | **Contract limitation** | Generic, pre-dispatch output-to-input checking across arbitrary language operations. Does not block host-authored calls or registry-only structural composition. |
| Registry structural schemas, semantic JSON contracts, and graph nominal byte ports have no automatic mapping | **Integration gap** | Uniform composition across the three surfaces. A bridge must define values, identity, revision provenance, and error semantics; renaming them all “tools” does not solve it. |
| No typed argument roles binding candidate source and retained criteria to evaluator inputs | **Semantic contract limitation** | Safe automatic evaluator reconstruction/refinement. Existing lifecycle explicitly records this missing interface rather than guessing argument names (`edit_lifecycle.rs`, lines 552–567). |
| Schemas cannot express unions, numeric ranges, enum membership, nominal types, or semantic preconditions | **Expressiveness limit** | Generic proof of richer compatibility. Not a blocker for a small string/object/integer fixture; opaque constraints must not be promoted into generic validators. |
| Registry has validation but no public compatibility/wiring helper | **Convenience gap** | Ergonomic generic wiring/preflight. Explicit host projection plus B's input validation works now. Equality alone would not handle all optional-field/closed-object cases. |
| Calls are revision-bound to one document/session; no multi-document transaction or automatic candidate snapshot dispatch | **Deliberate scope / genuine limit for broader plans** | Atomic workspace refactors and chaining against unapplied candidates through the session registry. Not a reason to broaden the first experiment. |
| Errors and evidence are not uniform payload schemas across entry points | **Contract/integration gap** | Generic recovery and semantic-success handling without surface-specific interpretation. Typed registry error categories already exist. |
| Native providers are synchronous trusted code; `Blocking` is advisory | **Execution boundary** | Enforced timeouts, in-flight cancellation, rollback, or isolation of untrusted providers. These are not implied by typed values. |
| Registry descriptors/opaque revision tokens are not a ready-made remote wire protocol | **Transport integration gap** | Out-of-process invocation without host-owned token mapping. Not an obstacle to an in-process proof. |

**Assessment:** do not require advanced type-system features before proving two runtime-validated operations can compose. The immediate uncertainty is interoperability and metadata sufficiency for real operations, not the absence of composition machinery everywhere.

## 4. Gaps blocking focused agent discovery

### 4.1 Focused discovery already exists

**Verified:** `SessionCatalog` contains definitions, provider labels, output descriptions, and structured metadata; exact lookup and export are available. Semantic metadata includes ID, argument descriptions, effect, target, purpose, evaluation support, and invocation schema. Opaque constraints are preserved separately as JSON text.

More importantly, `edit_lifecycle.rs` already implements focused semantic discovery:

- `SemanticDiscoveryOptions` specifies host-selected initial operation names and page size.
- `run_semantic_edit_with_discovery()` uses the existing decision protocol.
- `edit.discover_operations` reveals sorted pages of eligible operations; it accepts exactly `{}`, grants no authority, and consumes tool-call budget.
- Catalog context is phase-specific and compact. It includes exact invocation preconditions, single-document scope, and effect semantics, while whitelisting relevant metadata rather than blindly copying opaque constraints.
- Hidden tools cannot simply be invoked by guessed names; visibility and eligibility are checked.
- Continuation evidence is associated with exact calls/relevant host state and invalidated when that state changes.

References: `crates/scope-ide/src/edit_lifecycle.rs`, lines 431–509, 511–590, 727–855, 1234–1344.

**Assessment:** “add a focused tool-discovery framework” would duplicate existing work. Metadata already supports focused, reasonably accurate context for the existing semantic workflow. It does not establish uniformly typed results or an accurate cross-surface planner.

### 4.2 Remaining discovery gaps

| Finding | Classification / impact |
|---|---|
| Rich recursive registry descriptors are not automatically projected into `SessionCatalog`/focused semantic lifecycle | **Integration gap.** Registry discovery works directly; agent-facing reuse needs an intentional projection, not a second source of truth. |
| Semantic ordinary output shape and domain argument roles are unspecified | **Genuine metadata gap.** Agents can read prose, but generic consumers cannot safely infer result projection, candidate argument identity, or criteria preservation. |
| Focused discovery pages sorted eligible names, rather than semantic relevance-ranked search | **Convenience/retrieval gap.** Hosts can already choose an initial subset. Ranking is not authority and is unnecessary for the first experiment. |
| Registry descriptors have description/schema/effect/execution/permissions but no semantic purpose, selection requirement, or per-field prose | **Cross-surface context gap.** Do not infer semantic intent from `Read`/`Write` or operation names. |
| Catalog metadata is optional and dynamically represented | **Convenience/consistency gap.** Existing filtering works; a mandatory universal metadata format is not justified by S1. |
| Language metadata graphs are outline projections with snapshot-local symbol identities | **Coverage limit.** `LanguageMeta` is not a full callable signature/type/data-flow model (`language/meta.rs`, lines 1–115). Graph discovery does not fill missing operation output contracts. |
| `MetaDiscoveryProvider` inventories graph types/relations and source availability, not operation compatibility | **Different purpose, not a defective operation registry.** Keep source navigation separate from callable contract inference (`tool.rs`, `MetaDiscoveryProvider`; `tool/meta_surface.rs`). |
| Declaration is not evidence or permission | **Intentional boundary, not a gap to remove.** Purpose/evaluation support, catalog presence, and metadata references must not grant execution, source access, or edit acceptance. |

## 5. Minimal proposed next experiment

**Proposal only — not implemented by this milestone.**

### Question

Can a host discover two existing operations, expose only the relevant context, and explicitly pass A's validated output into B while preserving authorization, target/revision identity, and failure semantics?

### Scope

Use a test or example in-process Rust host, one host-opened document, two trusted read-only operations, and existing string/object/integer values. Inventory a concrete downstream pair first; if none fits the current contracts, record the exact missing contract rather than inventing a new framework. Prefer already implemented providers; any fixture-only provider should be explicitly identified as synthetic and cannot prove downstream interoperability.

### Procedure

1. Enumerate existing descriptors/catalog entries. Record the exact identity, input/output representation, effect, target, permission requirements, and execution declaration available on the chosen path.
2. Expose a host-selected initial subset; exercise existing semantic discovery paging when using the semantic path. Record which context is sufficient and which knowledge remains manually supplied by the host.
3. Invoke A through its existing validation/policy boundary at the accepted revision.
4. Explicitly project one returned field/value into B's declared argument. Do not infer candidate/criteria roles from names, invent automatic coercions, or add a plan interpreter.
5. Invoke B through its own existing boundary, with an independent call-aware policy decision and fresh revision check. Retain both calls/results as evidence.
6. Demonstrate wrong type, missing/unknown argument, denied second call, stale revision between calls, malformed provider output where an output schema exists, and semantic blocked/rejected outcomes where applicable. Assert rejected/denied calls do not dispatch and no source is edited.
7. Separately record whether the pair could have been wired using metadata alone. For semantic operations lacking an output schema, explicitly report that preflight typing remains unproven even if runtime calls work.

### Success and stop conditions

- **Success:** two explicit calls compose without new abstractions; each enforces its existing policy and freshness checks; focused context contains the actual installed operation contract; failures remain distinguishable; no editing authority is introduced.
- **Narrow follow-up only if demonstrated:** document the smallest missing contract on the existing interface. If the only issue is catalog projection or filtering, treat it as a convenience change. If safe wiring requires guessing opaque results or candidate argument roles, propose a downstream-informed contract change separately.
- **Stop expansion:** if the experiment requires an additional registry, silently bypasses policy, equates evaluation with authorization, or introduces new language semantics in Scope. Do not implement a parser, scripting executor, automatic evaluator orchestrator, Cell change, or multi-document transaction as part of this experiment.

The experiment is deliberately runtime-checked first. It tests the substrate before committing to a language syntax or a new static type system.

## 6. Risks of introducing a parallel abstraction

1. **Competing catalogs:** a new tool registry could drift from installed providers and duplicate IDs, schemas, purpose, or permission declarations.
2. **Authority laundering:** discovery, plan validation, nominal type compatibility, or evaluation evidence could accidentally become permission to invoke/apply/save. Existing code deliberately separates these decisions.
3. **Revision erasure:** flattening opaque session revisions, numeric buffer versions, and graph artifacts into one generic value could allow stale evidence or candidate output to be treated as current source.
4. **Weaker validation:** a generic wrapper could preserve only top-level tool kinds and discard recursive registry output validation, size budgets, or exact source preconditions.
5. **False safety claims:** native `Read` declarations do not sandbox providers; composition does not add rollback or interruptibility. Component execution retains Cell's authority boundary, not the registry's trust model.
6. **Language leakage:** interpreting opaque language constraints or inferring candidate/criteria arguments centrally would put downstream semantic knowledge into Scope.
7. **Lifecycle duplication:** another proposal/evaluation state machine could contradict `SourceEditController`, document review receipts, or workbench verification states.
8. **A fourth type model:** introducing scripting types before resolving existing structural schemas, JSON descriptions, and nominal ports could increase rather than reduce integration work.

## 7. Final decision

**Narrow scope and proceed only with the small existing-API experiment above.**

Scope already supports discoverable structured operations, mandatory registry authorization, validated inputs/outputs, immutable source context, focused semantic catalogs, programmatic sequential invocation, typed component graphs, and host-controlled proposal lifecycles. These are sufficient foundations to investigate composition without another framework.

What is not established is **uniform typed interoperability**: semantic output/nested-input contracts and argument roles are incomplete; the registry, agent, and graph surfaces do not automatically share contracts or execution; remote token handling is unspecified. These are specific limitations, not evidence that Scope needs a new scripting runtime now.

### Validation of this inventory

Source inspection covered the APIs referenced above and their nearby tests; test bodies were treated as examples/assertions, not observed passing results. No builds or runtime tests were run because this milestone changes documentation only. Documentation validation checks local linked source paths and confirms the change set is limited to this inventory. No production code or recommendations were implemented.
