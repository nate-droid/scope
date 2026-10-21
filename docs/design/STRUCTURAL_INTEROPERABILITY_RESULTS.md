# SI2 — Structural Interoperability Results

## Current status: completed test-local probe

The authorized follow-up added the development-only dependency and executed all eight scenarios successfully. See section 8 for current implementation, observations, validation, and limitations. Sections 1–7 below preserve the original blocked attempt as historical context; their statements of non-implementation describe that attempt only.

## Historical status: blocked before implementation

The requested experiment was not implemented. Inspection found a conflict between the required test location/API use and the prohibition on dependency changes. This is a packaging boundary, **not evidence that Scope's abstractions cannot express the experiment**. No SI2 compatibility outcome was observed, and the success criterion remains unmet.

Read `STRUCTURAL_INTEROPERABILITY_DISCOVERY.md` before investigation. Its suggested transitive access works for `TypeId` only, not for the other required nominal planning interfaces.

## 1. Architecture and verified source locations

The intended architecture remains one `tools`-gated integration target under `crates/scope-ide/tests/`, with independent producer/consumer modules and private host-owned comparison and acceptance evidence. No such target or supporting code was added.

Verified interfaces:

- `crates/scope-ide/src/tool/operations.rs`: public recursive `OperationSchema`, exact `PartialEq`/`Eq`, operation descriptors and provider discovery/invocation boundaries.
- `crates/scope-types/src/lib.rs`: public `TypeId`, `Transformation`, `Registry`, `Graph`, graph diagnostics and discovery. `Graph::validate` uses exact identities and host-held grants; `Registry::discover` returns declarations, not conversion evidence or authority.
- `crates/scope-meta/src/lib.rs`: re-exports **only `TypeId`** from `scope-types`.
- `crates/scope-ide/Cargo.toml`: depends on `scope-meta`, but has no direct dependency or dev-dependency on `scope-types`. The existing `tools` feature adds no dependency.

Rust integration targets can import their package's direct dependencies; a transitive package's public items are not automatically in their extern prelude. Thus `scope_meta::TypeId` is available here, but `scope_types::{Transformation, Registry, Graph}` is not. Inspection of `scope-ide` and `scope-meta` found no alternative re-export of those planning items.

The minimal ordinary solution would be a **test-only path dev-dependency** from `scope-ide` to `scope-types`. It requires no new type-system primitive and no production functionality, but it is still a dependency change, explicitly prohibited by this milestone. No manifest or lockfile was changed.

Source inclusion, manually locating/linking build artifacts, or duplicating graph/discovery code would not be sound substitutes for testing the existing published interfaces. A production re-export would also violate the constraints. Moving the test to another package would violate the requested location.

## 2. Independence and ownership boundaries

No independent fixtures were implemented, so independence, deterministic comparison, evidence gating and zero provider calls have **not** been established by a new test.

The proposed ownership remains appropriate: integrations declare native identities, representations and semantic obligations; the host pairs descriptions and retains acceptance evidence. Discovery must not construct grants. Cell and language-specific semantics remain outside this experiment.

Only this results document was added by SI2 work. The pre-existing untracked discovery document was preserved. No source documents, persistent workspace data, production APIs, features or dependencies were edited. Validation ran existing tests and updated ordinary build artifacts; this is not an instrumented proof of workspace non-mutation by a new probe.

## 3. Scenario outcomes

| Scenario | Intended observation | Actual SI2 observation |
|---|---|---|
| 1. Independent closed text/labels objects | Compatible by exact recursive schema equality; no obligations; zero dispatch | Not exercised |
| 2. Shared nominal identity | Existing graph accepts intentional shared identity | Not exercised by SI2; existing composition suite passed |
| 3. Distinct milli-Celsius/Kelvin identities | Direct wire rejected; explicit adapter needed | Not exercised |
| 4. Adapter discovery and host acceptance | Route discoverable; no evidence means blocked/unknown; scoped evidence permits an adapter plan only | Not exercised |
| 5. Missing representation or obligation | Unknown with missing fact identified | Not exercised |
| 6. String/integer field mismatch | Incompatible for direct structure, without ruling out adapters | Not exercised |
| 7. Opacity and different versions | No primitive unwrapping or version guessing | Not exercised |
| 8. Ungranted adapter capability | Discovery cannot make graph validation accept missing grants | Not exercised |

