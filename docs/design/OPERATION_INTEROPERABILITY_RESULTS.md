# S2 — Read-Only Operation Interoperability Results

## Decision

Existing APIs support a fixed, native two-call read-only workflow with a meaningful structured data dependency. No production interface changes or general-purpose composition layer are needed for this experiment.

The runnable fixture is [`operation_interoperability.rs`](../../crates/scope-ide/tests/operation_interoperability.rs). Starting from [S1's inventory](OPERATION_COMPOSITION_INVENTORY.md), it exercises the **language-semantic / agent-tool adapter path**, not the separate revision-bound `OperationRegistry` path. It does not establish automatic interoperability between those registries.

## Run

```sh
cargo test -p scope-ide --features tools --test operation_interoperability -- --nocapture
```

No model, network service, filesystem source, or API key is used at runtime. Cargo must already have dependencies available, or fetch them normally. `--nocapture` displays the captured `Result<ToolResult, ToolError>` for each attempted call, retaining structured statuses instead of turning every failure into an unstructured string.

Validation performed:

```sh
cargo test -p scope-ide --features tools --tests --offline
```

Passed, including all six S2 tests. This does not validate other feature combinations.

## Fixture and bounds

The host creates one scratch document through `ScratchBuffers`, with text `alpha beta alpha\n`. A setup-only identical-text edit advances its version from 0 to 1, making revision 0 a genuinely obsolete token without changing the source text. All operation execution occurs after setup, with identity, source text, and revision asserted unchanged afterward.

A minimal **test-local** `LanguageService` supplies `words` and `occurrences`. The existing selection-description examples do not supply this dependent query pair. The fixture uses existing `SemanticOperationDescription` and argument types; it introduces no descriptor/schema types, registry, dispatch system, or production language behavior. Its miniature whitespace semantics remain entirely in the integration test.

The fixed workflow is:

1. Bind `SemanticToolProvider` to the host-owned target, document scope, review controller, and mandatory call-aware policy.
2. Discover both operation names from `AgentSession::catalog()` using semantic metadata; verify their read effect and document targeting.
3. Invoke `words` with the exact source text and decimal version preconditions.
4. Extract `value.words[0]` from the structured result, rather than supplying a hard-coded second argument.
5. Invoke the discovered `occurrences` tool with that value as `arguments.word`, retaining the original source preconditions.
6. Observe `value = { word: "alpha", count: 2 }` and verify no source/revision/identity change or review proposal.

Execution is bounded to two sequential calls on a fixed 17-byte source, no retries, no recursion, no evaluation, and finite whitespace scans. Rejection cases dispatch at most one provider call. A wrong-scope case rejects the first attempt and stops. The adapter itself does **not** impose a wall-clock deadline or sandbox arbitrary native provider code; the fixture's provider is directly inspectable and finite. The test command is externally bounded by the validation runner.

## Focused evidence

| Test | Observed result | Dispatch / authorization evidence |
|---|---|---|
| `two_discovered_reads_consume_structured_output` | Two `read_only` results; second reports `alpha`, count 2 | Two provider calls and two host policy checks |
| `incompatible_result_is_rejected_before_dispatch` | Passing the entire first word array where a string is required returns `invalid_arguments`, including corrective invocation schema | Second provider call and policy check do not occur; argument validation precedes authorization |
| `stale_revision_is_rejected_before_dispatch` | Second call with obsolete revision 0 returns `revision_conflict` despite identical source text | No second provider dispatch or policy check |
| `source_precondition_is_preserved` | Correct version with different expected source returns `revision_conflict` | No second provider dispatch or policy check |
| `second_read_requires_authorization` | First call succeeds; host policy denies the second with `unauthorized` | Two policy checks but only one provider dispatch |
| `discovery_does_not_grant_source_authority` | Provider can be discovered, but scope for another identity yields `unauthorized` | Neither host policy nor provider executes |

Every case asserts the target's original identity, revision, and complete source text after execution, and an empty proposal controller. Expected semantic failures are `Ok(ToolResult)` with status fields, not transport-level `Err(ToolError)`. Both channels are retained and printed; no transport error is deliberately induced here.

## Supported behavior

- Discovery and programmatic invocation interoperate through the existing semantic adapter and session catalog.
- Structured arrays, objects, strings, and integer results survive the adapter and can feed a subsequent invocation.
- Required argument kinds are checked before provider execution.
- Read operations still require explicit host authorization and correctly scoped source targeting.
- Exact source and numeric revision preconditions protect both calls. The provider is bound to a single target identity; the policy additionally checks that identity on each authorized call.
- A read-only workflow can stay entirely in native Rust with no source mutation, proposals, or acceptance step.

## Unsupported or not demonstrated

- **Automatic typed composition:** semantic descriptions have input kinds but no recursive machine-readable output schema. The consumer must understand `words`' result shape. Successful value reuse is not a proof of generic type compatibility.
- **Registry bridging:** this experiment does not register semantic tools in `OperationRegistry`, convert schemas, or reuse its opaque `Revision` tokens. Numeric source versions and capability-backed document revisions are not interchangeable.
- **Concurrent live edits:** the real scratch target is immutably borrowed throughout the session. A host edit between these two calls is not possible through that borrowed handle. The stale test therefore uses an authentic older token produced during setup; it does not simulate concurrent mutation or test the adapter's post-dispatch conflict branch.
- **Persisted-document behavior:** the experiment creates an in-memory scratch document using an existing source API. It does not claim filesystem race detection, saving, or reopening behavior.
- **Untrusted-provider isolation:** effect metadata is not a purity proof or runtime limit. No arbitrary provider safety claim follows from this deterministic fixture.

## Actual API limitations versus inconvenience

### Actual limitations

1. Semantic outputs are opaque JSON (wrapped as `ToolValue`), with prose output descriptions, rather than typed recursive output contracts. Generic preflight wiring cannot be established from semantic discovery alone.
2. Semantic and registry operations remain separate contract and revision models. No automatic bridge is established by these APIs.
3. The semantic adapter constructs `LanguageDocument` with an empty path; path-sensitive services cannot rely on the original path through this entry point. The fixture deliberately does not require a path.
4. The agent transport has signed-integer numeric values, not the full JSON numeric domain. This fixture uses only small integers; broader numeric interoperability is not demonstrated.

### Merely inconvenient usage

- Manual `ToolValue` object/array matching and knowledge of the `status` / `value` envelope.
- Copying exact source text and string-encoded version into both envelopes.
- Looking up operation IDs through catalog metadata and then dispatching by the discovered tool name.
- Distinguishing structured semantic rejection from transport failure.

These are manageable in a fixed native consumer and did not block the experiment.

## Possible minimal upstream improvements

These are follow-up candidates, not changes made by S2:

- Document the semantic result envelope and numeric transport constraints alongside the public invocation API.
- Consider optional semantic output-contract metadata if a concrete consumer needs preflight compatibility checks; do not create a parallel schema system merely for this fixture.
- Preserve the actual language path through the adapter if a path-sensitive provider demonstrates the need.

No scripting language, general-purpose composition engine, production registry changes, Cell changes, or downstream-language-specific Scope behavior was introduced.
