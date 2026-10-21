# SI1 — Structural Interoperability Discovery

## 1. Executive summary

**Recommendation: narrow the experiment and proceed with SI2 as an inspection-only, test-local probe.** Scope partially supports the goal already; it does not currently provide structural interoperability between arbitrary downstream language types.

Two implemented foundations are particularly reusable:

- `scope-types` has exact nominal identities, named input/output ports, graph diagnostics, and discovery of explicitly registered transformation chains.
- `scope-ide::tool::operations::OperationSchema` has recursive, closed value contracts for inputs and outputs, with bounded runtime validation.

These are separate models. There is no implemented association between a nominal type and its structural representation, no schema-to-schema assignability API, and no general semantic equivalence or conversion proof. Semantic language operations use a third, shallower description model and have no operation-specific output schema. Successful structured value transport is not static type compatibility.

The smallest credible next experiment should reuse `TypeId`, `Transformation`, `Registry`, and `OperationSchema`, adding only test-local associations and conservative compatibility outcomes. Do not change existing graph matching or introduce a public type language before that experiment demonstrates a need.

### Evidence labels and scope

- **Inspected implementation:** claims below grounded in the cited Rust source in this checkout.
- **Validated:** only the existing checks explicitly listed in section 11 were run.
- **Proposal/hypothesis:** SI2 interfaces, compatibility rules, and possible future ownership are design suggestions, not existing features.
- Cell's own implementation was not audited here. Its boundary is evidenced by Scope's executor and documented contracts; no broader sandbox/security verification is claimed.

Paths below are repository-relative. Source line ranges identify the inspected interfaces, not stable API versions.

## 2. Existing type-system inventory

### 2.1 Nominal composition: `scope-types`

`TypeId { name, version }` is an exact nominal identity, not a Rust `TypeId`, native type definition, or structural description (`crates/scope-types/src/lib.rs:11–33`). Versions are opaque strings. Names are not authenticated ownership claims; qualification is a naming convention, not collision prevention enforced across projects.

`Port { name, ty }` and `Transformation { id, inputs, outputs, requires }` describe operation-like nodes (lines 35–61). The same `TypeId` structure identifies both transformations and port value types. There are **no built-in primitives, record/tuple/array definitions, representation widths, units, ranges, aliases, or nominal-to-representation registry** here. The registry registers transformations, not type definitions.

`Graph` supports named external inputs, multi-input/multi-output nodes and explicit connections. Validation checks exact `TypeId` equality, required connections, endpoint existence, duplicate ports/sources, cycles, and missing host-supplied planning capabilities (lines 187–335). An invalid wire does not satisfy an input. This is exact declared compatibility, not semantic verification of the declaration or its bytes.

`Registry::discover` performs breadth-first search through registered single-input transformations, considers each output, and can filter declared requirements. Identical source/target IDs yield an empty identity plan (lines 391–443). It does not infer conversions from layouts, synthesize adapters, validate conversion mathematics, or explain why an absent chain could not be found. `None` can mean no registered reachable route, excluded multi-input routes, or requirement filtering—not proof of semantic incompatibility.

### 2.2 Recursive structural contracts: operation registry

`OperationSchema` is a real recursive value-schema algebra (`crates/scope-ide/src/tool/operations.rs:16–28`):

- Primitives: null, boolean, integer, string.
- Composites: homogeneous arrays; closed objects with named schemas and a required-field set.
- Integer values are `ToolValue::Integer(i64)`, not arbitrary precision, floats, or every source-language integer.

`OperationDescriptor` contains input and output schemas, effect, execution classification, and authorization names (lines 30–47). Registration validates descriptors; input roots must be objects. Invocation validates input before provider dispatch and output afterward, requires a policy callback, and binds the result to a document snapshot/revision (lines 196–335). Schema/value depth, node and byte budgets are implemented; depth caps at 128 (lines 91–113, 381–470).

