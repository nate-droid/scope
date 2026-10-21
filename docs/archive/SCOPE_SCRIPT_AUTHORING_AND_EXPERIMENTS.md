# SS1 — Scope Script Authoring and Experimentation Audit

## Decision and scope

**Reuse the existing IDE shell, source buffers, semantic dispatch, and source-edit controllers. Make fresh Script execution pleasant before adding iteration syntax.** Script already supports immutable variables and structured data. The first missing language primitive is a genuine conditional, not variables or arrays. A small host-owned bounded experiment can establish isolation requirements before a loop is justified.

This is an audit and implementation proposal, not a language specification or an implemented feature. No production implementation changes accompany SS1. Paths below are repository-relative; implementation and checked-in tests are the evidence. Test coverage descriptions are not claims that those tests were run for this audit.

Three interfaces must not be conflated:

- `scope-terminal`: input editing, history, paste and completion, not document ownership or execution.
- `scope-script`: synchronous interpreter, standalone CLI/REPL and explicit host adapters.
- Scope IDE shell: documents, scratch buffers, multiline editing, language-session routing, proposals, and an optional host callback.

## 1. Current implementation inventory

### 1.1 Syntax, evaluation and inspection

Primary evidence: `crates/scope-script/src/parser.rs`, `src/lib.rs`, `src/main.rs`, `src/repl.rs`, and the crate README.

| Area | Implemented behavior |
| --- | --- |
| Statements | Expressions and immutable `let name = expression`, separated by newline or semicolon. Newlines within parentheses/collections are ignored. Whole source is parsed before evaluation. |
| Values | `Value` aliases `scope_ide::tool::ToolValue`: signed `i64`, boolean, string, null, array, object. No floating-point values or implicit coercions. |
| Collections | Array/object literals, nested values, identifier or quoted object keys, trailing commas. Duplicate object keys are syntax errors. |
| Inspection | `.field`, string-key object indexing, zero-based integer array indexing; missing fields, wrong kinds, negative/out-of-range indices fail. |
| Expressions | Checked integer arithmetic `+ - * / %`, unary minus, boolean `!`, structural value equality/inequality, integer/string ordering, parentheses, boolean-only short-circuit `&&` and `||`. String concatenation is not implemented. |
| Calls | Exactly `print(value)`, `scope.operations()`, `scope.invoke(provider, action, object)`. Fixed arities are parsed; invocation argument kinds are checked at runtime. |
| Bindings | Persist in one `Interpreter`; no reassignment. Existing/duplicate names are rejected before any statements execute. Reserved builtin/literal names cannot be bound. |
| Output | `eval` returns the last statement's value; empty source and `print` yield null. `print` retains structured values; `take_output` drains them. No output stream during evaluation and no public binding browser. |
| Not implemented | `if`/`else`, statement blocks, loops, user functions, modules, exceptions, mutable collection updates, general static typing, direct IO/process/agent primitives. Comments are not recognized by the lexer. |

Boolean short-circuiting can skip a host call **only as part of a boolean expression**. It is not a general conditional: `condition && scope.invoke(...)` fails if the invoked result is an object rather than a boolean. It does not provide branch-local bindings or report construction. Do not advertise this as workflow B support.

The parser uses a private token stream and precedence parser. `Interpreter::eval(&str, &mut impl Host)` parses, preflights duplicate bindings, then evaluates synchronously in order. Parsing is private; a future completeness adapter should share parser knowledge rather than implement a second grammar.

Runtime failure stops subsequent statements but preserves earlier bindings, output and host effects. There is no transaction or rollback. A fresh interpreter is therefore the appropriate default for rerunning a saved program; repeated evaluation in a persistent REPL encounters duplicate `let` names.

### 1.2 Invocation, documents and semantic operations

