# Structural Interoperability — Research Summary and Architectural Conclusions

## 1. Executive summary

Scope explored structural interoperability to determine whether unrelated downstream systems could describe values and operations using shared primitives, letting Scope discover useful relationships without knowing either system's implementation. The question was not whether structured data could cross an API boundary—that already worked—but whether published descriptions could justify connecting operations while preserving identity, meaning, uncertainty and authority.

SI1–SI5 established that existing mechanisms cover substantial parts of this problem. Exact nominal composition and explicit transformation discovery exist in `scope-types`; recursive transport-value contracts exist in `OperationSchema`; language services publish semantic operation descriptions; and Scope Script supports discovery, structured inspection and explicit orchestration. Curated experiments demonstrated that these mechanisms can cooperate without changing nominal matching or granting execution authority. They did not establish independently deployed cross-project interoperability.

The practical limitation was usually missing endpoint-owned contract information, not a missing universal type algebra. Real semantic operations lack complete published result contracts. The strongest existing operational interaction—a document read followed by checked replacement—requires projection, renaming and argument construction through an explicit host adapter. Structural equality alone cannot perform that work or discharge its revision and permission obligations.

**The initial investigation is complete. Further work is demand-driven.** The chosen direction is to retain exact nominal composition, reuse existing recursive schemas where appropriate, keep native semantics downstream, and preserve explicit adapters and separate authority checks. No generalized type-system expansion, compatibility engine or promotion of experimental helpers is currently justified. Resume real downstream integration work rather than creating another research milestone for its own sake.

## 2. Original hypothesis

Two independently developed projects with unrelated native type systems might publish descriptions using shared Scope primitives. A host could then inspect their producer and consumer contracts and discover an acceptable relationship without importing either implementation or embedding pairwise language knowledge in Scope core.

Three different claims must remain separate:

- **Structural compatibility:** published contracts establish an acceptable relationship between described values at a specified boundary.
- **Semantic equivalence:** those values mean the same thing, including units, normalization, domain restrictions and intended use.
- **Executable composition:** an implemented connection can run with correct encoding, argument assembly, source context, validation and permissions.

Equal integer representations do not make Celsius and Kelvin interchangeable. Equal object schemas do not prove that text is intended as a full-document replacement. Even a justified contract relationship does not provide an executable binding or authorization.

## 3. Existing architectural foundations

### Nominal composition: `scope-types`

`TypeId { name, version }` supplies exact nominal identity. Named typed ports, `Transformation`, `Graph` and `Registry` support explicit wiring, diagnostics and discovery over registered transformations. Graph validation checks exact identities and host-supplied planning capabilities. Registry discovery searches registered single-input transformation routes; it does not synthesize conversions or search arbitrary multi-input assemblies.

Names and versions are declarations, not authenticated ownership or semantic-version compatibility. The crate does not define native records, units, representations or a public nominal-to-structural association. A missing route means no route was found under the registry's search and filtering rules, not that conversion is impossible.

### Recursive contracts: `OperationSchema`

`scope-ide::tool::operations` provides recursive contracts for null, boolean, signed-i64 integer, string, homogeneous arrays and closed objects with required fields. `OperationDescriptor` publishes input/output contracts plus effect, execution classification and authorization names. The registry validates descriptors and validates values around invocation under separate policy, document/revision and resource-budget checks.

Derived equality recursively compares schemas, including requiredness. It is not general assignability or subtyping. These are transport-value contracts, not native layouts or semantic refinements; unions, ranges, units and arbitrary constraints are not part of this algebra. The output contract describes the returned value, separately from `OperationResult`'s document/revision envelope.

### Language integration: `scope-language`

Downstream `LanguageService` implementations own native syntax, semantics and declarations. `SemanticOperationDescription` publishes arguments, effect, target and optional opaque JSON constraints. Argument descriptions are shallow: array items and nested object fields are not recursively specified. Semantic results include provider-defined JSON, but descriptors do not publish operation-specific recursive result contracts.

Human-readable signatures, operation names and observed JSON fields are not substitutes for machine-readable native types or output promises. The semantic adapter also transports numbers through signed i64 tool values, a narrower domain than all JSON numbers.

### Inspection and orchestration: `scope-script`

Scope Script reuses structured `ToolValue` values and exposes host-controlled discovery and invocation. `RegistryHost` projects registered schemas into inspectable values; `SemanticHost` exposes selected semantic reads with host-held context. Scripts can inspect metadata, project results and construct subsequent arguments. Discovery does not itself establish compatibility, and successful transport does not create missing contracts.

### Adapters, authority and Cell

Explicit host adapters own projection, conversion, routing and argument construction. Hosts retain acceptance policy, source/revision context and invocation approval. Registration and discovery can run trusted metadata hooks: zero operation dispatch is not proof that no provider code executes or that providers are sandboxed.

Cell remains the separately enforced runtime capability boundary for component execution. `scope-execution` checks planning identities and component ABI requirements; it does not replace Cell's enforcement or prove the meaning of opaque payload bytes. The investigation did not audit Cell's implementation.