There are no nominal value wrappers, schema references, unions, enum membership, ranges, precision/encoding constraints, or semantic refinements. `PartialEq` supports exact schema equality; it is **not an assignability implementation**. Closed-object extra fields and optional fields make generic directional compatibility subtler than comparing overlapping fields. These contracts describe transport values, not arbitrary native object layouts.

### 2.3 Language semantic descriptions and agent tools

`SemanticOperationDescription` has arguments, effect, target and opaque JSON constraints, but no per-operation output schema (`crates/scope-language/src/semantic.rs:7–74`). Argument kinds include string, number, integer, boolean, null, array and object. Array items and object fields are not recursively described. Validation checks required/unknown argument names and top-level categories (lines 118–150). `Number` accepts JSON numbers; `Integer` accepts signed or unsigned JSON integers.

Effects are `Read` and `ProposeChanges`; targets are document/selection. Purpose (`Exploration` or `CandidateEvaluation`) is a separate reserved metadata convention, defaulting to exploration (lines 76–116). Arbitrary constraints remain integration-owned JSON, not an implemented shared constraint solver.

`SemanticOperationResult` describes outcome envelopes—read-only JSON, evaluation, edits, validated edits, blocked—not the native type of the returned language object (lines 298–329). Core semantic dispatch validates advertised arguments/selection and rejects change-producing results advertised as read-only (lines 356–384). Evaluation/validation observations are language-provided evidence, not a general type conversion proof.

Agent `ToolDefinition` input kinds are flat string/integer/boolean/object/array categories. `ToolValue` is recursive null/bool/i64/string/array/object (`crates/scope-ide/src/tool.rs:91–118, 212–298`). Tool definitions have no recursive output contract; catalog output descriptions are prose. `AgentSession` applies policy and dispatches; providers own argument validation rather than a universal validator for flat tool definitions (lines 543–577).

The semantic tool adapter maps effects and generates JSON-Schema-shaped invocation metadata, but does not convert to `OperationSchema` (`crates/scope-ide/src/tool/semantic.rs:19–48, 94–138`). JSON-to-tool numbers must fit `i64`; unsupported numbers fail instead of silently rounding (lines 751–775). Thus a language `Number` or large unsigned `Integer` advertisement can exceed the tool transport's domain.

### 2.4 Metadata graphs and native language distinctions

`scope-meta` re-exports the same `TypeId`, but uses it for node kinds and relation kinds, not structural value descriptions (`crates/scope-meta/src/lib.rs:26, 77–139`). Its `TypeRegistry` declares node kinds and exact allowed directed relation endpoint pairs (lines 148–246). `MetaGraph::new` validates nodes, edges, source spans and provenance (lines 302–370). This is typed graph structure, not record decomposition, native subtyping or executable port wiring.

`LanguageService::analyze` returns `LanguageAnalysis { diagnostics, symbols }`, not inferred types (`crates/scope-language/src/lib.rs:180–206`). `SymbolDescription.signature: Option<String>` is human-readable interactive inspection, not a machine-readable function type (`crates/scope-language/src/interactive.rs:108–117`). Languages retain syntax, native typing, semantic interpretation and evaluation. A symbol kind such as function is not its parameter/result type.

`LanguageMeta` projects outlines into `scope.language.<kind>@1` nodes and `declares` edges with source references (`crates/scope-ide/src/language/meta.rs:20–115, 133–135`). Neither that projection nor similarly named metadata establishes inferred language types.

### 2.5 Serialization and runtime representation

