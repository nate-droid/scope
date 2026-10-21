# S1 — Host-owned semantic invocation context

## Decision and scope

**Recommendation: proceed to a small, explicitly opt-in terminal-adapter milestone.** Model-authored source preconditions are redundant in the existing App-document-bound terminal run. The host can insert the original exact text and version without bypassing the current provider, language argument validator, authorization callback, or freshness checks.

This investigation changes no production API, operation contract, service signature, registry, or tool schema. A fixture-specific, test-only projection demonstrates the seam through the real decision loop. It is not a production implementation or a generic agent protocol. No remote model or downstream live proof was run.

## Current invocation anatomy

Relevant implementation:

- `crates/scope-ide/src/cli/agent_semantics.rs`: `TerminalSemanticConfig`, `run_terminal_agent_model`, eligibility filtering, private `Projection`.
- `crates/scope-ide/src/tool/semantic.rs`: `SemanticToolProvider`, envelope schema, invocation validation, semantic authorization and pre/post checks.
- `crates/scope-ide/src/source_edit.rs`: `SourceTarget`, `SourceSnapshot`, `DocumentSource`, document scope.
- `crates/scope-language/src/semantic.rs`: language argument validation and validated dispatch.
- `crates/scope-ide/src/semantic_inspection.rs`: existing host-bound T1 inspection binding.
- `crates/scope-ide/src/tool.rs`: session discovery, policy and provider dispatch.

| Layer | Inputs/ownership | Enforcement |
| --- | --- | --- |
| Model-facing action | Tool name `language.<language-id>.<operation-id>`; `expected_version` string, exact `expected_source`, nested `arguments` object | Agent parses the existing single-action protocol; the session does not generically enforce semantic JSON schemas |
| Terminal composition | Host config contains language-local allowlist and semantic callback | Only allowlisted Read/Document operations; filters both discovery and dispatch |
| Host target | App's explicitly open document and matching registered service | Missing document/service fails closed; no highlighted-file, filesystem or scratch fallback |
| Captured snapshot | Tagged document identity, existing version, exact unsaved text, language | Captured once before the run; terminal policy compares all fields and cached operation descriptor |
| Provider invocation | Full source-bound envelope plus language arguments | Unknown envelope keys, absent/malformed preconditions and invalid language arguments rejected |
| Semantic authorization | Operation descriptor, current source snapshot, validated language arguments | Document scope plus captured-source policy, then caller callback; discovery is not permission |
| Language dispatch | Existing `SemanticOperationRequest` with document, selection, operation ID and language arguments | Existing dispatcher validates registered operation arguments/target and prevents Read from returning changes |

**Source identity and document path are not currently model-supplied envelope fields.** The model supplies text/version and a tool name; identity, service, path context and permitted scope already come from the host. Removing text/version from model arguments does not remove identity enforcement.

### Ownership and lifetime

`DocumentSource` holds `&mut Document` and a host-selected language ID, forwarding the owner's identity, text and revision. The provider borrows that adapter and the selected `&dyn LanguageService`. The synchronous terminal run retains these borrows. Normal safe App mutation, reopening, or selection replacement cannot occur through App during the run. A snapshot is borrowed observation, not a second editable document or disk reload.

T2a distinguishes App document and shell buffer ownership even for identical paths. Their identities/revisions cannot be substituted. T2b selects only App's open document; it does not consume a shell scratch selection or infer source authority from an evaluator session. Reopening the same path between runs creates a different document identity. Path is language/display context, not authority.

### Existing validation order

1. Session resolves an advertised tool and applies `read_only_policy`.
2. Provider clears prior native observation and resolves the exact tool name.
3. Provider rejects unknown envelope fields; requires a string version parseable as `u64`, string source, and `arguments`.
4. Cached operation's language arguments are validated: object, required fields, unknown keys and declared types. Nested opaque constraints remain language-owned.
5. Provider snapshots its live target; requires `PermittedEditScope::Document(snapshot.id)`.
6. Expected version/text must equal the live snapshot (`revision_conflict`).
7. Terminal-bound policy requires original identity/version/text/language and operation descriptor; host callback must additionally approve (`unauthorized` otherwise).
8. Selection, if supplied, is range-validated. Terminal semantics supplies none.
9. Existing language dispatcher resolves the registered operation again, validates arguments/target, and invokes the language implementation.
10. Provider checks live identity/version/text/language against the invocation snapshot after execution, including execution errors, before publishing its result.

Read effects cannot return source-change proposals. Terminal mode installs no generic document/workspace write, acceptance, shell, filesystem or proposal tools. The controller is isolated. Source changes are not applied by a read result.

## Trust boundaries and guarantee limits

The model controls operation selection and language arguments, not the selected source or policy. Two separate gates remain: agent session policy and semantic host policy. Candidate evaluation can declare Read while still requiring an explicit host decision about execution authority. Native provider declarations are trusted in-process contracts, not an OS sandbox.

The existing service identity guarantee is **the same borrowed service object for the run**, not a durable registration-generation token. Do not invent such a token for S1. Likewise the terminal policy binds the cached operation description, but the language dispatcher rereads the catalog; it does not compare that later description to the cached authorized description. Stateful catalogs are an existing uncertainty, not something this projection fixes.