| Surface | Actual boundary |
| --- | --- |
| `Host` (`src/lib.rs:15–28`) | Discovery and invocation hooks. Hosts choose authority and underlying operation infrastructure. This trait is not a registry or permission grant. |
| `NoHost` (`src/host.rs`) | Standalone CLI has no installed privileged operations. Reading the explicitly supplied script file still uses the CLI's host-side filesystem access. |
| `RegistryHost` (`src/host.rs`) | Borrows `OperationRegistry` and persisted `DocumentSession`, captures opaque expected revision and explicit policy. Projects existing descriptors and delegates validation/invocation. Additionally restricts calls to read effects. Returns result value while retaining complete identity/revision-bearing results in `results`. |
| `SemanticHost` (`src/semantic.rs`) | Exposes one host-selected semantic read through `AgentSession`, with explicit authorization and host-captured source/version preconditions. `AgentSession` is a tool dispatcher, not a model call. |
| `SemanticToolProvider` (`crates/scope-ide/src/tool/semantic.rs`) | Binds service, target, selection, document scope, freshness and concrete-call policy; checks identity/text/version and provider arguments. Language-specific interpretation remains downstream. |
| Workspace examples | `workspace.rs`, `workspace_semantic.rs` and their support modules explicitly assemble capabilities/providers. They are not built-in semantic modes of the standalone CLI. |

There is no Script builtin `open`, `eval`, `agent`, `save`, `reset_to_original_revision`, or `report`. These conceptual verbs would require host wiring or downstream operations, not merely new spelling. `print` plus object construction already provides basic structured reporting.

`Document`, `ScratchBuffer`, and `DocumentSession` have different ownership contracts. The first two can contain unsaved text; `DocumentSession` manages explicitly authorized persisted source with opaque session-local revisions. Do not reopen the current unsaved IDE document through a persisted session as an implicit execution target.

### 1.3 Errors, attribution and budgets

`Error { offset, kind }` carries a UTF-8 byte offset in the submitted script. `ErrorKind` distinguishes syntax, runtime, resource limit, original `OperationError`, and original `ToolError`. Host-call failures are attributed to the call expression; this is not automatically a downstream document diagnostic range. The CLI adds a script pathname in file mode, but the interpreter has no source identity, historical snapshot resolver, line/column range, or nested source stack.

Semantic failure envelopes are ordinary values. Interpreter success, tool dispatch success, semantic success, adapter compatibility and authorization are different outcomes. A script that projects `result.value` without checking status can turn a legitimate rejection envelope into a later missing-field runtime error. Preserve both context and original envelope rather than reporting semantic success.

Implemented interpreter limits:

- Source at most 65,536 bytes; nesting/expression depth at most 64.
- Per evaluation: 100,000 expression steps and 1,000,000 accounted work units.
- Approximately 1,000,000 accounted retained value/output units across interpreter lifetime, counting nodes/string bytes rather than exact allocator memory.
- Registry host history is separately bounded (64 results and approximately 1 MB accounted values).

These are fixed implementation budgets, not a configurable experiment policy. They do not bound native provider wall-clock time, pre-return allocations, external effects or model cost. Retention failure after dispatch cannot undo dispatch.

Script has no execution cancellation hook. Terminal cancellation discards pending input, not an executing provider. The language-session API has optional `CancellationHandle` support and reset-required outcomes (`crates/scope-language/src/interactive.rs`, `crates/scope-ide/src/language/interactive.rs`), but Script has no adapter implementing it. The synchronous shell does not concurrently poll its input while evaluating. Advertise cooperative cancellation only when the interpreter, host and downstream runtime all define it; a hard timeout requires a stronger execution boundary.

### 1.4 Existing evidence and aspirations

Useful checked-in coverage:

- `src/lib.rs` unit tests: values, projection, arithmetic, short-circuiting, persistent bindings, parse-before-effects, resource limits.
- `tests/cli.rs`, `terminal_pty.rs`: file/stream/interactive behavior.
- `tests/registry_host.rs`: nested schemas, policy, revision, results and read gate.
- `tests/semantic_inspection.rs`, `workspace_semantic.rs`, `portability.rs`: explicit provider wiring, downstream semantics and discovery portability.
- `tests/failure_envelopes.rs`, `execution_transcript.rs`: structured failures and output retention.
- `tests/contract_first_interoperability.rs`: host-derived contract inspection, not universal composition.
- Examples `structured-report.scope`, `semantic-explorer.scope`, `workspace_semantic.scope`: real orchestration with existing values, not loops or agent automation.