- `TypeId`, endpoints, sources and connections have serde representations. `Manifest` version 1 stores nominal references and graph wiring, **not structural definitions, full transformations, implementations, grants or requirements** (`crates/scope-types/src/manifest.rs:5–16, 88–175`). Local registry declarations and host grants resolve it. `Resolution` diagnostics distinguish missing declarations, graph errors and optional implementation-presence checks; binding presence does not certify ABI or authority (lines 179–235).
- `scope-meta` serializes graph/registry descriptions and validates registry reconstruction (`crates/scope-meta/src/lib.rs:248–270, 302–370`). This does not add value schemas.
- Semantic descriptions use serde/JSON. Catalogs expose tool definitions and metadata as `ToolValue`; constraints are preserved as `constraints_json` text to avoid numeric loss (`crates/scope-ide/src/tool/semantic.rs:161–195`). That field is no longer directly traversable as structured tool data.
- `OperationSchema`/`OperationDescriptor` are inspectable Rust structures; their derives do not establish a portable serde schema protocol.
- `scope_execution::Value { ty, bytes }` pairs a nominal tag with opaque bytes (`crates/scope-execution/src/lib.rs:5–29`). The executor checks graph/input identities and the component port-envelope ABI (lines 60–109 and subsequent binding/output checks). Byte-envelope ABI checking is not validation that bytes encode the declared domain value. WIT component types are runtime ABI concerns, not automatically imported semantic type definitions.

## 3. Source-level evidence and ownership map

All named contracts in this table are public Rust items; helper validators and adapters may be private. Public Rust visibility does not imply a uniform remote protocol. Tool modules require the `scope-ide` `tools` feature (`crates/scope-ide/Cargo.toml:8–12`). Consumers listed are concrete in-repository consumers, not claims about external adoption.

| Interface / source | Owner and public surface | Current consumers / role |
|---|---|---|
| `TypeId`, `Port`, `Transformation`; `scope-types/src/lib.rs:11–61` | `scope-types`; public structs/fields | Composition graphs/registry; `scope-meta` re-export; `scope-execution` bindings/value tags; composition examples/tests |
| `Graph`, `Diagnostic`, `Registry`, `Pipeline`; same file:94–140, 187–443 | `scope-types`; public planning/inspection APIs; registry storage private | `scope-types/tests/composition.rs`, `examples/composition.rs`; executor; manifest resolution |
| `Manifest`, `CompositionDiagnostic`, `Resolution`; `scope-types/src/manifest.rs:10–16, 51–67, 179–235` | `scope-types`; public import/export/resolution APIs; resolution graph storage private | `scope-types/tests/manifest.rs`; `scope-execution/tests/execution.rs` portable composition test |
| `MetaNode`, `MetaEdge`, `RelationType`, `TypeRegistry`, `MetaGraph`, `MetaProvider`; `scope-meta/src/lib.rs` | `scope-meta`; public metadata contracts | `scope-ide/src/language/meta.rs`, `tool.rs:690–768`; `scope-ide/examples/meta_providers.rs`; meta graph/provider tests |
| `LanguageService`, `LanguageAnalysis`; `scope-language/src/lib.rs:180–206` | `scope-language`; public downstream extension seam; IDE reuses it | IDE language services/outline; declaration-description example; semantic adapter |
| `SymbolDescription`; `scope-language/src/interactive.rs:108–117` | `scope-language`; public interactive result | Interactive sessions and Scope Script inspection; signature is optional text |
| Semantic descriptions/arguments/results/dispatch; `scope-language/src/semantic.rs:7–150, 298–384` | `scope-language`; public re-exports from `lib.rs:46–53` | `SemanticToolProvider`; declaration fixture; `scope-ide/tests/operation_interoperability.rs` |
| `ToolProvider`, `ToolDefinition`, `SessionCatalog`, `ToolValue`; `scope-ide/src/tool.rs:24–55, 91–298` | `scope-ide`; public feature-gated tool API | `AgentSession`, semantic/meta providers, agent integrations, `scope-script::SemanticHost` |
| `SemanticToolProvider`; `scope-ide/src/tool/semantic.rs:94–195, 257–313` | `scope-ide`; public adapter; JSON conversion helpers private | Agent catalogs; semantic inspection/workspace examples and tests |
| `OperationSchema`, `OperationDescriptor`, `OperationProvider`, `OperationRegistry`; `scope-ide/src/tool/operations.rs:16–89, 178–335` | `scope-ide`; public feature-gated registry API | Module unit tests; `scope-ide/src/tool/editing.rs:71` proposal adapter; no automatic semantic-tool registry bridge |
| `Value`, `Binding`, `Bindings`, `execute`; `scope-execution/src/lib.rs:5–70` | `scope-execution`; public runtime adapter | Text pipeline example and execution tests; Cell supplies runtime enforcement |

