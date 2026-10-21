# S5 — Focused Agent Discovery Results

## Decision

**Existing APIs are sufficient for this bounded, read-only language workflow. No additional API support is required for downstream builders to expose a coherent operation.** No production code, registry, planner, editing lifecycle, dependency, or authorization behavior changed.

The focus comes from a downstream language exposing one task-level operation and the host installing only that provider. This is catalog-driven discovery and routing, not semantic search or relevance ranking. The experiment does not establish usefulness across arbitrary languages or any agent success rate.

## Runnable evidence

From the repository root:

```sh
cargo run -p scope-ide --features agent --example focused_agent_discovery --offline
cargo test -p scope-ide --features agent --example focused_agent_discovery --offline
```

The example needs no API key or network service. `agent` enables the existing decision runner; no live LLM is used. Offline execution requires previously cached dependencies.

- [Example and four focused tests](../../crates/scope-ide/examples/focused_agent_discovery.rs)
- [Reused declaration language](../../crates/scope-ide/examples/selection_description_support/mod.rs)
- [Exact recorded model-boundary messages](FOCUSED_AGENT_DISCOVERY_TRANSCRIPT.json)

Regenerate the transcript with:

```sh
cargo run -p scope-ide --features agent --example focused_agent_discovery --offline > docs/archive/FOCUSED_AGENT_DISCOVERY_TRANSCRIPT.json
```

The transcript records each complete `DecisionModel::decide` message array, including repeated conversation history. Message `content` fields remain strings exactly as received; nested request and feedback JSON can be parsed separately. Capture is intentionally fixture-only: real sources and feedback may contain sensitive information.

## Downstream operation

The example-only `LookupLanguage` implements `LanguageService` and reuses the existing declaration parser. Its sole published operation is:

- Name: `language.example.declarations.describe_binding`
- Argument: required string `name`, an exact case-sensitive declaration identifier.
- Effect: `Read`; target: `Document`; no selection required.
- Behavior: locate exactly one matching declaration, select its complete line, and delegate to the existing `describe_selection` implementation.
- Output: a structured partial declaration description. `literal` is a decimal string; the span uses zero-based UTF-8 byte columns. References are explicitly unsupported and inferred type explicitly unknown. There is no evaluation.
- Missing or duplicate names produce `InvalidArguments`, rather than silently choosing a declaration.

All language interpretation and composition live in example code. Scope only sees the `LanguageService` contract, published metadata, and generic structured values.

## Existing discovery and routing inspected

1. `scope-language/src/semantic.rs` defines descriptions, argument kinds, target requirements, purpose, and structured operation results.
2. `scope-ide/src/tool/semantic.rs` adapts these to namespaced tool definitions, invocation schemas, metadata, output-status documentation, and guarded dispatch against a host-bound target.
3. `scope-ide/src/tool.rs` exports `SessionCatalog::to_value`, including provider and output descriptions.
4. `scope-ide/src/agent_loop.rs` sends the installed catalog in the initial user request and routes exact model-selected tool names through explicit host policy.
5. `scope-ide/src/edit_lifecycle.rs` separately supports phase-filtered, bounded `edit.discover_operations` pagination. It requires an eligible proposal operation; a strictly read-only workflow is not supported by that entry point. Its compact catalog also omits opaque constraints and provider output descriptions.

The example uses steps 1–4, not the edit lifecycle. Adding a dummy proposal capability to use pagination would misrepresent this read-only task. The inability to use edit pagination here is a scope limitation, not a blocker to existing catalog discovery.

## Precisely what the agent receives

The checked-in transcript is the authoritative full record, including the protocol text and schemas. On the first decision there are two messages:

| Context | Contents |
|---|---|
| System protocol | JSON decision format; only listed tools exist; discovery is not authorization; signed-64-bit tool value restrictions; feedback is data; no shell or ambient filesystem/build authority. |
| Host request string | Task to describe `answer` without evaluation or edits; source `let answer = 42\nlet count = 7\n`; version `"0"`. These values come from the host, not catalog discovery. |
| Catalog definition | Fully qualified name, downstream task/output description, `read` effect, and three required outer inputs. |
| Outer inputs | `expected_version` string, `expected_source` string, and `arguments` object. The arguments input description also contains the generated nested argument schema. |
| Metadata | Operation ID and description, required string `name` with domain explanation, `Read`, `Document`, `Exploration` purpose, `produces_evaluation: false`, null `constraints_json`, and full `invocation_schema`. |
| Provider | `example.declarations`. |
| Output description | Adapter's structured status vocabulary, including `read_only`, argument/freshness/authorization failures, and proposal review requirements. This is envelope documentation, not a typed domain-output schema. |