These expectations are not reported as test results.

## 4. Actual validation

Commands ran from the repository root, offline, each with a 120-second timeout:

| Command | Observed result |
|---|---|
| `cargo test -p scope-types --offline` | Passed: 21 composition tests, 10 manifest tests; no failing unit/doc tests |
| `cargo test -p scope-ide --features tools --lib tool::operations --test operation_interoperability --offline` | Six operation unit tests passed; 302 unit tests filtered. The name filter excluded all six integration tests, so this invocation did not validate that integration target |
| `cargo test -p scope-ide --features tools --test operation_interoperability --offline` | Reran without the filter: six integration tests passed |

No new integration test command exists because no SI2 target was created. No unrelated test failures were observed. Cargo emitted existing duplicate-package warnings for vendored copies in the cached Cell checkout; these did not fail validation. The full workspace and runtime execution suites were not run.

## 5. Intended versus implemented behavior

The entire test-local comparison/association experiment remains unimplemented. There is no new description structure, comparator, acceptance record, provider counter or executable scenario coverage. The blocker is smaller than substantial new infrastructure, but bypassing it would still require violating the explicit no-dependency-change constraint. This report records that precise boundary rather than presenting partial structural checks as completion.

## 6. Evaluation

1. **Can independent descriptions be compared using existing infrastructure?** Source inspection supports feasibility for a restricted exact-schema rule plus existing nominal planning. SI2 has not experimentally demonstrated it.
2. **Can nominal identities and structural representations coexist?** The existing pieces can be retained side by side in a private association. Whether the proposed comparator preserves every boundary remains untested.
3. **Is a new common primitive required?** No evidence establishes that need. The observed blocker is dependency visibility, not missing schema expressiveness for the proposed cases.
4. **Does association logic deserve promotion?** No association logic was implemented; there is no new promotion evidence.
5. **Would promotion simplify architecture?** Unproven. Promoting a parallel description before exercising a real consumer risks adding another representation and an undesirable dependency from types to IDE schemas.
6. **Could this serve semantic operations and Scope Script without language-specific core logic?** Plausible but unproven. Semantic output contracts, schema bridging, transport domains and host authorization remain separate concerns. No language-specific core rules are justified.
7. **What remains unproven?** All SI2 scenarios, independently deployed interoperability, publisher authenticity, portable schema exchange, runtime encoding/ABI agreement, hidden semantic obligations, actual adapter correctness, domain/precision validation, execution authorization and evidence lifecycle.

Passing existing tests is baseline regression evidence only, not evidence of structural interoperability or conversion correctness.

## 7. Recommendation for SI3

**Conduct the isolated experiment; do not promote an abstraction yet.** First explicitly permit the minimal test-only `scope-types` path dev-dependency in `scope-ide`, while continuing to prohibit production dependency/API/feature changes. Then implement and run the original eight SI2 scenarios, including negative acceptance and authority tests. This is a proposed relaxation, not a change made here.

If even a test-only dependency is unacceptable, stop this experiment at the documented packaging boundary. Do not introduce a production re-export, a second planner/schema algebra, or a common type primitive solely to evade it.

## 8. Authorized SI2 follow-up — observed implementation

### Locations and verified interfaces