Relevant documentation: `crates/scope-types/README.md` describes implemented exact nominal composition and future adapter responsibilities; its “registering future language or Cell components” section is guidance, not evidence of automatic integration. `crates/scope-meta/README.md` explicitly separates metadata relationships from execution. `docs/design/OPERATION_COMPOSITION_INVENTORY.md:151–165` records the same schema/nominal/semantic split. `docs/design/OPERATION_INTEROPERABILITY_RESULTS.md:64–79` explicitly excludes automatic typed composition from its two-call fixture. `docs/SCOPE_SCRIPT_WORKSPACE_SEMANTIC.md` documents the example traced below. Historical test claims in those documents are not substituted for checks run in this milestone.

## 4. Traced type-description lifecycle

**Chosen path:** workspace-backed declaration description consumed by Scope Script. This is an example integration, not inferred typing of Scope Script itself.

1. **Original information:** `crates/scope-ide/examples/selection_description_support/mod.rs:10, 44–88` supplies `let answer = 42` and recognizes a whole-line declaration by splitting words, checking the identifier and parsing the literal as `i64`. It deliberately performs no type analysis.
2. **Scope-owned description:** the service advertises `describe_selection` using `SemanticOperationDescription` (same file:28–36), with read effect, selection target and constraints. Its read-only result is provider-defined JSON: `kind: integer_literal_binding`, `literal: "42"`, `partial: true`, `references.status: unsupported`, and `inferred_type: { status: unknown, reason: "fixture has no type analysis" }`. The JSON envelope is Scope-owned; those inner field meanings are provider conventions, not a Scope type algebra.
3. **Host installation:** `crates/scope-script/examples/workspace_semantic.rs:18–40` opens a workspace revision, constructs a document/source and selection, and installs the semantic tool provider/session. The host owns the target and revision, not the script.
4. **Catalog conversion:** `scope-ide/src/tool/semantic.rs:94–195` makes `language.example.declarations.describe_selection`, maps read effect, supplies expected-source/version inputs, and retains metadata/constraints. `AgentSession::catalog` exposes installed descriptions (`tool.rs:479–494`). It does not discover the result's declaration-field schema.
5. **Invocation conversion:** scope, source/version, arguments, policy and selection are checked (`tool/semantic.rs:257–292`). The language receives an empty path at this seam. Its JSON result becomes a `ToolValue` envelope with `status`, `operation_id`, `value`, and `rationale` (lines 301–313). Unsupported numeric values fail conversion; the literal here remains a string.
6. **Unrelated consumer:** `scope-script/src/semantic.rs:50–100` exposes selected discovery metadata, injects host-held preconditions on calls and returns the result value. Script values alias `ToolValue` (`scope-script/src/lib.rs:10`). `examples/workspace_semantic.scope:1–14` projects fields into a report and returns the original result.

**Survives:** literal spelling, source span, partial status, unsupported-reference status, unknown-type status/reason and rationale survive in the full result. Syntactic integer recognition is correctly distinct from successful type inference. The reduced printed report omits the literal, some span detail, rationale and unknown reason; this is consumer projection, not type erasure by inference.

**Missing or lost:** no native nominal identity, recursive value definition, range, unit or assignability information is produced. The original file path is not forwarded to the semantic request. Opaque constraints become text in tool metadata. Script discovery substitutes a `semantic` provider label and does not forward every full catalog field. Native JSON's broader numeric domain is not available through this transport.

**Static inspection:** the operation ID, arguments, effect, target, purpose, constraints and invocation preconditions can be inspected without invoking the semantic operation. The particular declaration and its unknown type require running the read query; they cannot be inferred from its descriptor. There is no machine-readable result schema for an unrelated consumer to use without provider-specific knowledge. “Read-only query” still means executing trusted provider code and requiring authorization; it is not metadata-only discovery.