Earlier design records are not necessarily current implementation. In particular, `docs/archive/INTERACTIVE_SCRIPT_WORKBENCH_DISCOVERY.md` describes a then-missing host callback. **That seam now exists** as `cli::ReadOnlyCommand` and `run_with_session_and_command` (`cli.rs:333–365`), invoked by `:scope host NAME --selection LINE:BYTE..LINE:BYTE`. It borrows the App document, not the selected scratch buffer. This supersedes the older claim that no callback exists; it does not establish a generic Script buffer runner or a Script language-session adapter.

## 2. Concrete authoring workflow and pain points

### 2.1 What a user can do today

**Saved pure Script:** create a UTF-8 `.scope` file using an existing editor, then run:

```sh
cargo run -p scope-script --offline -- crates/scope-script/examples/pure.scope
```

Whole-file multiline evaluation works, with a fresh interpreter and `NoHost`. Explicit prints are shown; file mode does not automatically print the final value. Edit the file and rerun the same command. There is no built-in watch mode or IDE source attachment.

**Standalone exploration:** `cargo run -p scope-script --offline` starts terminal editing when stdin/stdout are terminals; otherwise the stream REPL handles one LF-framed submission at a time. Bindings persist. Use history/completion for repeated expressions and `print` or a final expression for inspection. `:quit`/EOF exits. This is not a multiline Script editor: the terminal REPL uses default single-line paste, and the stream runner does not accumulate incomplete submissions.

**Workspace semantics:** use the explicitly authorized example runners documented in `crates/scope-script/README.md`. They install real workspace-backed hosts; the plain executable does not acquire these operations merely because a file contains `scope.invoke`.

**Existing IDE authoring machinery:** for a registered interactive language, the shell supports:

```text
:scope session <registered-language-id>
:scope scratch new <matching-language-id>
:scope edit
<literal source lines>
:done
:scope show
:scope buffer eval
```

For an existing file use `:scope open <path>`, edit, evaluate, and explicitly save. The cooked editor supports `:at`, `:insert`, `:delete`, bounded `:show`, `:done` and `:cancel`. It edits a draft and commits once. Buffer evaluation uses unsaved text and does not save or select/reset a language runtime. **These commands cannot currently evaluate Scope Script merely by opening a `.scope` file: no Script `InteractiveLanguageSession` implementation exists.** Opening a file does not select its language runtime.

The IDE's multiline input layer (`shell_input.rs`) supports completeness-aware Enter, Ctrl+J newline, line navigation and history. Public `scope-terminal` instead has an escaped single-line viewport; its `NormalizedLayout` paste policy preserves embedded layout but is not a full multiline editor. Neither should be replaced for SS2.

### 2.2 Actual friction

1. Existing multiline/source-buffer ergonomics and Script execution are disconnected.
2. No explicit fresh-run action for the active Script buffer; persistent REPL rebinding makes repeated full programs awkward.
3. Source script and semantic target are separate concerns. The active runtime receives submitted text, not App access; installing Script as active language also replaces the downstream interactive runtime.
4. Current host callback borrows the App document only. It does not receive selected scratch Script source, and cannot safely pretend App and scratch owners are interchangeable.
5. Errors need source-aware rendering. A byte offset alone does not identify which edited historical script or target produced an error.
6. Printed values, final value and semantic rejection status need consistent presentation, especially after partial failure.
7. Scratch Save As/binding currently expects an existing destination; creating a new source file entirely inside this workflow has a distinct persistence gap. Add explicit non-overwriting creation semantics only if needed, through workspace capability APIs.
8. Separate scratch buffers evaluated through one language controller share a runtime. Different source IDs do not provide experiment isolation.

## 3. Supported-versus-missing workflow matrix

| Workflow | Supported now | Missing / constraint | Owner and smallest useful addition |
| --- | --- | --- | --- |
| A: open → evaluate → capture → report | IDE opens/owns source; host binds semantic read; Script invokes, binds structured result and builds/prints report. Existing workspace example demonstrates the core. | No universal evaluate action or automatic live-target attachment; no active Script-buffer run command. | Shell resolves script and target independently; downstream installs operation/provider; command-local fresh interpreter and `SemanticHost`. No new language primitive. |
| B: inspect → conditionally invoke | Bindings, projections, equality and boolean operators; host authorization already independent of conditions. | No general conditional. Missing-field access is fatal; arbitrary read envelopes cannot be boolean operands. Current semantic adapter exposes just one selected read. | Script owns a minimal boolean-only `if`/`else`; host owns explicit operation allowlist and context. Start with statement branches, lexical scope, no mutation/functions. Define semantics before implementation. |
| C: predefined bounded independent variations | Host can capture source, create new document/source owners and sessions, use proposals and record structured arrays/objects. Script can manually enumerate a small fixed sequence of calls. | No experiment runner, automatic baseline restoration, fresh-per-trial runtime guarantee, collection loop or accumulator. Current Script adapters are read-only; agent/edit authority is not available through them. | Host/downstream owns a small bounded trial controller using existing source/session/edit APIs. Script initially invokes/inspects its declared contract only if explicitly exposed. Do not begin with loops. |

