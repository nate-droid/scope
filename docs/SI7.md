# SI7 — retained scratch semantic agent composition

## Baseline and revision history

The implementation baseline is `d73879c0afb386263415ea02909176d0612bb03d`.
The main agent personally observed a **clean initial Git status** before this task's
implementation changes.

`git diff --stat a91e23d..d73879c` showed changes only to Cargo example configuration,
`editable_workspace`, TUI composition/editor/session/pane code, scope-terminal
documentation/library code, and `docs/design/S3_RESULTS.md`. There were no changes
to `cli.rs`, `scratch.rs`, or agent semantic code between those revisions: the
relevant boundary APIs were unchanged. The old audit's working-tree state cannot
be reconstructed from that committed comparison, so this is not an exhaustive
account of every change since the old audit.

## Six original observations — confirmed API boundaries

All six original observations were confirmed at the clean baseline. SI7 adds the
explicit integration described in the final column; it does not retroactively
attribute that integration to the baseline.

| Original observation at baseline | Confirming API / implementation path | Added by SI7 |
| --- | --- | --- |
| The normal `run_cli_agent` path reaches the terminal runner with semantic configuration `None`. | `run_cli_agent` → `run_cli_agent_model` → `run_terminal_agent_model(..., None)` in `src/cli.rs`. | An opted-in branch for `:scope agent` before the unchanged ordinary path. |
| That normal path supplies no retained scratch, semantic provider, or semantic authorization callback. | The same `src/cli.rs` call chain; `TerminalSemanticConfig` in `src/cli/agent_semantics.rs` is not supplied there. | A host binding supplies the service, allowlist, per-call authorization, and retained-source runner. |
| The public terminal semantic runner is App-oriented, not a retained-scratch runner. | `scope_ide::cli::{run_terminal_agent_model, run_terminal_agent_model_with_projected_arguments}` accept `&mut App` and optional `InteractiveSession` in `src/cli/agent_semantics.rs`. | `scope_ide::cli::run_retained_scratch_agent` accepts the original `&ScratchBuffer`. |
| `semantic_target` is independent of active scratch selection. | `scope_ide::scratch::ScratchBuffers::{bind_semantic_target, semantic_target_id, semantic_target}` in `src/scratch.rs`. | The integrated agent explicitly resolves this binding, without changing selection semantics. |
| Borrowed launchers retain caller-owned sources but have no retained semantic-agent hook at baseline. | `scope_ide::cli::{run_borrowed_with_sources, run_terminal_borrowed_with_sources}` accept `&mut ScratchBuffers` in `src/cli.rs`; baseline `ReadOnlyCommand` has no `semantic_agent` method. | `ReadOnlyCommand::semantic_agent` returns `SemanticAgentBinding` through the shared shell. |
| Enabling feature `agent` alone is insufficient to connect retained scratch semantics. | Feature-gated normal dispatch in `src/cli.rs` still takes the ordinary path without a host opt-in. | The host must return `Some(binding)`, bind a live target, explicitly allow operations, and authorize calls; the feature alone still grants none of that authority. |

Here and below, `src/...` paths in API tables are relative to
`crates/scope-ide`.

## Opt-in contract and launcher composition

With feature `agent`, `ReadOnlyCommand::semantic_agent(&mut self)` defaults to
`None`. Returning `Some(SemanticAgentBinding<'_>)` opts that invocation of the normal
`:scope agent REQUEST` command into the retained semantic path. The binding carries
`service`, `allowed_operation_ids`, `authorize`, `limits`, optional `trace`, optional
`model`, and `observe`. It is borrowed per invocation, not installed as permanent
authority.

`run_borrowed_with_sources` accepts `&mut App`, `&mut InteractiveSession`, an optional
host command, and `&mut ScratchBuffers`, plus input/output streams.
`run_terminal_borrowed_with_sources` provides the terminal equivalent. Sources,
App, and session remain caller-owned across shell handoffs. The normal
`run_with_registry_and_command(args, registry, command)` launcher also reaches this
hook through the shared shell implementation; it does not require a separate agent
launcher. Launcher-owned scratch state and caller-retained scratch state are
ownership alternatives, not two competing semantic dispatchers.

An opted-in invocation resolves only `ScratchBuffers::semantic_target()`. Missing,
closed, or wrong-language targets fail before model invocation, with no fallback
to active scratch, App/editor document, or disk. Closing the bound target leaves a
closed identity, not permission to retarget. Clearing the binding does not opt the
host out of the semantic contract. By contrast, an absent host or a host returning
`None` preserves the ordinary agent path and does not require a semantic target.