**Validation evidence:** `crates/scope-script/tests/workspace_semantic.rs:96–186` compares the script result with direct invocation, asserts retained uncertainty and the reduced report, and checks unchanged source/disk state. All three tests in that target passed in SI1. This demonstrates faithful transport, not cross-language type compatibility.

## 5. Structural interoperability feasibility

### 5.1 Temperature probe

Producer identity could be `producer.temperature-celsius@1`; consumer identity could be `consumer.temperature-kelvin@1`. Both are expressible today as separate `TypeId`s. They should remain distinct regardless of whether their representations happen to match.

| Requirement | Existing support | Limitation / consequence |
|---|---|---|
| Signed integer backing for milli-Celsius | `OperationSchema::Integer` describes i64 tool values | Cannot attach it to a nominal `TypeId` or specify milli-degree scale, domain bounds or native encoding |
| Kelvin's own representation | A schema can describe supported tool shapes | The hypothetical does not specify that representation; it must remain unknown until the consumer declares it |
| Independent nominal identities | Exact `TypeId` equality and mismatch diagnostics | No ownership authentication; names/versions need an agreed namespace discipline |
| Representation versus meaning | Separate models exist | No implemented association or semantic barrier joining them; manually replacing an ID with `Integer` would erase meaning |
| Possible conversion | Explicit single-input `Transformation` can connect the two IDs and be discovered | No built-in unit conversion inference, precision proof or validated-adapter certificate |
| Explicit validated adapter before composition | Direct nominal wire fails; an explicitly registered transformation provides a separate plan step | Registration checks duplicate identities/ports, not semantic correctness; host/integration validation must establish adapter fitness |

The intended mathematical relation is Kelvin = Celsius + 273.15, but converting an integer count of milli-Celsius requires the destination's representation, rounding/error policy, overflow bounds and valid domain. For example, milli-Kelvin integers would admit an offset of 273150 **if** that representation and domain were explicitly selected; SI1 does not assume the consumer uses them. Absolute-zero constraints may reject some signed inputs. Scope should report missing information, not select a representation or approve a total conversion over all i64 values.

An adapter should be authored/validated by an integration or independent adapter provider, carry exact endpoint identities and relevant evidence/preconditions, and be explicitly accepted by the host. Core Scope can reason about the declared relationship without containing Celsius/Kelvin semantics. Discovery of its declaration must not label its implementation verified or executable.

**Current outcome:** direct wiring is a nominal mismatch. No discovered route means no declared supported route in that registry, not “conversion impossible.” A registered adapter permits nominal planning today but does not prove semantic safety. Equal integer representations never justify bypassing that adapter.

### 5.2 Positive structural case

Two independent operations can declare an identical closed object contract, for example `{ text: String, labels: Array(String) }`, both fields required. Define the fixture's meaning explicitly as uninterpreted UTF-8 text plus ordered uninterpreted labels, with no hidden normalization, unit, authority or refinement obligations. One operation produces exactly this object; the other accepts exactly it. No semantic conversion or implicit projection is needed.

Existing `OperationSchema` can describe both contracts independently and exact equality can support a **test-local sufficient rule for this restricted declared contract**. Registry validation already checks values against those shapes; it does not currently preflight the two schemas against each other. This establishes compatibility relative to declared contracts, not proof that downstream implementations have no undisclosed assumptions. If either integration declares an extra semantic obligation or cannot affirm the shared contract, return unknown instead.

For the nominal composition model, both integrations can instead intentionally adopt the same shared contract `TypeId`; this works today but demonstrates agreed nominal interoperability, not structural discovery between different private IDs. SI2 should test both and clearly separate the claims.

## 6. Minimum missing capabilities, ordered by necessity

These are **proposals**, not changes in SI1. Begin test-local; production ownership below is conditional on evidence from SI2. Preserve all current exact nominal behavior.