- `crates/scope-ide/tests/structural_interoperability.rs`: one integration target, gated at crate level with `#![cfg(feature = "tools")]`; eight tests and exclusively private experimental helpers.
- `crates/scope-ide/Cargo.toml`: adds only `scope-types = { path = "../scope-types" }` in a new `[dev-dependencies]` table (the current checkout had no existing development table). No production dependency, feature, public API, or re-export changes.
- `Cargo.lock`: Cargo added exactly one dependency-list entry, `scope-types`, to the existing `scope-ide` package. No package versions, sources, or checksums changed.

Current source inspection confirmed exact `TypeId` name/version matching, `Graph::validate` diagnostics and host-held planning grants, and `Registry::discover`'s existing declaration-only single-input search. `OperationSchema` already has recursive equality and closed objects; `OperationRegistry::register` validates descriptors, and `descriptors()` returns an iterator of operation IDs and borrowed descriptors. No `scope-types` or `scope-meta` implementation was modified.

### Experimental association and independence

Private `Description` retains source provenance, explicit structural/nominal mode, optional nominal identity, optional existing `OperationSchema`, and integration-owned semantic obligations. `Comparison` retains both complete endpoint descriptions. Structural success uses only existing exact recursive schema equality and requires no additional obligations. A top-level differing-field diagnostic explains a mismatch; it is not a recursive validator or an assignability implementation. Missing representation remains `None`, never `Null`, a wildcard, or an assumed integer.

The inline `producer` and `consumer` modules each import only the shared private description vocabulary and public Scope interfaces. Neither imports the sibling module, its native types, or its constants. Both separately spell the recursive object contracts and published nominal identity. This is source-inspected independence within one test target, not separately authored/deployed package evidence. All pairing, comparison, graph assembly, adapter declarations, and acceptance records are owned by the enclosing host test code. The shared written vocabulary explicitly limits text/labels to uninterpreted text and ordered uninterpreted labels without normalization or refinement promises.

`Acceptance` binds the entire transformation declaration and both endpoint descriptions (including provenance, representations, and obligations), plus a nonempty fixture validation reference. Tests reject stale provenance and changed declaration requirements. It is a deliberately limited, single-adapter evidence gate, not an evidence verifier or generalized conversion solver.

### Scenario-by-scenario observations

| Scenario / test | Observed assertion |
|---|---|
| 1. `structural_success_without_provider_execution` | Independent closed objects with required `text: String` and `labels: Array(String)` yield `StructuralEqual`, with empty obligations and distinct retained provenance. Registered/discovered operation input/output schemas match these descriptions. Provider call count is zero. |
| 2. `shared_nominal_identity_uses_existing_graph_validation` | Independently declared output/input ports intentionally publish `published.text-labels@1`; the wired graph has no diagnostics with empty planning grants. This is nominal agreement, not structural discovery. |
| 3. `equal_integer_representations_do_not_cross_nominal_barrier` | Integer representations compare equal, but milli-Celsius and milli-Kelvin identities yield `RequiresAdapter`; existing direct graph validation reports both `TypeMismatch` and `MissingInput`. |
| 4. `adapter_declaration_is_not_host_acceptance_or_conversion_proof` | Existing registry discovers one registered transformation and nominal graph validation accepts its declaration. Host composition stays `Unknown` without scoped evidence; exact fixture evidence yields `PlannedAdapter`, not direct compatibility. Stale provenance or changed requirements invalidate acceptance. |
| 5. `missing_information_is_unknown_not_a_wildcard` | Missing structural or Kelvin representation yields `Unknown("missing representation")`; an undischarged structural normalization obligation yields `Unknown` naming that fact. Adapter obligations require the separate acceptance gate. |
| 6. `closed_object_mismatch_identifies_text_field` | Replacing consumer `text` with integer yields direct `Incompatible`, explicitly naming `text`. This does not rule out a separately declared adapter. |
| 7. `opacity_and_versions_are_not_erased` | Nominal integer backing does not permit bare-integer unwrapping; changing version `1` to `2` requires an adapter and the nominal graph rejects the wire. Identical nominal declarations are separately labeled `NominalIdentity`, not structural or semantic proof. |
| 8. `inspection_and_host_acceptance_cannot_manufacture_authority` | Unfiltered discovery finds the capability-requiring adapter, and host acceptance can label its relationship planned. Graph validation still reports exact `MissingCapability`; discovery filtered by empty grants returns no route. Grants remain empty. |