## 4. Minimal proposed improvements and ownership

### 4.1 Fresh buffer run before a new execution lifecycle

Prefer an explicit namespaced shell action that captures the selected Script buffer text/source/version, resolves a distinct semantic target, and calls a downstream-installed runner. The callback owns a **fresh interpreter per run**, fresh target preconditions and per-call policies. The active downstream runtime remains unchanged.

Reuse the existing `ReadOnlyCommand` pattern, but do not claim its present signature already accepts arbitrary Script buffers. The smallest required extension is a Script-independent borrowed source/request seam; exact API and lifetimes need review. `scope-script` already depends on `scope-ide`, so adding the reverse dependency would create a cycle. Assembly belongs downstream or in example integration depending on both.

An `InteractiveLanguageSession` adapter is useful later for a persistent Script REPL, completion and inspection. It is not sufficient by itself for live App context and is not the default solution for running orchestration while Path remains the active runtime. Do not create a nested REPL or parallel buffer/session store.

### 4.2 Minimal conditional and reporting

Propose only boolean `if`/`else` with lazily executed branches. Require boolean conditions, fresh branch-local immutable bindings, full-program parsing and duplicate checks respecting lexical scope, existing depth/work charging, and call-site attribution. No implicit truthiness, unbounded loops, mutable variables or user functions. Exact syntax and branch result behavior are SS3 decisions, not SS1 commitments.

Keep reports as ordinary structured values. A host formatter should display numbered print values, final value **or** typed evaluator error, and semantic statuses without flattening payloads. Always drain prints after failure. Start with status checks against declared envelopes; do not invent a universal success-field convention. Add optional-field/variant inspection only if a real contract demonstrates a need; never make a missing field silently null.

### 4.3 Explicit bounded experiments

Represent predefined trials as host-validated data, not executable reset commands. Validate the count before dispatch; apply a hard maximum, per-trial operation/model-call budgets, and an explicit stop/continue policy. Record every attempted trial and any unattempted/skipped remainder with reasons. Interpreter work limits alone are insufficient.

Capture baseline **text plus identity/version and relevant runtime configuration**, not a pathname or integer version alone. Create a fresh owned source and downstream session for each trial; evaluate baseline setup according to that kernel's documented reset semantics. Do not mutate the original and then promise rollback. Document clone is not runtime isolation. A new `DocumentSession` only provides persisted-source freshness; it does not clone unsaved state or kernel state.

Existing source proposals remain the route for generated edits. For in-memory targets reuse `SourceTarget`/`DocumentSource`, `SourceSnapshot`, `resolve_edit_target`, and source proposal controllers. For explicitly persisted trials reuse `DocumentSession`'s propose → review → authorize → apply flow, acknowledging that apply writes disk. A temporary workspace is only appropriate with explicit confined capability grants and cleanup ownership; do not silently save the user's source.

### 4.4 Ownership decisions

| Concern | Owner |
| --- | --- |
| Keystrokes, history, paste, viewport | Terminal / existing IDE input layer |
| Buffers, create/open/edit/save, target selection, source snapshots | IDE document/scratch/workspace mechanisms |
| Script parsing, expressions, branch evaluation, interpreter budgets | Scope Script |
| Semantic operation identity, payload contracts, analysis meaning | Scope registry / downstream language integration |
| Compatibility declarations and adapter evidence | Scope-owned contract facilities / integration owners, not Script inference |
| Concrete authorization and proposal acceptance | Existing host policies and edit controllers |
| Runtime capability enforcement | Cell and capability-backed host integrations |
| Baseline capture, fresh trial owners, lifecycle, resource accounting | Downstream experiment controller using existing Scope APIs |
| Report composition | Script values; rendering/provenance retention in host |