| Priority / capability | Concrete gap and why existing APIs are insufficient | Smallest addition and prospective owner | Compatibility / migration / neutrality |
|---|---|---|---|
| 1. Conservative compatibility outcome | `TypeMismatch`, value validation errors and `discover -> Option` cannot distinguish insufficient information, declared adapter needs and structural success | Test-local `Compatible`, `RequiresAdapter`, `Incompatible`, `Unknown` with reason/path and evidence scope; if promoted, planning belongs in `scope-types`, not execution | Additive query only; do not reinterpret `None` or change `Graph::validate`. Generic contract facts, not language rules |
| 2. Nominal/representation association with an opacity barrier | IDs have no decomposition; schemas have no identity. Neither alone prevents a generic checker from unwrapping a private type | Test-local description containing optional nominal ID, optional existing `OperationSchema`, and explicit structural-vs-nominal contract mode. Missing representation is distinct from a known primitive. Later shared description ownership: `scope-types`, with an IDE-side schema bridge | Never strip identity to compare representation. No automatic migration of existing nominal ports; do not make `scope-types` depend on `scope-ide`. Promote/extract only a demonstrated common subset, not the whole tool runtime |
| 3. Adapter acceptance evidence | `Transformation` already expresses endpoint relationships but registration does not validate conversion semantics | Reuse `Transformation` unchanged. Test-local host acceptance record keyed by exact adapter ID and endpoint descriptions, with declared domain/precision obligations and validation reference | Do not add temperature knowledge to core. Evidence is scoped and may expire when descriptions change; accepted planning still grants no execution authority |
| 4. Representation/domain/precision obligations | `Integer` fixes transport storage, not units, valid subsets or exactness. Opaque semantic constraints are not generic validators | For SI2, retain explicit fixture-owned obligation identifiers and known/unknown status. Return unknown for unevaluated obligations. Add bounded range facts only if a later concrete case needs them | Downstream owns interpretation/validation. No arbitrary predicate language or claims that free-text tags prove safety; additive metadata must not silently widen accepted values |
| 5. Recursive descriptions / portable output contracts | Recursion already exists in `OperationSchema`, but not in `scope-types` or semantic output descriptions | Reuse that algebra in the isolated fixture. Defer a public semantic output-schema field and portable format until a real semantic consumer requires them | Avoid duplicate schema languages, changing existing serialization, enum variants that break exhaustive matches, and mandatory trait methods that break integrations |

**Already present, not new requirements:** exact nominal identity, named operation ports, explicit transformation chains, graph diagnostics and recursive structural decomposition for the restricted tool domain. **Not required now:** native type expansion, general subtyping, function variance, arbitrary precision algebra, theorem proving or a universal unit vocabulary.

A future shared description must distinguish “unknown,” “intentionally opaque” and “known primitive.” Optional metadata must not turn an opaque nominal value into an unrestricted primitive. Type descriptions can be incomplete without their incompleteness being interpreted as universal compatibility.

## 7. Architectural risks and boundaries

1. **Downstream semantic ownership:** adapters describe their own native types and supply validation evidence. Scope reasons only over shared declared facts. It must not infer a language's representation from its spelling or a signature string.
2. **Structural equality is not semantic equality:** apply structural comparison only to explicitly structural contracts with covered obligations. Distinct nominal IDs remain barriers even when decompositions are equal. Representation inspection is not permission to unwrap or coerce.
3. **Unknown is not compatible:** absent Kelvin representation, unsupported constraints, missing adapter evidence and opaque types produce explicit unknown/blocked outcomes. Known shape contradictions can be reported as incompatible for the selected direct contract without asserting that no adapter can ever exist.
4. **Metadata is not authority:** `Transformation.requires`, operation authorization names, read effects, evaluation purpose and Cell advisory descriptors cannot populate runtime grants. Each invocation still passes through host policy and, for component execution, Cell. Pure-looking shape compatibility must not authorize reads or writes.
5. **Declarations are claims:** matching IDs, schemas and adapter records do not authenticate publishers, validate bytes or prove correctness of trusted native provider code. Preserve provenance and validation scope; do not label metadata “certified” by default.
6. **Keep Cell's boundary:** `scope-execution` validates planning and ABI but uses the supplied Cell for runtime operations. SI2 needs no component execution. Do not move capability enforcement or WIT interpretation into the compatibility checker.
7. **No pairwise language core:** an independent adapter can refer to published IDs/contracts without importing either implementation. Temperature formulas and native codecs remain downstream. Core discovery only sees declared transformations.
8. **Existing integrations remain functional:** do not alter shallow semantic argument behavior, tool value encoding, graph exact matching, revisions or public traits. An isolated probe cannot claim a bridge among these APIs already exists.
9. **Bounded inspection:** retain existing schema limits; any future external decoder must bound input before expensive allocation. Schema limits do not sandbox providers or bound their execution.