`SemanticAgentBinding::model = Some(...)` supplies a host model, useful for
scripted tests or another trusted implementation. **`None` uses the normal shell
model and its configured endpoint** through `CellAgent::new(agent_config.clone())`;
it does not disable the agent or force offline execution. The public borrowed
launchers currently supply their normal default local model/base URL configuration;
the argument-based launcher supplies its ordinary configuration. Authorized source
and result content may reach that model endpoint.

## Authority, source identity, and read-only meaning

`scope_ide::cli::run_retained_scratch_agent` in
`crates/scope-ide/src/cli/retained_agent.rs` accepts the original `&ScratchBuffer`,
service, model, request, limits, and `RetainedScratchAgentConfig`. Eligibility is the
intersection of:

1. An explicit host operation-ID allowlist (empty grants nothing).
2. `SemanticOperationEffect::Read`.
3. `SemanticTargetRequirement::Document`.

Eligible calls still require the host's **per-call** `SemanticAuthorization`
callback. Registration, discovery, model output, and the `Read` label do not grant
authority. Invalid arguments are rejected before semantic dispatch; hidden tools
are excluded from both discovery and dispatch. `with_projected_arguments()` exposes
language-owned arguments while the provider supplies source preconditions and
validates arguments/results.

The immutable invocation borrow pins the original identity, revision, language,
and exact unsaved text. It is not a clone relabelled as another authoritative
document. The authorization wrapper also checks those snapshot fields against the
original binding. Safe host editing can resume after the invocation; the next run
gets a fresh snapshot and a fresh conversation, not the prior run's evidence.

A candidate evaluation declared `Read` can be eligible, but it is still separately
authorized. Candidate text is disposable input, never installed in the retained
source. `Read` constrains declared persistent effects, not computational cost.
Native language services and models remain trusted host code, not sandboxed code.
Agent decision/tool/conversation budgets are reused; language-specific execution
fuel, memory, recursion, cancellation, wall-clock deadlines, and other computational
limits remain downstream/host responsibilities. A synchronous callback is not
preempted by this adapter.

Only the filtered semantic provider is installed in this integrated agent session.
There are no generic mutation tools, generic read/workspace tools, proposal/apply
capabilities, or save access supplied to it. The internal `SourceEditController`
and document-scoped provider setup do not grant editing authority. Ordinary explicit
shell commands, including their existing edit/save behavior, are unchanged. This
is a narrow agent capability boundary, not a claim that the whole shell or trusted
native callbacks have become sandboxed.

## Outcomes, attribution, tracing, and failures

`RetainedScratchAgentRun` contains the unchanged `AgentRun`, a borrowed
`SourceSnapshot`, `trace_jsonl`, and `trace_truncated`. On a successful loop return,
`SemanticAgentBinding::observe` is called once, including budget and correction
stops. The shell prints the stop reason and content; `Completed` is not implied by
an `Ok` return, nor is model completion independent semantic verification.

The host must **copy desired attribution during `observe`**: source ID, revision,
language, exact text, outcome/counts, and trace bytes as needed. The shell neither
retains this invocation-local borrow nor creates a durable audit archive for the
host. Copied evidence stays attributed to its original revision after subsequent
edits; it must not be relabelled as current evidence.

Tracing uses the existing `ProtocolTrace`, beginning with a `source_binding` event
and continuing with protocol requests, decisions, feedback, and outcome as
available. Capture is streamed and bounded by `AgentLimits::max_conversation_bytes`.
This is a sensitive protocol record: source, arguments, model output, and semantic
results may occur in it. It is not byte-exact HTTP wire capture and is never printed
automatically by the shell.

At the byte cap, the last JSONL record may be incomplete (and arbitrary byte
truncation need not preserve UTF-8). Consumers must handle partial records rather
than assuming every line parses. `trace_truncated` also indicates sink failure;
`ProtocolTrace` latches its first write error and stops tracing without replacing
the agent outcome. The optional external sink receives the bounded stream as it is
written; on sink failure it may contain only a prefix, not all internally captured
bytes.

Target/setup/model errors return through the shell's error path: **`observe` is not
called** when the runner returns `Err`. In particular, model failure does not return
a `RetainedScratchAgentRun`, so its internal capture is not handed to the observer.
A configured external trace sink can preserve events written before that failure.
Sink persistence, sensitive-data handling, and failure diagnostics belong to the
host. This is not a guarantee of a complete trace on every error.

## Runnable offline example

From the workspace root:

```sh
cargo run -p scope-ide --features agent --example retained_semantic_agent --offline
```