A script invocation cannot grant agent, write, network, process or save authority. A compatible signature cannot authorize a call. A read classification cannot prove native purity. New experiment-facing operations must declare their actual effects; do not disguise agent/edit execution as a read to pass existing adapters.

## 5. Type Preservation and Structural Composition Readiness

### 5.1 Current type boundaries

There is no single unified type representation to extend casually:

| Boundary | Existing information | Limits |
| --- | --- | --- |
| Script values/builtins | Runtime `ToolValue` kinds, checked operators, fixed builtin arities | Bindings carry no nominal identity, schema, contract version or provenance. No user function signatures. |
| `OperationRegistry` | `OperationDescriptor` input/output recursive `OperationSchema`, effect, execution and authorization declarations | Transport contracts, not semantic equivalence, refinements or native type identity. Recursive means finite nested array/object schemas, not named recursive definitions/unions. |
| Language semantic descriptions | Operation identity, shallow argument kinds, optional opaque JSON constraints and provider-defined outcomes | No general recursive operation-specific output contract; nested input and semantic obligations remain provider-owned. |
| Nominal composition | `scope-types::TypeId { name, version }`, ports and transformations, exact connection checking | No public general nominal-to-structural association; equal shapes do not weaken exact identities. |
| Component execution | Nominally tagged opaque bytes, explicit executable bindings and envelope checks | Codec/adapter meaning is integration-owned; Cell capability enforcement remains separate. |

Evidence: `crates/scope-ide/src/tool/operations.rs`, `crates/scope-language/src/semantic.rs`, `crates/scope-types/src/lib.rs`, `crates/scope-execution/src/lib.rs`, and Script host adapters.

Registry discovery preserves nested schemas and delegates input/output validation to the original registry, including unknown/missing field rejection. Script does not need another schema validator. `SemanticHost` preserves the tool envelope, but cannot manufacture deeper contracts the semantic provider never published. Arbitrary custom `Host` implementations can expose less information; the trait itself makes no strong contract-preservation guarantee.

### 5.2 Concrete information-loss risks

- `RegistryHost` projects `OperationResult.value`; document identity/revision survives in host `results`, not in an exported plain Script value. Reports need host-retained provenance correspondence.
- Semantic JSON conversion supports only signed `i64` numbers. Fractional or oversized unsigned values fail conversion rather than round/truncate. `constraints_json` preserves opaque constraints as serialized text, not executable compatibility knowledge (`tool/semantic.rs`). Never infer an opaque constraint is satisfied.
- Field projection/reconstructed reports may discard status, partiality, unsupported features, rationale or source obligations. A selected payload is not durable proof of semantic success.
- Plain integer/object equality cannot distinguish domain units, nominal versions or adapters. Missing type information is **unknown**, not structurally compatible.
- Interpreter offset alone does not bind a value to the exact script snapshot, target snapshot, operation declaration or adapter version.

These are existing boundary limitations, not evidence that a fully typed contract is silently erased everywhere. Preserve what is available and explicitly label what is absent.

### 5.3 Minimal readiness improvements

1. Continue aliasing the existing transport value and consuming authoritative registry descriptors; do not add a Script-specific schema/type algebra.
2. Keep operation identity and provider/action separate, and retain upstream contract identity/version when upstream publishes it. Do not synthesize identities from display names or field sets.
3. Retain source/operation/provenance alongside run results in the host. New branches and collections must preserve ordinary envelope values without coercing away unknown/unsupported variants.
4. If a real producer/consumer pair needs deeper checking, first publish the missing owner-authored contract upstream, reusing `OperationSchema` where sufficient. Script may inspect the resulting host report; compatibility logic stays upstream.
5. Keep comparison, adapter correctness evidence, explicit adapter selection, binding, authorization and execution as separate decisions. No automatic conversion based only on matching fields.
6. Require explicit rejection or adapter handling for unsupported numeric domains; defer a transport expansion until a concrete downstream requirement establishes it.

Current dynamic bindings and simple control flow do not inherently prevent later polymorphism or interface composition. The risk is cementing unversioned payload assumptions or duplicating compatibility logic in Script. Typed variants, nominal/structural associations and integration-provided adapters would need upstream declarations before Script can consume them reliably. Traits, generics, higher-kinded types and universal structural matching are speculative long-term directions, not SS2–SS4 requirements.