## 8. Proposed SI2: isolated contract-association probe

**Status: design only; not implemented by SI1.** One integration-test target under `scope-ide` with `tools` enabled is sufficient. Reuse the existing transitive `scope-types` identity through `scope-meta` if appropriate; an eventual direct test-only dependency should be evaluated separately, not added by SI1. Keep all helpers private to the fixture.

### Minimal interfaces and independence

- Two separately authored fixture modules, producer A and consumer B, each publish descriptions using only existing `OperationSchema` and nominal identities. Neither imports the other's module, native types or constants. Have authors write their fixtures from a small shared written vocabulary; audit imports. This simulates independent publication, not independently deployed packages or a proven wire protocol.
- A host-owned test harness pairs descriptions. Test-local description: contract mode (structural or nominal), optional `TypeId`, optional `OperationSchema`, and explicit obligation status. Do not equate missing schema with `Null` or an unconstrained wildcard.
- A conservative test-local comparator uses exact schema equality only for the complete structural subset. It returns outcome plus explanation and the compared endpoint identities. Reject malformed descriptions separately. No optional-field subtyping is needed.
- Adapter declarations reuse `Transformation` and existing `Registry::discover`. Host fixture evidence identifies which adapter declaration is accepted and under which endpoint/domain assumptions. Shape equality must never create such a declaration or acceptance record.
- Invoke no provider in the core probe: inspect descriptors through the existing discovery surface where practical and use a panic/counting provider hook to demonstrate zero dispatch. Native temperature representations are descriptions only, not temperature integrations.

### Cases and expected results

| Case | Fixture setup | Expected result |
|---|---|---|
| Positive structural | A output and B input independently declare identical closed text/labels object, both required, explicitly structural and without additional obligations | `Compatible` for the declared value contract; recursive equality explained; zero calls |
| Positive existing nominal baseline | Independent operations intentionally use the same published shared contract ID | Existing graph validation accepts that wire; label this nominal agreement, not private-type structural equivalence |
| Nominal temperature barrier | A milli-Celsius ID with integer representation; B different Kelvin ID, even if the fixture also gives it integer representation | Never directly compatible; `RequiresAdapter` with distinct IDs if enough declarations are known; existing direct graph wire emits `TypeMismatch` and missing-input diagnostic |
| Adapter declaration versus acceptance | Add one exact-endpoint transformation; first omit host validation record, then provide a fixture record with explicit representation/domain/precision assumptions | Registry can discover the declared chain in both states; SI2 acceptance remains unknown/blocked without evidence, then reports an explicit adapter plan, never direct equivalence or runtime permission |
| Unknown representation | Kelvin identity exists but backing schema or precision obligation is absent | `Unknown` with missing fact; no assumed integer or lossless conversion |
| Known direct incompatibility | Structural object has integer `text` versus required string `text`, or an undeclared extra field in a closed destination | `Incompatible` for direct composition with field explanation; no claim of global nonconvertibility |
| Opacity/version regression | Opaque nominal integer-backed type versus bare integer; same nominal name with different opaque version | Never compatible by unwrapping/version guessing; unknown or adapter-required as supported by available facts; existing graph rejects unequal IDs |
| Authority regression | Discover adapter with a declared requirement and no host grant | Unfiltered discovery may find it; `Graph::validate` reports `MissingCapability`. No checker manufactures grants; zero runtime calls |

