# CE2: standalone downstream Forth workflow

This crate owns only `examples/ce2-forth`. It uses the production generic
`AgentWorkflow`, `AgentWorkflowContext::invoke`, and
`run_session_with_workflows` APIs without production or other downstream changes.
Dependencies and initial lockfile follow CE1.

## Run

From this directory:

```sh
cargo test --offline
cargo run --offline --quiet > deterministic.json
cargo run --quiet -- --live MODEL BASE_URL > live.json
```

The optional live adapter uses `CellAgent` and reads `OPENAI_API_KEY` if present.
Live mode sends the task, source, catalog and results to your endpoint; it can
incur inference costs. It was **not** run for the checked-in report. Neither
credentials nor authorization headers are recorded. Model request messages are retained
at each decision, so review reports before sharing them. This is not a terminal
feedback log: feedback produced after the last decision (for example on a budget
stop) is not captured in these request snapshots.

Each mode performs three paired trials, ordered baseline/workflow,
workflow/baseline, baseline/workflow. This alternates order but cannot perfectly
balance first position with an odd number of pairs. Each trial starts a fresh
scratch target, provider, session, model conversation and workflow.

## What is compared

- Baseline: ordinary `run_session`, making four separate semantic calls.
- Workflow: `run_session_with_workflows`, adding one fixed host-defined read tool.
  Its host-authored Script runs through `Interpreter`; a custom `Host` forwards
  every inner call through `context.invoke`, **not** a parallel provider session.
- Both use exactly the same base semantic catalog, read grants, task, source,
  projected arguments, transport limits and policy. The workflow arm necessarily
  adds its workflow definition. Direct semantic tools remain available there;
  live models are not forced to select the workflow. Mixed/direct calls are
  retained and assessed too.
- The existing `SemanticToolProvider::with_projected_arguments()` captures source,
  revision, identity and language. Model arguments cannot replace that binding.
  Only `words` and `describe` are installed in the agent session. No edit, save,
  proposal, filesystem or shell tools are installed.
- Outer workflow attempts and inner attempts share the same call/byte budgets.
  Inner policy denial and the API's additional read-only gate remain effective.
  The Script is fixed; agents cannot supply arbitrary Script source.

The source deliberately defines related behavior:

```forth
: square dup * ;
: fourth square square ;
: misleading-fourth square dup ;
```

The host explicitly selects `square` as a related definition from the source.
**There is no dependency-graph operation in this experiment.** This is a documented
selection gap, not automated graph traversal. Calls are `words(document_only:
true)` and three `describe(name: ..., symbolic_inputs: 1)` inspections.

## Evidence and projection

The Script actually projects names, statuses and symbolic stacks. The host adds
the actual target snapshot binding (identity, language, revision and text), inventory, limitations and local `evidence_index` /
`attempt_indexes` references. `workflow_projections` preserves each invocation,
including repeated-call index offsets. The final workflow projection is also
returned as model tool feedback. The baseline receives raw semantic results;
its projection is produced only for report comparison, not fed to its model.

The retained `evidence` array is host-local report data, **not a new durable
evidence store or reference protocol**. Each entry includes full projected call
arguments, response and/or error. Projected arguments exclude the host-injected
source/revision preconditions; their actual snapshot binding is retained separately
as `source_binding` and `projection.source`, including the host target identity.
Provider envelopes retain their full source
metadata, symbolic nodes and arithmetic obligations. Inner calls denied before
provider dispatch and budget failures are retained by the forwarding Host. If a
provider returned data but the context rejected its byte size, the retained
response is accompanied by `context_error`; it is not successful delivered
workflow evidence. Byte limits are checked after provider return: they are not
allocation or retention guarantees, and an oversized result may already have been
allocated and retained locally. Baseline pre-dispatch failures appear in the
conversation only when included in a subsequent model decision request. Terminal
feedback on budget stops is not captured by request snapshots. There is no claim
that an unexecuted call returned data.

`InvestigationEvidence` belongs to the edit/investigation lifecycle, which this
generic-session experiment does not enter. `ToolExecutionEvent` is payload-free,
and the workflow context currently exposes no observer hook. Therefore this
report retains actual call results directly; it does not fabricate an execution
event stream, call IDs, or durable citations. Local references resolve only
against the containing run's retained `evidence` array.

All targets are immutable borrows; every run checks text, revision and dirty
state afterward. Semantic completion checks inventory plus all three found,
revision-0, final, `constructed-conditional` stack observations. Expected stack
node indexes are fourth `[2]`, square `[1]`, misleading-fourth `[1,1]`. These
are **symbolic node indexes**, not numeric stack values. This checks observation
coverage, not final-answer prose quality, arithmetic equivalence or universal
correctness. Provider limitations and not-proved obligations still apply.

## Checked-in deterministic measurements

`deterministic.json` contains six runs and model request snapshots at each decision. All six
complete with the expected observations and unchanged source.

| Per trial | Baseline | Workflow |
| --- | ---: | ---: |
| Model decisions, including completion | 5 | 2 |
| Charged tool calls | 4 | 5 |
| Successful semantic observations | 4 | 4 |
| Sum of model message-content UTF-8 bytes | 85,483 | 24,113 |

The workflow removes three prescribed model turns (60%) and about 71.8% of
cumulative message-content bytes, while adding one charged outer call. These
are fixed `DecisionModel` replay sequences, **not evidence of an LLM choosing
better tools or answering more accurately**. Bytes exclude role/framing and
are not tokens, wire sizes or inference costs. Token usage is unavailable.
Timing is retained in microseconds but includes local overhead and is not a
causal latency benchmark; in this replay the interpreter adds local work.
Live runs would need separately assessed answer quality, provider/model settings,
more trials, latency and actual token accounting before making broader claims.

Seven offline tests cover paired evidence/catalog equality, exact success and
local-reference and agent-visible target-identity expectations, typed-invalid arguments, outer and inner denial,
shared call-budget partial failure, message/conversation budgets, repeated and
mixed calls, projected source/revision override rejection, and unchanged source.
An existing `scope-ide` dead-code warning is unrelated to this crate.