### 5.4 Generic cross-language semantic workflow

**Implemented substrate:** a host installs declaration-language or state-machine-language services, binds the chosen source/selection/policy, discovers a semantic read, invokes it through the existing tool path, and records the complete outcome. A Script can use provider/action discovery and print the envelope without understanding language-specific fields. `tests/portability.rs` and `docs/CROSS_PROVIDER_DISCOVERY.md` demonstrate shared orchestration with distinct semantics. Current `SemanticHost` still exposes just one selected read per instance, not arbitrary multi-provider composition.

**Proposed generic producer → consumer experiment:** describe a selected construct in language A, then feed a summary to a review operation in language B. Registry-backed endpoints can publish recursive input/output shapes now. Their host must explicitly identify payload versus envelope boundaries, source obligations and any adapter assembling the consumer request. Language semantic descriptions alone cannot currently prove this pairing safe: they lack the required deep output contract and nominal association. Until integrations publish verified contracts and adapter evidence, report compatibility as unknown and do not auto-wire.

`tests/contract_first_interoperability.rs` demonstrates a restricted host comparison of provider-owned contracts with boundary attestations; Script inspects its report and invocation remains denied. `crates/scope-ide/tests/structural_interoperability.rs` uses private experiment machinery. Neither is a public assignability service or proof of semantic equivalence. See `docs/archive/STRUCTURAL_INTEROPERABILITY_SUMMARY.md` and `STRUCTURAL_INTEROPERABILITY_RESULTS.md`. Portable contract exchange, verified general adapters, interface composition and automatic polymorphic routing remain future upstream work.

## 6. Recommended milestones SS2–SS4

### SS2 — Pleasant authoring and fresh read-only runs

**Scope:** connect existing source buffers to an explicitly installed downstream Script runner; preserve the active language runtime. Establish separate script source and semantic target. Reuse the existing cooked editor first. Add source-aware output/error presentation and fresh interpreter semantics. Defer a persistent Script session adapter unless a demonstrated use requires it.

- **Reuse:** `ScratchBuffers`, buffer draft editor, `Document` source IDs/versions, `ReadOnlyCommand` configuration pattern, `DocumentSource`, `SemanticToolProvider`, `SemanticHost`, `Interpreter::{eval,take_output}`; existing terminal/history/completion APIs.
- **Acceptance:** author/edit a multiline Script in the shell; run unsaved text twice without rebinding failure; inspect all prints plus final outcome; edit and rerun; no implicit save, target switch, runtime reset or authority acquisition. Errors resolve to the exact captured Script source. Denied/stale semantic calls remain distinguishable.
- **Tests:** embedded stream-driven shell tests; print-before-error; unchanged original document/disk/proposals; mismatched language/target and expired version; default-deny host; EOF/cancel draft behavior; Unicode byte offsets; repeated fresh runs. Add PTY coverage only for changed terminal behavior.
- **Blockers:** borrowed-source callback signature/lifetimes and dependency direction; App versus scratch target distinction; source snapshot retention policy. New-file persistence is a separate capability-backed addition, not a reason for another editor.

### SS3 — Conditional structured orchestration

**Scope:** add only the minimal conditional once SS2 can exercise real rejection/success envelopes; improve discovery/report presentation without inventing a new type system.

- **Reuse:** current parser/evaluator, `ToolValue`, upstream descriptors/validation, typed error kinds, SS2 run transcript and source mapping.
- **Acceptance:** evaluate a document, inspect a declared status/boolean field, execute exactly the selected branch, and report the complete outcome. Denied operations remain denied even in a valid branch; unknown contracts never become compatible by default.
- **Tests:** lazy branches with zero calls in the unselected branch; nonboolean condition error; lexical binding rules; syntax failure before effects in either branch; nested depth/work limits; partial output preservation; failure envelopes without `value`; unchanged registry recursive validation and nominal mismatch tests.
- **Blockers:** decide branch scope/result semantics; downstream status conventions and incomplete result contracts. Do not bundle loops, mutable accumulators or general pattern matching.

### SS4 — Bounded isolated Path experiment