These mechanisms cooperate through deliberate bindings and adapters. They do **not** constitute one unified type system today.

## 4. SI1–SI5 findings

The following are historical observations from the reports, not new validation performed for this summary.

| Milestone | Question investigated | Actual observed result | Limitation or unresolved issue |
|---|---|---|---|
| **SI1 — Discovery** | Which foundations exist, and what prevents structural relationships between native systems? | Identified exact nominal composition, recursive operation schemas and shallower semantic descriptions. Located missing nominal-to-structural associations and semantic output declarations. | Proposed associations and conservative outcomes were designs, not public implemented APIs. Structured transport was not compatibility evidence. |
| **SI2 — Isolated probe** | Can existing identities, schemas and planning coexist conservatively? | Eight curated fixture tests passed: exact structural equality, shared nominal identity, distinct nominal barriers, adapter discovery versus acceptance, missing information, direct mismatch, opacity/version preservation and authorization separation. Inspection did not dispatch providers. | Private helpers and host-supplied assumptions in one test compilation; no implemented temperature conversion or verified adapter. The initial attempt was blocked by dependency restrictions; an authorized test-only dependency enabled the completed follow-up. |
| **SI3 — Consumer review** | Do real semantic descriptions support static producer-result-to-consumer-input comparison? | The declaration-description example and its Script report lacked a published output contract and consumer requirement respectively. Descriptor-only compatibility was unknown. | Sample values and field accesses cannot fill those gaps. Proposed result metadata, schema extraction and boundary experiments were not implemented by SI3. |
| **SI4 — Contract-first probe** | Can independently registered fixture contracts be compared and inspected through Script? | Two fixture providers registered their own descriptors. Existing schema equality supported an exact case, a non-exact case and unknown cases. Script inspected the report without provider dispatch; invocation remained denied. | Independence was declaration/registration within one compilation, not independent deployment or authorship. Complete-boundary promises remained private fixture facts, not public descriptor fields. |
| **SI5 — Adoption review** | Is there a real usable downstream pair with sufficient published contracts? | Identified the existing `scope.document.read` → checked `scope.document.replace` interaction through the harness's explicit adapter. Existing harness and registry-host tests passed. | No independently published real downstream pair with complete comparable contracts was established. Read output was prose-only; replacement required a constructed request. No new demonstration or contract mechanism was implemented. |

SI2's positive registration used one host-created descriptor containing supplied input/output schemas; SI4 improved this evidence by retrieving contracts from two provider registrations. Neither established portable publication. SI5 did not infer absent contracts from runtime success, and external downstream sources unavailable to that review were not treated as evidence of missing capabilities in those systems.

## 5. Architectural conclusions

1. **Retain exact nominal composition.** SI2 confirmed that equal backing representations must not bypass distinct identities or opaque versions. Intentional shared IDs remain a valid existing route, separate from structural discovery.
2. **Reuse existing recursive structural schemas where appropriate.** SI4 needed only borrowed descriptors, a boundary gate and existing equality. Neither a second schema algebra nor extraction into another crate was justified by demonstrated consumers.
3. **Keep native semantics and declarations downstream-owned.** Endpoint owners must publish promises about their values. Core must not derive types from names, prose, signatures, sample executions or implementation field accesses.
4. **Treat missing and opaque information as unknown.** Missing representation is not `Null` or a wildcard. Equal shapes do not discharge hidden or unresolved obligations; absence of evidence is not universal incompatibility either.
5. **Require explicit adapters for transformations.** Projection, conversion, argument construction and assembly are real integration behavior. SI5's working adapter shows why publishing a read schema would not turn that interaction into direct wiring.
6. **Keep structural compatibility distinct from semantic correctness.** Exact contracts can establish a restricted declared-shape relationship, not normalization, unit agreement, replacement intent or faithful implementation behavior.
7. **Separate adapter acceptance, execution and authorization.** A discovered transformation is a declaration. A host-accepted plan is not conversion proof, an executable binding or a grant. SI2 and SI4 deliberately tested these distinctions.
8. **Preserve Cell's capability-enforcement boundary.** Planning requirements and metadata must not manufacture runtime capabilities. Compatibility inspection is not a substitute for host policy or Cell enforcement.
9. **Do not introduce a universal type language, automatic adapter synthesis or general compatibility engine without demand.** The observed gaps concern publication and explicit adaptation. Promoting fixture policy would add shared complexity before demonstrating reusable value.

These decisions constrain future extensions; they do not claim that proposed metadata protocols, acceptance records or compatibility outcomes are production APIs.

## 6. Conceptual model

**Compatibility — Can published descriptions establish an acceptable relationship?** Identify the exact output/input boundary, available schemas, nominal identities and unresolved facts. A conservative answer may be an exact declared match, unsupported direct exact composition, an adapter requirement or unknown. The experimental outcome names are not a shared production API.

**Adaptation — What explicit transformation, projection or assembly is needed?** An integration author supplies the connection: extracting a successful value, selecting fields, renaming keys, converting representations or adding host-owned arguments. Contract inspection can inform this work but does not synthesize it.