The temperature fixture explicitly selects **milli-Kelvin integers**, not an inferred representation. Source domain is declared as -273150..=1000000 milli-Celsius and destination domain as 0..=1273150 milli-Kelvin, with exact i64 transport. The host fixture reference records an assumed exact offset of 273150 over those domains. No adapter implementation exists here and no conversion mathematics, domain enforcement, overflow behavior, or precision correctness is proved by accepting that record.

### Non-execution and boundary evidence

The probe uses the existing `OperationProvider` hook: invocation increments an atomic counter and panics. Registration and descriptor discovery leave its counter at zero. Other scenarios use only plain descriptions and existing nominal registry/graph APIs, which have no provider binding. No document/workspace/session instance is constructed, no mutation API is called, no Cell runtime is constructed or invoked, and no runtime capability token is created. Empty planning-grant sets are never populated from requirements. This is evidence about the fixture's code path, not a sandbox audit or a runtime enforcement test.

Existing exact nominal semantics remain unchanged: shared identities succeed while distinct temperature identities and opaque versions fail direct graph validation. A structurally equal description is not inserted into the nominal graph as a substitute identity. Host acceptance does not imply planning capability approval, and planning capability approval would still not be a runtime authorization token.

### Validation of the follow-up

Run offline from repository root, with 120-second bounds:

| Command | Result |
|---|---|
| `cargo test -p scope-types --offline` | Passed: 21 composition tests, 10 manifest tests; zero unit/doc tests, no failures. |
| `cargo test -p scope-ide --features tools --test structural_interoperability --offline` | Passed: all 8 tests, 0 filtered out. |
| `cargo test -p scope-ide --features tools --lib tool::operations --offline` | Passed: 6 operation-registry unit tests, 302 unrelated unit tests filtered out. |
| `cargo test -p scope-ide --features tools --test operation_interoperability --offline` | Passed: all 6 existing integration tests, 0 filtered out; deliberately run separately without the unit-test name filter. |

The first probe compile found an iterator API mismatch (`descriptors().len()`); this was corrected to use the actual iterator API before the passing runs. No unrelated failures were observed. Cargo still emits duplicate-package warnings for vendored packages in the cached Cell checkout. Full workspace and runtime execution suites were not run.

### Supported conclusions, limits, and next experiment

The existing infrastructure supports this restricted test-local association: exact recursive structural declarations, unchanged nominal planning, adapter discovery, scoped host acceptance, and capability diagnostics can coexist without erasing their distinctions. No second recursive schema language, nominal matching change, new authorization behavior, language-specific core rules, or production API was required. The authorized test-only dependency resolved the original packaging boundary.

A passing fixture is **not independently deployed interoperability**. The declarations are curated in one Rust compilation and share a small written vocabulary. Names are not authenticated publisher identities; matching declarations do not establish that providers honor them. The acceptance reference is a host assumption, not a certificate, trusted proof, or implementation test. Opaque obligations have no generic interpretation. No portable schema exchange, evidence lifecycle/revocation, native ABI/codec agreement, hidden-assumption detection, optional-field subtyping, runtime adapter correctness, or runtime authority enforcement is established.

For SI3, keep these helpers private and exercise independently packaged producer/consumer descriptions with host-owned pairing and a separately implemented bounded adapter. Test actual values and explicit domain/precision failures downstream, including stale evidence and authorization rejection. Establish a concrete portable description/transport requirement before proposing any shared production API; do not promote this fixture's association or acceptance record merely because its eight tests pass.