**Scope:** one downstream-owned trial runner over a short predefined strategy list. Start with two trials, not a general experiment framework. Script assembles/inspects structured results; host owns fresh source/runtime state and explicit effects.

- **Reuse:** `SourceSnapshot`, fresh source owners, `InteractiveSession` lifecycle/reset-required handling, source proposal controllers, explicit operation policy, Path kernel/integration APIs if available. Persisted trial artifacts require separate workspace grants and reviewed writes.
- **Acceptance:** both trials start from identical captured baseline bytes and declared configuration; distinct source/runtime identities prevent cross-trial leakage; explicit maximum trials and agent-call budget; each attempt records evaluation, proposal/review disposition, errors, provenance and bounded resource outcomes; original source and active runtime remain unchanged. User rejection is an observable outcome, not silently retried.
- **Tests:** instrumented deterministic provider/session fixture first; baseline equality and identity separation; no leaked definitions; denied edits/model calls; stale baseline; partial failure and stop/continue policy; cleanup failure; cancellation/reset-required handling where supported; maximum checked before any provider call; report preserves unknown/unsupported values and contract metadata. Optional live Path smoke test remains separate from CI/model-free tests.
- **Blockers:** Path may not yet expose evaluation, agent strategy, proposal application or fresh-session contracts through this integration; verify downstream. Hard model deadlines/cancellation and kernel isolation are not supplied by Script. A general bounded loop/collector is deferred until this runner demonstrates that manual enumeration or one aggregate operation is insufficient.

## 7. End-to-end Path experimentation example

**Proposed downstream scenario, not runnable syntax or a claim about Path APIs in this checkout:** compare two explicitly configured attempts to complete `learning/reflexivity.path`.

1. Open the file through Scope's existing document workflow. Capture its current unsaved text, source identity/version and Path runtime configuration. Do not silently reload disk.
2. Author a `.scope` report program in a scratch buffer using SS2. Select the Path document separately as target. Discover only host-installed operations; select by explicit identity/declared contract, not arbitrary catalog index.
3. Run baseline evaluation read-only. Record the complete kernel outcome: proof status, diagnostics, partial/unsupported information and source correspondence. If the integration lacks this operation, stop with unsupported rather than infer success from a display string.
4. Configure exactly two host-owned strategies, for example “direct reflexivity” and “inspect goal, then propose a proof”, with maximum trials = 2 and at most one authorized agent request per trial. These are strategy labels, not assumed Path tactics or model prompts with proven semantics.
5. For each trial the host creates a fresh source owner from baseline bytes and a fresh Path session. It evaluates setup as required by Path, requests an agent proposal only with explicit permission, and routes edits through existing proposal/review/authorization controllers. No Script read adapter is widened to smuggle these effects through.
6. Apply an approved proposal only to that trial's in-memory target (or explicitly authorized isolated workspace), then evaluate it independently. A rejected proposal, stale source, unavailable agent, invalid proof or reset failure is recorded. Nothing writes the original file.
7. Return structured trial records. The Script builds a report using existing arrays/objects/print; SS3 can select a summary branch based on a declared result field. A successful proof result is a Path-owned semantic claim, not merely successful interpreter execution.
8. Close trial sessions and release capabilities. Display baseline and both trial outcomes. Promoting a chosen proposal back to the original is a **separate** review, freshness check and authorization, outside the experiment run.

Illustrative **report data**, not a newly standardized schema:

```text
{
  experiment: "reflexivity comparison",
  maximum_trials: 2,
  baseline: {source: "host-retained snapshot reference", evaluation: "full Path outcome"},
  trials: [
    {strategy: "direct reflexivity", disposition: "review rejected", evaluation: null},
    {strategy: "inspect goal, then propose a proof", disposition: "approved in isolated trial", evaluation: "full Path outcome"}
  ]
}
```

Real records should keep structured envelopes rather than the explanatory strings above and include host-retained source/contract/proposal correspondence. Rejection is not proof failure; interpreter completion is not proof success. The useful initial result is a reproducible comparison with honest limits—not a new general-purpose language.

## Audit validation

This deliverable is documentation-only. Implementation, example and test sources were inspected; no production code was modified. Validation for this audit checks the new document's local file references and diff whitespace. Existing runtime test suites were not run because no runtime behavior changed; the milestone test plans above describe future required coverage.