`crates/scope-ide/examples/retained_semantic_agent.rs` supplies a minimal Notes
language service, explicit `inspect` allowlist/authorization, and a scripted
`DecisionModel`. It drives the ordinary stream shell through
`run_borrowed_with_sources`, creates unsaved `café 🦀` text, explicitly binds scratch
0, and invokes `:scope agent`. The observer reports `Scratch(0)`, revision 1, and
`Completed`; the caller can still inspect its retained source after shell exit.
No endpoint, credentials, Hilb runtime, or source-file writes are required. Cargo's
`--offline` requires dependencies already available locally; it is not itself a
runtime network sandbox for arbitrary models.

## Coverage matrix and validation

The shell cases below are in
`crates/scope-ide/src/cli/retained_agent_shell_tests.rs`; prefixes identify exact
test names without repeating their full descriptive suffixes.

| Contract | Coverage |
| --- | --- |
| Exact unsaved Unicode/CRLF text, original attribution, unchanged dirty state/App/disk | `a_live_unsaved_source_has_exact_host_attribution_and_no_mutation` |
| Independent target, caller ownership, fresh revision across handoffs | `b_binding_is_independent_of_selection_and_survives_shell_handoffs` |
| Missing/closed/wrong-language target rejects before model or authorization | `c_absent_closed_and_wrong_language_targets_fail_before_model` |
| Candidate Read needs authorization and cannot install text | `d_candidate_read_still_requires_authorization_and_is_not_installed` |
| Hidden edit/selection and generic tools cannot dispatch | `e_hidden_and_generic_tools_cannot_dispatch_or_authorize` |
| Invalid arguments feed back and recover without invalid service dispatch | `f_invalid_arguments_recover_without_semantic_dispatch` |
| Empty allowlist has no generic fallback | `g_empty_allowlist_never_falls_back_to_generic_tools` |
| Budget stop reaches observer with original counts, not fabricated completion | `h_budget_stop_is_observed_as_a_stop_not_completion` |
| Model transport error skips observer and preserves source owner | `i_transport_failure_reports_error_and_keeps_source_and_owner` |
| Malformed decision correction followed by valid dispatch | `j_normal_shell_corrects_malformed_decision_before_dispatch` |
| Repeated malformed decisions stop without dispatch | `k_normal_shell_stops_malformed_no_progress_without_dispatch` (`MalformedBudget`) |
| Copied old evidence retains original revision; next call is fresh | `l_saved_evidence_keeps_its_revision_and_next_run_gets_fresh_evidence` |
| Absent/opt-out host retains normal model path | `m_opt_out_and_absent_hosts_use_normal_model_without_requiring_semantic_target` |
| Trace event forwarding and bounded capture; eligibility and hidden dispatch wrapper | Four unit tests in `src/cli/retained_agent.rs` |

Validation passed for this task:

```sh
cargo test -p scope-ide --features agent --lib retained_agent --offline
cargo test -p scope-ide --features agent --offline
cargo check -p scope-ide --offline
cargo fmt --all -- --check
git diff --check
cargo run -p scope-ide --features agent --example retained_semantic_agent --offline
```

The focused test command passed **17 tests, 0 failed, 0 ignored**, with 547 filtered
out. The full `scope-ide` agent-feature test command passed all crate unit,
integration, and doc tests, including **564 unit tests (17 new)**. Default-feature
`cargo check`, formatting, and diff checks passed. The example exited successfully
and showed the expected retained attribution and semantic result. This does not
claim a full workspace test run. The configured endpoint path was inspected, not
validated against a live model server.

The baseline already had a test-call compile issue in
`crates/scope-ide/src/cli/source_proposals/configured_tests.rs`: a call omitted the
final host-command `None`. The fix made in this task adds only that missing `None`,
solely to compile validation. It is not part of retained-agent behavior.

## Limits and deferred work

- One explicit retained scratch target per scratch owner; no multi-target routing,
  automatic retargeting, cross-owner identity lookup, or replacement-target search.
  Future multi-target support needs an explicit host target set, attribution per
  target, and separate authorization for each target; it must not implicitly retarget.
- No candidate editing/installation, proposal approval, apply/save workflow, or
  automatic source persistence in the integrated agent.
- No new Cell implementation or protocol changes; the normal `DecisionModel`
  abstraction and configured endpoint composition are reused.
- No new downstream language semantics, evaluator sandbox, preemptive execution
  limits, durable evidence store, or complete-on-error trace guarantee.
- No host evidence-acceptance policy equivalent to the separate semantic
  investigation API is added here. Hosts must inspect stop reasons and actual
  evidence rather than treating fluent completion text as verified correctness.
