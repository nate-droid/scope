# AI2 — Protocol robustness and host-owned preconditions

## Findings

The supplied FW5b observations match the inspected implementation. The external FW5b JSONL artifacts are not in this checkout; the failures were reproduced deterministically rather than claimed as independently replayed live trials.

- `tool/semantic.rs` introduces `expected_source` and decimal-string `expected_version` in the public provider's inputs and invocation schema. The provider checks exact source and revision before dispatch, then checks freshness and result identity after execution. This explicit-envelope provider contract is appropriate for host callers but need not be the model contract.
- Previously, `edit_lifecycle.rs` advertised that envelope in its compact catalog and system instructions, placed the baseline in `edit_request`, and required the model to copy it. Losing a trailing newline therefore produced a genuine envelope mismatch even without a changed target. Trimming would hide conflicts, not fix ownership.
- `cli/agent_semantics.rs` already has an explicitly selected, private domain-argument projection for terminal agents. It demonstrates the existing separation but was unavailable to downstream semantic lifecycle callers.
- `CellAgent::decide` forwards messages/token limits and returns response content unchanged. Shared `agent_loop.rs` strictly deserializes one tagged `tool` or `complete` decision. A completion becomes typed only after deserialization and nonempty-content validation; lifecycle policy then decides whether it can conclude, advance inspection to proposal, or reject completion. No prose fragments are conclusions.
- Literal newlines inside JSON strings are invalid JSON. The original parser already rejected them, but schema/data errors were also labeled malformed responses. Existing correction feedback requested valid protocol without specifically explaining JSON escaping.

## Changes and contracts

`SemanticToolProvider::with_projected_arguments()` is an additive public opt-in returning a `ToolProvider`. It captures exact source, revision, identity and language once. Tool inputs and `invocation_schema` describe domain fields directly. Each invocation injects the frozen source/revision into the unchanged provider envelope. It never reloads or normalizes source, refreshes the baseline on retry, mutates source, or saves.

Both semantic editing and AI1 investigation runners now install this adapter and advertise the projected contract. Example:

```json
{"type":"tool","name":"language.example.describe","arguments":{"name":"mystery"}}
```

Previously the decision's `arguments` contained `expected_source`, `expected_version`, and a nested `arguments` object. Scripted models using these semantic runners must migrate. There is deliberately no heuristic envelope detection: domain fields can themselves have those names. Existing direct consumers of raw `SemanticToolProvider::new(...)` retain the explicit-envelope contract; generic agent/edit-session APIs are not automatically projected. The terminal opt-in API is unchanged.

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

## Deterministic validation

Added/updated tests cover:

- Domain-only calls against exact trailing-LF and CRLF snapshots; exact source/revision delivered to the language; no whitespace or Unicode normalization.
- Changed source, revision, identity and language rejection; unauthorized operations and target/precondition override rejection.
- Multiline investigation completion and semantic editing with exact host source binding, retained pending proposal preview, and unchanged active source.
- Literal-newline malformed completion, precise correction, subsequent correctly escaped completion, bounded exhaustion, schema rejection, and exact raw-response trace retention.
- Existing candidate evaluation, guided refinement, proposal review/application separation, stale proposal rejection, no-save behavior, authorization and AI1 investigation tests.

Validation results:

- `cargo check -p scope-ide --features agent --lib`: passed.
- Default library suite: 361 passed on rerun (an intermittent workspace/mount test failure occurred during earlier broad runs).
- Agent-enabled semantic tests in an isolated copy: 29 passed.
- Full `cargo test -p scope-ide --features agent --lib` in an isolated copy: **533 passed, 0 failed** after migrating scripted fixtures.
- Formatting and `git diff --check`: passed.

The normal agent-enabled worktree test build is blocked by an existing `E0061` at `cli/source_proposals/configured_tests.rs`: a `run_shell_configured` test call lacks the seventh `host_command` argument. Only the isolated validation copy received the missing final `None`; that unrelated baseline defect was not changed in the worktree. Initial isolated runs exposed migration/test expectation failures; those caused by AI2 were fixed before the final passing run.

## Limitations and adoption

No provider-specific structured-output request was added. The current Cell adapter exposes text content through `DecisionModel`; OpenAI-compatible transport alone does not establish JSON-schema support. Portable strict text parsing plus bounded correction remains the fallback. Structured output could be an explicit future transport capability, not assumed for LM Studio.

The projected adapter rejects providers registering `Number` or `Null` argument kinds because the existing tool input enum cannot describe them faithfully. This includes unselected operations on that provider. Such downstream services must retain raw provider calls or resolve this representational limitation before adopting the projected semantic runners. No numeric coercion was introduced.

Minimum Scope Forth adoption:

1. Update the Scope dependency and use the existing semantic investigation/editing runners; they now project arguments automatically.
2. Remove precondition-copy instructions and update scripted model fixtures to put only domain fields in decision `arguments`; use the advertised invocation schema. Keep supplying the same authorized host target, operation allowlists, semantic policy and completion assessment.
3. For custom `AgentSession` assembly, explicitly call `SemanticToolProvider::new(...)?.with_projected_arguments()?` before installation. Bind it when the authorized baseline is established, not after a failed invocation to refresh stale source.
4. Preserve raw provider envelope calls for integrations deliberately using that public contract. Do not mix envelope and projected schemas.
5. Keep existing budgets, evidence assessment, trace capture, proposal acceptance and save policy unchanged.

Next controlled trial: rerun the same four FW5b cases against the same verified endpoint, model, fixtures, settings and budgets. Record exact raw decisions, correction categories, frozen baseline and structured results. Expect source-transcription conflicts to disappear on unchanged targets, not genuine conflicts; measure valid conclusions without assuming they will improve. Retain repeated-inspection suppression and report duplicate stops separately. No Path, Scope Forth or Cell changes are included here.