Pre-dispatch text/version checks do not independently compare language to the service on every call; construction checks the language, terminal policy compares captured language, and post-execution checks target language. A policy callback with access to interior-mutable test state can change state during authorization; the existing postcheck rejects stale output, but there is no new check immediately after that callback. None of these guarantees should be overstated as process isolation or transactional execution of malicious native services.

General proposal/candidate/contract validation has additional paths and checks. Those are not exposed or redesigned here. In particular this read-only investigation does not claim uniform service/language freshness at every proposal-validation or acceptance boundary.

## Smallest proposed projection

Extend the existing private terminal provider adapter, behind a **new explicit host opt-in**, to advertise only the language-defined arguments. Keep existing terminal behavior and direct provider envelopes available unchanged. A separate entry point/config wrapper can preserve current `TerminalSemanticConfig` struct-literal callers; the precise public API is deferred to the implementation milestone.

For each eligible operation:

- Keep the registered namespaced tool ID, description, effect, target, purpose, constraints, evaluation hints and output description.
- Derive tool input definitions from its registered language argument definitions.
- Project `invocation_schema` to its `arguments` object (required fields/types and `additionalProperties: false` retained).
- Treat the entire model action argument map as language arguments, without merging it with host fields.
- Build a fresh internal map containing only `expected_version = original.version.to_string()`, `expected_source = original.text`, and `arguments = model_argument_map`.
- Invoke the existing `SemanticToolProvider` normally. Do not call `perform_semantic_operation` directly, skip provider checks, or refresh the baseline from the live target.

The original snapshot remains the precondition for every call. A changed identity fails scope/bound policy; a changed version/text fails provider preconditions. No source is silently rebound. Source-field-looking keys cannot override the envelope: in the fixture they are rejected as unknown language arguments. If a real language explicitly defines an argument named `expected_source`, it remains only a nested language argument and grants no binding authority.

### Placement comparison

| Location | Assessment |
| --- | --- |
| Terminal provider adapter | **Preferred.** Already owns the run binding, filtered catalog and host policy; smallest change and no downstream contract effects |
| General `SemanticToolProvider` wrapper | Technically possible as a provider decorator, but generalizing before a second consumer adds unnecessary policy/schema surface; do not globally weaken its envelope |
| Existing host-bound `InspectionBinding` | Useful precedent: owns original snapshot and internally builds an envelope. Intentionally Read/Document and argument-free, with human T1 semantics; extending it would conflate T1 confirmation with agent authorization and require widening its contract |

No replacement registry or language-service change is needed.

### Feedback is part of the projection

A production implementation must project **both** advertised definitions and error guidance. `invalid_invocation` currently returns the full provider schema and instructions to preserve source preconditions. Forwarding that verbatim would reintroduce the model bookkeeping on retries. Preserve status, diagnostics and outcome meaning, but supply projected schema/retry guidance, never ask the model to reconstruct host fields. This test prototype deliberately leaves that production feedback adaptation unimplemented.

## Deterministic prototype

Added only to `crates/scope-ide/src/cli/agent_semantics_tests.rs`:

- `LanguageArguments<P>`: test-only fixture projection wrapping the actual provider; not a general registry or public adapter. Tool input definitions are deliberately fixture-specific; metadata schema is projected from provider metadata.
- `run_projected`: actual `AgentSession` composition and `agent::run_session` with `read_only_policy`, real provider, captured snapshot-bound semantic policy and isolated controller.
- `projected_language_arguments_preserve_host_authority_and_source`.
- `projected_language_arguments_stale_identity_and_revision_fail_closed`.

The first test uses the existing registered `terminal-fixture` service and App `DocumentSource`. Model decisions contain only `{}` or `{ "candidate": "candidate λ\n" }`. Successful execution and fixture events demonstrate that the real provider accepted a complete internal envelope and delivered exact original UTF-8 text **including final newline**, revision and arguments. The callback asserts original identity, language and operation metadata. Discovery contains only two Read tools with projected schemas and retains CandidateEvaluation purpose.

It also tests host denial of an advertised candidate, wrong argument type, missing required argument, unknown argument, forged envelope/identity-looking keys, and hidden edit dispatch. Invalid/denied calls do not reach the service. The document (including identity/version/text) and disk remain unchanged; no edit/proposal tool is advertised.

The second test uses a deliberately interior-mutable `SourceTarget` and deterministic model to change identity or revision after catalog capture, before invocation. Both fail closed before caller authorization/language dispatch. Its mutation method panics if called. This simulates transitions that the normal exclusive document borrow prevents; it does not add a document mutation mechanism.

Limitations: no production opt-in entry point is tested because none is introduced. The tests compose the actual provider/session/loop rather than replacing semantic validation with a mock. They do not instrument the raw envelope at a separate spy layer; acceptance by the real required-envelope validator, exact fixture events and stale-precondition failure establish the reconstruction. This prototype does not claim exhaustive adversarial service coverage or test projected retry guidance/result labels.

