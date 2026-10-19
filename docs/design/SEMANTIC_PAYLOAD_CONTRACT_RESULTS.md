# S3 — Semantic Payload Contract Results

## Decision

**No upstream changes are necessary before beginning the bounded scripting experiment. Proceed directly to S4 using existing contracts.**

Review of [S1's inventory](OPERATION_COMPOSITION_INVENTORY.md), [S2's results](OPERATION_INTEROPERABILITY_RESULTS.md), and its [runnable fixture](../../crates/scope-ide/tests/operation_interoperability.rs) found no demonstrated payload-contract defect blocking S2's fixed, read-only workflow. No production changes, new descriptors, or additional test fixtures are justified by that evidence. This is not a claim that semantic output contracts are complete or that arbitrary operations compose automatically.

## Evidence and classification

| S2 observation | Classification | S3 disposition |
|---|---|---|
| `words` returns an object containing an array of strings; projecting its first element into `occurrences.word` returns `alpha`, count 2 | Successful structural interoperability | Existing structured result handling is sufficient for explicit, host-understood wiring. |
| Passing the entire array as `word` returns `invalid_arguments`, with invocation schema, before the second policy check or provider dispatch | Structural incompatibility, correctly rejected | This is an intentionally malformed consumer argument, not loss of payload information or a missing validation contract. No coercion or schema change is warranted. |
| Semantic descriptions lack recursive output schemas | Structural metadata limitation | Discovery alone cannot establish generic output-to-input compatibility. S2 succeeds with explicit result projection; it does not demonstrate a consumer requiring automatic preflight checking. Do not add output metadata speculatively. |
| Semantic and registry operations use separate contracts and revision models | Cross-surface structural/integration limitation | S2 uses only the semantic adapter. No attempted bridge failed; do not invent one or interchange revision tokens. |
| Semantic JSON supports numbers outside the agent transport's signed-integer domain | Payload representation limitation | Real restriction, but S2 uses only representable small integers. No numeric failure was exercised; broadening or coercing transport values is not justified here. |
| The semantic adapter supplies an empty language path | Provider-context limitation, potentially relevant to language semantics | S2's provider does not need a path. No path-sensitive failure was demonstrated, so do not change source targeting for this milestone. |
| Stale version, wrong expected source, denied second call, and wrong source scope are rejected | Freshness and authorization boundaries, not payload incompatibility | Preserve these behaviors; discovery and successful structural matching must not bypass them. |

### Structural versus language-semantic compatibility

A string array is structurally different from a string; the existing argument validator detects that exact mismatch. The corrected usage is already present in S2: select `value.words[0]` and supply it as `arguments.word`. Both the successful projection and the rejected array are covered by focused tests; S3 does not introduce a fix for behavior that already works.

By contrast, equal structural kinds cannot establish that two strings refer to the same language concept, target, symbol, unit, or interpretation. The fixture's test-local provider defines both word queries over the same immutable snapshot. That supplies the domain agreement for this example, not a general language-semantic guarantee. S2 demonstrates no language-semantic incompatibility that Scope should resolve. Downstream providers and explicit consumers must retain that knowledge; Scope should not infer it from names or opaque constraints.

## Selected limitation and implementation

**None selected for production changes.** The missing semantic output schema is meaningful for generic preflight composition, but it did not block S2 and is not evidence that S4 needs a new contract. The numeric, path, and cross-registry restrictions likewise remain documented boundaries, not newly reproduced failures. Introducing a fixture solely to justify expanding one of these contracts would exceed this evidence-driven milestone.

No production code, test code, dependencies, operation registry, invocation framework, type system, authorization policy, or proposal lifecycle changed. Existing S2 tests provide the focused evidence for the no-change outcome.

## Validation

Ran successfully for S3:

```sh
cargo test -p scope-ide --features tools --tests --offline
```

All tests in this command passed, including all six S2 tests:

- `two_discovered_reads_consume_structured_output`: catalog discovery, read effect/document targeting, result projection, two independent policy checks and provider calls.
- `incompatible_result_is_rejected_before_dispatch`: array-to-string mismatch remains rejected before the second authorization check or dispatch.
- `stale_revision_is_rejected_before_dispatch`: genuinely obsolete numeric version remains rejected even with unchanged text.
- `source_precondition_is_preserved`: exact source precondition remains enforced.
- `second_read_requires_authorization`: denial prevents the second provider dispatch.
- `discovery_does_not_grant_source_authority`: wrong identity scope prevents policy and provider execution.

Every S2 case also checks unchanged source identity, revision, and complete text, plus an empty proposal controller. The broader command exercises existing scope-ide tests in the `tools` configuration, including selection-description and source-edit-controller behavior. It does not validate every feature combination, downstream language provider, automatic registry bridging, concurrent live mutation, or arbitrary numeric payload.

## Recommendation for S4

Proceed with a small, explicit scripting consumer of the existing semantic adapter, not an automatically typed planner:

1. Use the installed catalog and host-bound target. Keep operation selection and result projection explicit; do not claim that discovery proves output compatibility.
2. Check the semantic `status` envelope before reading `value`, and retain transport errors separately. `Ok(ToolResult)` alone is not semantic success.
3. Preserve structured values exactly within the transport's supported domain. Do not silently coerce arrays to strings or unsupported numbers to integers/strings.
4. Retain exact source/version preconditions and mandatory host authorization for every call. Do not treat discovery or script validation as authority.
5. Keep language interpretation downstream and the initial experiment bounded and read-only. Do not bridge revision models or introduce a new proposal lifecycle.

If S4 actually requires generic preflight wiring, unsupported numeric values, or path-sensitive dispatch, record the precise failing call and consumer requirement before proposing a narrowly scoped adaptation of existing contracts. Current evidence supports beginning scripting without those upstream changes, not promising unrestricted interoperability.
