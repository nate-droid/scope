# Example catalog

Run commands from the repository root. Offline runs require cached dependencies;
see [setup](../TUTORIAL.md) and [Cell build details](../docs/development.md#toolchain-and-cell).
“Supported demo” here means a checked-in entry point illustrating a current API,
not a production service or an automatically installed IDE capability. Fixture
and historical hosts with contract/safety coverage remain useful regression evidence.

## Supported entry-point demos

| Location | Purpose / invocation |
| --- | --- |
| [`downstream/`](downstream/README.md) | Standalone native integration, CLI and WASM binding coverage; `cargo run -p xtask -- check-downstream --offline`. Its `fixtures/main.calc` and `fixtures/reverse.wat` are test inputs, not applications. |
| [`scope-script` saved scripts](../crates/scope-script/examples/) | `daily.scope` and `pure.scope` use the standalone interpreter: `cargo run -p scope-script -- crates/scope-script/examples/daily.scope`. See the [Script guide](../docs/scope-script.md). |
| [`scope-script` pure stdlib examples](../crates/scope-script/examples/stdlib-text.scope) | `stdlib-text.scope`: publication-label normalization; `stdlib-collections.scope`: function/loop-based numeric analysis; `stdlib-experiment.scope`: S10-S13 structured summary with S12 `parse_int` and safe `get` result branching. Run `cargo run --locked -p scope-script -- crates/scope-script/examples/stdlib-text.scope` (substitute either other filename). No host/capabilities; local value success is not downstream semantic acceptance. Contracts and limits: [Script standard library](../docs/scope-script.md#small-pure-standard-library). Repeated CLI/scratch checks: `cargo test --locked -p scope-script --test stdlib`. |
| [`scope-terminal`](../crates/scope-terminal/examples/) | `minimal.rs` and `completion.rs`: reusable terminal API demos (`cargo run -p scope-terminal --example minimal`). |
| [`scope-types`](../crates/scope-types/examples/composition.rs) | Nominal composition demo (`cargo run -p scope-types --example composition`). |
| [`scope-meta`](../crates/scope-meta/examples/snapshot.rs) | Metadata snapshot demo (`cargo run -p scope-meta --example snapshot`). |
| [`scope-execution`](../crates/scope-execution/examples/text_pipeline.rs) | Explicit execution/component pipeline; consult the [crate guide](../crates/scope-execution/README.md) for its build requirements. |

## Explicit embedding demos and fixture hosts

These are separately compiled, trusted hosts with their own installed services
and policies. Their operation names do not become available in `scope-script FILE`
or the default Scope IDE by naming them in a script.

| Location / files | Classification and purpose |
| --- | --- |
| [`ce4-composition/`](ce4-composition/README.md) | Real sibling Forth → explicit bounded Church adapter → Path kernel checking, with Script, normal agent workflow replay and failure tests. `cargo test --offline --locked --manifest-path examples/ce4-composition/Cargo.toml`. See its [retained evaluation evidence](ce4-composition/deterministic.json) and README for controls and limits. |
| [`harness/`](harness/src/main.rs) | Deterministic scripted tool/approval fixture, not a model agent or general runtime. See the [agent and harness guide](../docs/agents.md#standalone-scripted-harness). |
| [`scope-script/examples/`](../crates/scope-script/examples/) `semantic.rs`, `registry.rs` | API embedding demos with fixed declaration/registry fixtures; paired inputs are `semantic.scope`, `conditional-declaration.scope`, `structured-report.scope`, and `registry.scope`. Semantic command: `cargo run -p scope-script --example semantic -- --allow-read crates/scope-script/examples/semantic.scope`. |
| `workspace.rs`, `workspace_semantic.rs`, `exploration.rs` in that directory | Explicit workspace-host demos, not standalone filesystem privileges. Inputs: `workspace.scope`, `workspace_semantic.scope`, `capability-inspector.scope`, `semantic-explorer.scope`, `markdown-inventory.scope`, `pending-review.scope`. `semantic-workspace/example.decl` and `handoff-workspace/` are fixture data. See the [Script guide](../docs/scope-script.md) for explicit host adapters. |
| `scratch_runner.rs` | Opt-in shell embedding demo: `cargo run -p scope-script --example scratch_runner`. Separate Script source and host target, not a default shell evaluator. |
| `execution_transcript.rs` | Execution-record/output API demo; not replay or proof of semantic acceptance. |
| `trial_runner.rs`, `trial-comparison.scope` | Bounded comparison fixture with safety regression tests, not a generally installed trial runtime. |
| `support/mod.rs`, `workspace_support/mod.rs` | Shared example-host implementation, not independent runnable examples. |
| [`scope-ide/examples/`](../crates/scope-ide/examples/) `embedded_shell.rs`, `native_logging.rs` | Native host embedding demos. |
| [`editable_workspace.rs`](../crates/scope-ide/examples/editable_workspace.rs) | Focused scratch editor and read-only metadata action: `cargo run -p scope-ide --features tui --example editable_workspace`. See the [current launcher tutorial](../TUTORIAL.md#interactive-workspace). |
| `selection_description.rs`, `cross_provider_discovery.rs`, `focused_agent_discovery.rs`, `interactive_semantic_inspection.rs` in that directory | Deterministic semantic discovery/inspection fixture hosts; use tools; `interactive_semantic_inspection` additionally requires `--features tui`. `selection_description_support/` and `state_description_support/` are shared fixture providers, not real language servers. |
| `meta_tools.rs`, `meta_providers.rs`, `read_only_trace.rs` | Tool/provider and deterministic trace fixture demos. Metadata demos use `--features tools`; `read_only_trace` needs `--features agent`. Trace evidence is not a live-model result. |

## Retained evaluation reports and generated artifacts

These standalone Cargo workspaces are not members of Scope's root workspace.
Run their manifest-specific tests; root `cargo test` does not validate them.
The checked-in CE4 `deterministic.json` and H1–H4 `evidence/` files are retained
**evaluation evidence**, not unused configuration or required build inputs. Keep
them with their harnesses and reproduction instructions; wall times vary and local sibling dependencies
remain checkout-dependent despite lockfiles.

| Host / reproduction instructions | Retained artifact | Evidence and limits |
| --- | --- | --- |
| [CE4 composition](ce4-composition/README.md) | `ce4-composition/deterministic.json` | Independent Forth execution → explicit adapter → Path kernel evidence; one bounded arithmetic equality is not a general interoperability proof. |
| [H1–H4 Path retained-agent harness](h1-path-agent/README.md) | [`h1-path-agent/evidence/`](h1-path-agent/evidence/) | Sanitized historical snapshots and summaries, deterministic real-kernel controls and opt-in local-model comparisons; byte counts are not tokens and completed runs do not imply every task passed. |

These artifacts do not establish a general live-model advantage. The host
READMEs describe controls, reproduction instructions and limits. Do not remove
source fixtures, support modules or Cargo targets merely because they are not linked as standalone demos.

## Research hosts and regression coverage

[`h1-path-agent/`](h1-path-agent/README.md) is a standalone research harness,
not an ordinary workbench action. It requires a sibling Path checkout; fake and
H3 control modes script decisions against the real kernel without a model server.
Live modes require an explicitly configured local backend.

[`ag1_typed_tools.rs`](../crates/scope-ide/examples/ag1_typed_tools.rs) is an
opt-in live prompt-comparison host, not a general typed-tool tutorial or launcher.
It requires the `agent` feature and explicit `--live MODEL BASE_URL` arguments
for inference, and emits JSONL including retained protocol events.

Under [`crates/scope-ide/examples/`](../crates/scope-ide/examples/), `q3_live.rs`
is retained because it includes seven deterministic sequence, authorization,
and malformed-decision regression tests. Run those without a backend:
`cargo test -p scope-ide --features agent --example q3_live --offline`.
`trial_runner.rs` likewise retains bounded comparison and safety regression tests.

The Q3 live host and `q3_capture.py` are experiment/capture tools, not generic
agent launchers. As with AG1 and H1 live modes, live invocation requires explicit
opt-in and backend configuration; it can send prompts to a backend, incur cost, and retain sensitive transcripts.
Do not run live captures just to validate documentation.