For the adapter case, choose and publish a **test-only** Kelvin representation rather than assuming it from the name. Record bounded valid source/destination domains and exactness assumptions; if they are not discharged, acceptance must remain unknown. A fixture validation record tests evidence gating, **not correctness of an unimplemented adapter**. Actual conversion correctness requires a separate downstream implementation/test and is outside SI2's claim.

### Acceptance criteria

1. At least one compatible declared structural contract, one explicit adapter-required relationship and one incompatible/unknown relationship are asserted deterministically.
2. A and B share only the vocabulary, not implementation dependencies or fixture constants; host assembly owns pairing and accepted adapter evidence.
3. Original nominal IDs and relevant obligations remain inspectable in every result. Equal representations never erase nominal boundaries.
4. Reuse existing recursive schemas and nominal graph/discovery algorithms; add no production APIs, dependencies or generalized planner for this probe.
5. Every non-success has an explanation identifying the endpoint, field or missing fact. No inference from opaque constraint text.
6. Provider invocation counts stay zero; no document mutation, Cell execution, runtime grants, temperature integration or automatic semantic-tool bridge is introduced.
7. Results distinguish declared shape compatibility, host-accepted adapter planning and runtime authorization. If the existing pieces cannot support this test-local seam without substantial machinery, stop and report that limit rather than expanding SI2.

## 9. Explicit non-goals

- A complete shared native type system, universal semantic equivalence, units framework, categorical model, theorem prover or arbitrary constraint solver.
- New public APIs, production registries, serialization versions or mandatory downstream methods in SI1.
- Implementing the hypothetical temperature integrations or proving a conversion safe over an unspecified Kelvin representation.
- Runtime execution, provider isolation, capability-policy changes or replacing Cell's enforcement.
- Automatic bridging of semantic tools, revision-bound operations and opaque-byte component pipelines.
- General assignability, width subtyping, coercion insertion, adapter synthesis or multi-input search.
- Beginning SI2 implementation or treating its proposed results as existing behavior.

## 10. Recommendation

**Narrow and proceed.** The current `TypeId`/`Transformation` foundation already answers whether a declared nominal route exists, and `OperationSchema` already provides the small recursive vocabulary needed for a positive structural fixture. The missing association and conservative result semantics can be investigated privately without destabilizing either surface.

Do not extend `scope-types` first, and do not call the existing semantic transport experiment structural type interoperability. The declaration trace and prior S2 results demonstrate preservation of structured data and uncertainty, not machine-readable inferred types or generic output compatibility. SI2 should decide whether the test-local association is genuinely reusable before any promotion into shared production descriptions.

## 11. Validation performed

Run from the repository root, offline, with a 120-second timeout per command; all completed successfully:

| Command | Observed result / coverage |
|---|---|
| `cargo test -p scope-types -p scope-meta --offline` | Passed all selected unit, integration and doc tests. Includes 21 composition tests, 10 manifest tests, meta registry/graph/provider tests and one meta doctest. Run twice; both passed. No interoperability feature is inferred from this success. |
| `cargo test -p scope-ide --features tools --lib tool::operations --offline` | 6 passed, 0 failed; 302 filtered out. Covers schema/metadata limits, policy/revision checks, non-dispatch on rejection, output validation and discovery/authority separation. |
| `cargo test -p scope-script --test workspace_semantic --offline` | 3 passed, 0 failed. Covers workspace semantic result preservation and source/boundary behavior used in the trace. |

Read-only `git --no-optional-locks status --short` was clean before research. No network access, external dependency changes, production edits or SI2 implementation were required. The full workspace, all feature combinations and `scope-execution` runtime tests were not run. Existing test source and prior design documents were inspected beyond the tests above; their historical passing claims are not new validation. The only intended repository change is this document.
