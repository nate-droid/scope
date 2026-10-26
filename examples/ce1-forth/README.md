# CE1 actual Forth discovery presentation baseline

Standalone workspace inside Scope; no downstream edits. Requires sibling
`../../../scope-forth` with its `agent` feature and the current Scope investigation
APIs. `Cargo.lock` pins external resolution; path dependencies remain dependent on
checkout contents (including any existing local edits). Target artifacts stay in
this example's `target/`.

From the Scope root:

```sh
cargo test --offline --locked --manifest-path examples/ce1-forth/Cargo.toml
cargo run --offline --locked --quiet --manifest-path examples/ce1-forth/Cargo.toml > examples/ce1-forth/deterministic.json
```

Offline requires cached dependencies. Default execution makes no model/network
calls: a fixed DecisionModel submits `words {document_only:true}`, then
`describe {name:"misleading-square",symbolic_inputs:1}`, then the same describe
for `square`, then completes. Both operations are in `initial_operations`;
there is no canonical-detail roundtrip. An empty discovery call with this fully
revealed inventory terminates with `discovery_exhausted`, so is deliberately not
included. Discovery-decision counts are therefore zero in this experiment.

Both arms use the exact same task, static source, revision, operation allowlist,
authorization callback, budgets and decisions. Only the investigation entry point
changes: detailed baseline versus compact discovery presentation. This measures
presentation overhead, **not LLM improvement**. Timing is nondeterministic and not
a performance benchmark. JSON retains full input messages, decisions, errors,
EditRun source/evidence/results/trace, counts and elapsed times. UTF-8 byte metrics
sum message content bytes per decision, including repeated history; they exclude
role names, wire framing and tokenizer behavior. Actual token usage is unavailable
and explicitly null; requested maximum tokens are not measured usage.

Real provider: `scope_forth::LANGUAGE` (`example.forth`). `words` accepts optional
`prefix` and `document_only`; returns sorted vocabulary with stack metadata.
`describe` accepts optional `name`, `symbolic_inputs` (0..32), and `interpretation`
(the latter not requested). It returns found status, definition/stack metadata,
source attribution and bounded symbolic evidence. The host callback ignores model
prose and checks document vocabulary plus final conditional symbolic observations:
misleading-square leaves two references to input node 0, square leaves node 1.
This is not a proof of universal arithmetic safety, termination or equivalence;
all provider obligations and limitations remain in JSON. Tests require equal
provider evidence between arms, reduced context bytes, unchanged text/version/dirty
state, zero proposals, and no successful evidence when host authority is denied.

## Standalone host-driven Script composition

`script_composition` is a separate demonstration of the existing
`scope_script::runner::ScriptRunner`, not another agent arm. The host installs
only `words` and `describe` with `Some(true)` and binds the same `SOURCE` snapshot.
One Script invocation performs words then two describe calls and returns a compact
object projected directly from known `value.total`, `value.word.name`,
`value.status` and `value.symbolic` fields. No loops or arbitrary wrapper are used.
The complete Script text is retained in the report.

The retained `record` includes the full catalog, all three unprojected semantic
results, source/target snapshots and revisions, selection, printed values and
interpreter outcome/final value. `results:0` etc refer **only** to indexes in this
record's `results` array, not durable references. Snapshot identities are preserved
verbatim; the existing API currently reports `SourceId(0)` for both scratches,
so separate host scratch IDs are also retained in `binding`, not treated as global
identities. Both snapshot revisions are `RevisionId(0)`; describe's source revision
is `"0"`. Words has no per-envelope source revision: its provenance is the bound
RunRecord target, not an invented response field.

Script's existing 64-call limit is invocation-local, **not shared with agent
budgets**. No normal agent tool wraps this API: transitive agent budget accounting
is absent. This demonstrates host composition, not an equivalent agent benchmark
or fewer measured LLM rounds. Interpreter success alone does not establish semantic
success; inspect envelope statuses, symbolic finality, outcomes and limitations.
Denial is tested as an interpreter error with no final value or successful results.
The source remains unchanged. No edit, proposal, evaluation, filesystem or runtime
operation is installed.

### Local checkout provenance for the refreshed report

Observed with actual `git rev-parse HEAD` and `git status --short` commands:

- Scope HEAD: `5be95e6f233dbc9b14600fa92049fbcc61ed0ed3`, dirty:
  `crates/scope-ide/src/agent.rs`, `edit_lifecycle.rs`, `investigation_tests.rs`,
  plus this untracked example subtree.
- scope-forth HEAD: `fc095911457ec64af1fcac73a52f071c3788caf6`, dirty:
  `README.md`, untracked `docs/tutorial/`, `examples/tutorial-{basics,candidate,challenge,inspect}.scope`,
  `examples/tutorial-learning.fs`, `tests/tutorial_smoke.rs`.

These are observed local-checkout provenance, not claims of clean commit-only
reproducibility. Dependencies include Scope's local `scope-script` path and the
sibling provider; the fixture is the static `SOURCE`, not a downstream file.
For future runs, record fresh checkout state separately:

```sh
git --no-pager rev-parse HEAD
git --no-optional-locks status --short
git -C ../scope-forth --no-pager rev-parse HEAD
git -C ../scope-forth --no-optional-locks status --short
```

## Optional live mode — unverified

```sh
cargo run --offline --locked --quiet --manifest-path examples/ce1-forth/Cargo.toml -- --live MODEL BASE_URL > examples/ce1-forth/live.json
```

Replace MODEL and BASE_URL with an authorized OpenAI-compatible backend. Cargo's
`--offline` affects dependency resolution, not the backend HTTP request. Live mode
uses CellAgent, the same task/prompt/budgets/grants and host assessment in both
arms, with no arm-specific retuning (CellAgent's default temperature is 0.2).
Decisions are no longer fixed; backend stochasticity and sequential arm order are
confounders. No live run has been validated. Optional `OPENAI_API_KEY` is read from
the environment, never printed by this harness. Requests disclose the fixture,
metadata and evidence to the chosen backend and may incur cost/rate limits.
Reports retain backend responses verbatim: review before sharing; do not commit
live reports containing sensitive content. The retained deterministic report uses
only the static fixture, no credentials or live backend.