### Existing regression evidence inspected

- T2b: `cli/agent_semantics_tests.rs` covers discovery, unsaved text, denied policy, malformed arguments, stale source/version, missing service/document, malformed catalogs, budget/transport stops, reopened identity and conflicting highlight.
- Provider: `tool/semantic_tests.rs` covers envelope validation, scope/policy denial, selection restrictions and Read change rejection. `tool/semantic.rs` includes `native_observation_requires_post_execution_freshness` and native-output handling tests.
- T1: `semantic_inspection.rs` tests binding staleness for identity/text/revision/language, operation removal/description changes, authorization and invalid output identities. CLI/shell integration coverage is in `tests/shell_semantic_inspection.rs`; interactive coverage is in `tests/interactive_semantic_inspection.rs`.
- T2a: `source_context.rs` and CLI context tests cover explicit owner resolution, same-path distinct sources and observational routing; the design records why context discovery supplies no authority.

These existing tests are evidence of coverage, not all rerun results for S1.

## Before/after usability examples

Illustrative actions using the registered fixture; revision `1` is only an example. The old original source is `disk unsaved é\n`.

| Case | Current model action arguments | Host-bound projection arguments | Outcome |
| --- | --- | --- | --- |
| Argument-free `language.terminal-fixture.inspect` | `{"expected_version":"1","expected_source":"disk unsaved é\n","arguments":{}}` | `{}` | Read-only observation of original snapshot |
| Parameterized `language.terminal-fixture.candidate` | `{"expected_version":"1","expected_source":"disk unsaved é\n","arguments":{"candidate":"candidate λ\n"}}` | `{"candidate":"candidate λ\n"}` | Existing structured domain result; candidate text is a genuine semantic argument, not original-source bookkeeping |
| Advertised candidate denied by host | Same full envelope plus `{"candidate":"denied"}` nested under `arguments` | `{"candidate":"denied"}` | `unauthorized`, no language dispatch; discovery grants nothing |
| Inspect after revision changes | Original full envelope with version `1` | `{}` | Host still inserts original version `1`; `revision_conflict`, no rebinding or dispatch |

An identity replacement can yield `unauthorized` via document scope rather than `revision_conflict`; preserve this existing distinction rather than promising a new universal stale status. Hidden edit operations remain unadvertised and denied.

The tool name already selects the registered operation. Argument-free reads go from three required envelope fields to zero; parameterized reads expose only actual language inputs. Source length no longer affects action size, and newline reproduction cannot invalidate an otherwise valid read. This is a material structural simplification, **not a measured claim about live model success rates**.

### Source awareness and result binding

Keep host-rendered selected-source context: owner `app-document`, tagged local identity, language, original revision, display path if appropriate, and that the snapshot is unsaved/read-only. These are observations, not model-supplied selectors. Source content may still be relevant task context, but must not be a required action argument or an identity proof. Existing context text should be reviewed for redundant full-source repetition in a production projection.

Production tool feedback should report a host-owned binding label alongside the unchanged native result: original owner/identity/revision/language and display-only path, clearly labeled as invocation baseline rather than current selection. Do not require the model to echo the label, hash, bytes or revision, and do not let native output impersonate host binding metadata. On stale/denied calls distinguish the attempted original baseline from a successfully observed result. The prototype does not add this label or change native result contracts.

## Compatibility and proposed milestone

Unchanged in this investigation: direct `SemanticToolProvider` callers and schemas; Script integration; TUI inspection; T1 human confirmation; language registry and `LanguageService` signatures; terminal config and default CLI routes; document identity/revision machinery; agent JSON parsing and budgets.

A follow-up milestone should:

1. Add an explicit host-bound terminal opt-in without breaking existing config struct literals or changing legacy envelope behavior.
2. Derive projected inputs from registered descriptors, retain metadata/constraints and filtered dispatch, and reconstruct only from the original run binding.
3. Adapt error schemas/retry instructions and add unambiguous host-owned result binding labels without altering domain results.
4. Move the fixture proof onto that opt-in production entry point. Retain final-newline/UTF-8, spoofing, wrong-type, denied, hidden edit, stale identity/version coverage; add explicit text/language and post-execution staleness regressions, descriptor/constraint preservation, no-proposal behavior and legacy-mode compatibility.
5. Decide how to document trusted stable catalogs/service reference lifetime; do not silently introduce service generations, new revisions or generic binding infrastructure.

Remaining uncertainties are production opt-in API shape, feedback-label format, stable-catalog assumptions, and usability under real models. Unsupported floating/large unsigned JSON transport remains the existing independent limitation. None requires relaxing source checks or running downstream experiments now.

## Validation

Deterministic command run for the prototype:

```sh
cargo test --offline -p scope-ide --features agent --lib cli::agent_semantics::agent_semantics_tests -- --nocapture
```

Result: **13 passed, 0 failed**, including both new tests. `cargo fmt --all -- --check` also passed. Cargo reported duplicate-package warnings. No network/model calls, source editing capability, downstream proof rerun, or broad unrelated cleanup was introduced.