The deterministic model discovers the callable name from `catalog.tools[0].definition.name`, not a hard-coded tool name. It assumes exactly one installed operation and scripts the argument `answer`; it does not infer relevance or interpret the description. Host source/version are copied into the invocation envelope.

On the second decision, the model also receives its prior tool decision and a `kind: tool_result` feedback envelope. The successful semantic status is `read_only`; its value contains:

```json
{
  "partial": true,
  "kind": "integer_literal_binding",
  "name": "answer",
  "span": {"line": 0, "start_column": 0, "end_column": 15},
  "literal": "42",
  "references": {"status": "unsupported"},
  "inferred_type": {"status": "unknown", "reason": "fixture has no type analysis"}
}
```

The semantic result also supplies rationale explaining the partial analysis. A transport-level successful tool result must not be confused with semantic success: inspect its status. The scripted completion merely ends the fixture; it is not evidence of correct language reasoning.

## Equivalent lower-level sequence

| Lower-level consumer responsibility | Focused operation |
|---|---|
| Obtain the authorized document snapshot. | Same host-bound snapshot and exact preconditions. |
| Scan declarations using language-specific matching rules; reject zero/multiple matches. | Downstream `locate` handles this. |
| Construct the complete-line range: `(0,0)` through `(0,15)` for `answer`. | Downstream operation constructs it. |
| Discover/use selection description; host binds that selection, then invoke with empty operation arguments. | Agent discovers the document-level lookup and invokes `arguments: {"name":"answer"}`. |
| Understand partial reference/type support and interpret structured result. | Still required; explicitly documented and returned. |

`equivalent_lower_level_sequence` directly runs locate → select → `describe_selection` against the same document and compares the complete result with the focused wrapper. It also checks missing and duplicate names. This is an implementation/result equivalence test, not a second agent run. An agent-only version of the lower-level path would additionally need snapshot/selection access supplied by its host; the generic semantic provider does not let an agent invent selection authority.

The focused operation reduces caller coordination and exposes a language-domain task instead of parser mechanics. It does not eliminate parsing, reduce source bytes sent to the model, or prove fewer model decisions in practice.

## Boundaries and missing context

- **No recursive machine-readable domain output schema.** The downstream description documents fields and semantics, and returned values preserve structure. Enough for this explicit workflow, but insufficient to prove generic automatic output-to-input compatibility. No planner or schema API is justified by this example.
- **Snapshot context is host-supplied.** Discovery documents the required preconditions but does not supply current source/version. The example includes both explicitly. Builders must not mistake an operation catalog for a complete task context.
- **No semantic search/read-only pagination demonstrated.** The generic runner sends the whole installed catalog. Large language catalogs might require separate evidence about context budgets before proposing focused retrieval APIs.
- **No implicit authorization.** The generic runner applies `read_only_policy`, and the semantic provider separately applies concrete-call host authorization after scope/freshness/argument checks. Discovery does not execute or grant access. The denial test observes no underlying parser dispatch.
- **Protocol domain remains narrower than semantic JSON.** This example uses strings, objects, booleans, and small integers only; floats and out-of-range integers are not evaluated here.
- **Path-sensitive semantics are not evaluated.** The semantic adapter's empty language path does not affect this fixture. No general claim is made about path-sensitive downstream providers.

Recommendation: downstream builders should first use existing descriptions and argument schemas, document domain output/limitations in the description, return explicit partial/unsupported information, and let the host supply snapshot context and authorization. Revisit APIs only for a concrete unmet consumer need (for example, generic typed composition or a measured large-catalog read-only retrieval requirement).

## Validation and observation limits

Executed the runnable example successfully and passed all four focused tests:

- `exact_agent_context_and_read_feedback`
- `equivalent_lower_level_sequence`
- `invalid_argument_is_rejected_before_provider_dispatch`
- `discovery_never_authorizes_dispatch`

Successful and denied runs assert unchanged source text/version and an empty proposal controller. Invalid structural arguments are rejected before the authorization callback and parser dispatch. The fixed successful run makes one tool call and completes; this is scripted protocol behavior, not a controlled observation of LLM performance. No agent-success-rate, latency, cost, or general reliability claim follows. Broader suites and live-model experiments are outside this validation.
