# Projected semantic arguments and strict decisions

## Host-owned preconditions

`SemanticToolProvider::with_projected_arguments()` is an additive public opt-in returning a `ToolProvider`. It captures exact source, revision, identity and language once. Tool inputs and `invocation_schema` describe domain fields directly. Each invocation injects the frozen source/revision into the unchanged provider envelope. It never reloads or normalizes source, refreshes the baseline on retry, mutates source, or saves.

Semantic editing and read-only investigation runners install this adapter and advertise the projected contract. Example:

```json
{"type":"tool","name":"language.example.describe","arguments":{"name":"mystery"}}
```

Raw provider decisions contain `expected_source`, `expected_version`, and a nested `arguments` object. Scripted models adopting projected semantic runners must instead supply only domain arguments. There is deliberately no heuristic envelope detection: domain fields can themselves have those names. Existing direct consumers of raw `SemanticToolProvider::new(...)` retain the explicit-envelope contract; generic agent/edit-session APIs are not automatically projected. The terminal opt-in API is unchanged.

The underlying language operation request, provider contracts, semantic authorization callback, proposal acceptance and application APIs are unchanged. Discovery is not authorization. Frozen target identity/language are checked by the adapter; the underlying provider still checks exact source/revision and pre/post execution freshness and enforces scope and semantic policy. Unknown target/precondition overrides are rejected as invalid domain arguments, not interpreted as authority.

The strict parser now distinguishes:

| Failure | Category |
| --- | --- |
| Empty or truncated decision | `incomplete_response` |
| Invalid JSON, literal control/newline characters, trailing objects/text | `malformed_response` |
| Valid JSON with wrong tagged shape, fields, argument types, or empty content/name | `invalid_decision` |

Direct typed deserialization is retained, including duplicate-key and integer constraints. No permissive parser or semantic repair was added. Correction feedback includes the actual error, requests exactly one complete decision matching the system protocol, and explains escaping newlines, quotes and backslashes. Retry counts and terminal classifications are unchanged. Lifecycle continuation retains its existing bounded history policy; raw output remains in existing trace events rather than being manufactured into a corrected response.

## Lifecycle and trace integrity

Investigations remain read-only, retain evidence, and conclude only through host assessment; there is no proposal authority. Editing still requires a permitted proposal and separate review/acceptance/application. Completion cannot satisfy a missing proposal or bypass review. Repeated-call suppression is unchanged.

Existing protocol trace events retain raw model output, accepted/rejected decision kind and categorized error, feedback, tool results and lifecycle outcomes. Existing lifecycle invocation records retain the source snapshot, model call and structured provider result; host baseline fields are no longer model-authored call arguments. Syntax/schema rejection occurs before dispatch; lifecycle rejection and structured provider statuses (including `revision_conflict`) remain distinct. No credentials or new logging framework were introduced.

## Regression coverage

The semantic provider, investigation, editing lifecycle, and decision-loop tests
cover exact trailing-LF/CRLF source binding; changed source, version, identity and
language rejection; authorization and override denial; retained pending proposals
without application; malformed literal-newline correction and bounded exhaustion;
schema rejection; raw trace preservation; and separate review/save authority.
These are descriptions of checked-in coverage, not a new validation run.

## Limitations and adoption

No provider-specific structured-output request was added. The current Cell adapter exposes text content through `DecisionModel`; OpenAI-compatible transport alone does not establish JSON-schema support. Portable strict text parsing plus bounded correction remains the fallback. Structured output could be an explicit future transport capability, not assumed for LM Studio.

The projected adapter rejects providers registering `Number` or `Null` argument kinds because the existing tool input enum cannot describe them faithfully. This includes unselected operations on that provider. Such downstream services must retain raw provider calls or resolve this representational limitation before adopting the projected semantic runners. No numeric coercion was introduced.

## Embedding adoption

1. Use the semantic investigation/editing runners when those lifecycle contracts fit; they project arguments automatically.
2. Remove precondition-copy instructions and update scripted model fixtures to put only domain fields in decision `arguments`; use the advertised invocation schema. Keep supplying the same authorized host target, operation allowlists, semantic policy and completion assessment.
3. For custom `AgentSession` assembly, explicitly call `SemanticToolProvider::new(...)?.with_projected_arguments()?` before installation. Bind it when the authorized baseline is established, not after a failed invocation to refresh stale source.
4. Preserve raw provider envelope calls for integrations deliberately using that public contract. Do not mix envelope and projected schemas.
5. Keep existing budgets, evidence assessment, trace capture, proposal acceptance and save policy unchanged.