**Authority and obligations — Are semantic, revision, validation and permission requirements satisfied?** Preserve source correspondence, freshness, domain restrictions, adapter evidence, runtime budgets and approval. Structural facts cannot discharge unrelated requirements.

```text
Published endpoint descriptions
          |
          v
Compatibility inspection ----> unknown / unsupported / acceptable relationship
                                            |
                                            v
                                  Explicit adaptation
                                            |
                                            v
                         Obligation checks + host authorization
                                            |
                                            v
                           Execution, with Cell where applicable
```

These are distinct responsibilities, not a new unified pipeline implementation. Checks may occur at multiple stages and again during invocation; a positive earlier result never waives later checks.

## 7. Known limitations

**Not established by the investigation:**

- Independently deployed cross-project contract exchange. Fixtures shared a compilation and agreed vocabulary; external deployments were not tested.
- General semantic equivalence or verified adapter correctness. Completeness promises and validation references were assumptions, not proofs; the temperature adapter had no implementation.
- Complete, comparable contracts from a real independent producer/consumer pair. Existing working orchestration is narrower evidence.

**Absent from the inspected interfaces as general facilities:**

- Authenticated publisher identity and portable structural contract versioning. Nominal version strings exist, but do not provide this publication protocol.
- General structural assignability or subtyping; equality and value validation do not supply them.
- Automatic result-to-input composition or adapter synthesis. Explicit invocation and scripting exist, but automatic relationship discovery and wiring do not follow.
- Portable nominal-to-structural declarations. SI2's association was private experimental machinery.
- Complete semantic-operation result contracts, including recursive payloads and outcome distinctions.
- Universal constraint reasoning for domains, units, refinements, conditional requirements or arbitrary opaque constraints.

These limits do not imply that downstream adapters cannot implement domain-specific checks, or that unavailable external integrations publish no contracts. They identify what Scope's inspected shared mechanisms and the recorded experiments do not establish.

## 8. Conditions for reopening the work

Reopen a specific interoperability question when:

- A real downstream producer and consumer need a concrete connection.
- An owner cannot publish a necessary contract through existing interfaces.
- An explicit adapter exposes recurring projection, conversion or assembly duplication.
- Multiple consumers need the same reusable inspection query.
- A missing primitive prevents a demonstrated composition rather than a hypothetical one.

Start with the contract owner or downstream adapter. Specify the payload/envelope boundary, consumer requirements, supported transport domain, unresolved obligations and authority checks. Reuse authoritative declarations rather than maintaining host-written shadow schemas. If publication is insufficient, identify the smallest owner-authored addition before proposing a shared abstraction.

Only repeated concrete requirements should motivate core extraction or promotion. Do not turn SI4's private completeness promise into a blanket public assertion, infer schemas from observations, or restart broad type-system research to avoid an explicit adapter.

## 9. Recommended next direction

**Resume real downstream integration work.** Use existing nominal graphs, registered schemas, language services and Script hosts according to the actual boundary. Keep working adapters explicit, and request additional owner-published contracts only when a consumer needs inspection that cannot otherwise be supported.

Do not propose SI6 merely to continue the numbering. Future work should resolve a demonstrated consumer need and validate the resulting integration, not generalize a speculative type system. Until such demand appears, retain the current architecture and the limitations recorded here.

## 10. References

### Research record

- [SI1 — Structural Interoperability Discovery](STRUCTURAL_INTEROPERABILITY_DISCOVERY.md)
- [SI2 — Structural Interoperability Results](STRUCTURAL_INTEROPERABILITY_RESULTS.md), including the completed authorized follow-up in section 8.
- [SI3 — Real-consumer interoperability boundary review](STRUCTURAL_INTEROPERABILITY_CONSUMER_REVIEW.md)
- [SI4 — Contract-first interoperability results](CONTRACT_FIRST_INTEROPERABILITY_RESULTS.md)
- [SI5 — Downstream contract adoption](DOWNSTREAM_CONTRACT_ADOPTION.md)

### Implementation and experimental evidence

Paths are repository-root-relative; experimental tests are not production compatibility APIs.

- `crates/scope-types/src/lib.rs`: nominal identities, ports, graphs and transformation discovery.
- `crates/scope-ide/src/tool/operations.rs`: recursive schemas, descriptors, validation and invocation.
- `crates/scope-language/src/semantic.rs`: semantic descriptions, argument validation and results.
- `crates/scope-ide/src/tool/semantic.rs`: semantic tool adaptation and source/policy checks.
- `crates/scope-script/src/host.rs` and `crates/scope-script/src/semantic.rs`: registry and semantic hosts.
- `crates/scope-execution/src/lib.rs`: nominal component bindings and execution through Cell.
- `crates/scope-ide/tests/structural_interoperability.rs`: SI2's eight curated cases.
- `crates/scope-script/tests/contract_first_interoperability.rs`: SI4's two-provider inspection probe.
- `crates/scope-ide/src/tool.rs` and `examples/harness/src/main.rs`: document tools and the existing explicit read-to-replacement adapter.
