# Example catalog

Run commands from the repository root. Offline runs require cached dependencies;
see [setup](../docs/getting-started.md) and [Cell build details](../BUILDING_WITH_CELL.md).
“Supported demo” here means a checked-in entry point illustrating a current API,
not a production service or an automatically installed IDE capability. Fixture
and historical hosts remain useful regression evidence; no code/tests were removed.

## Supported entry-point demos

| Location | Purpose / invocation |
| --- | --- |
| [`downstream/`](downstream/README.md) | Standalone native integration, CLI and WASM binding coverage; `cargo run -p xtask -- check-downstream --offline`. Its `fixtures/main.calc` and `fixtures/reverse.wat` are test inputs, not applications. |
| [`scope-script` saved scripts](../crates/scope-script/examples/) | `daily.scope` and `pure.scope` use the standalone interpreter: `cargo run -p scope-script -- crates/scope-script/examples/daily.scope`. See the [Script guide](../docs/scope-script.md). |
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
| [`harness/`](harness/src/main.rs) | Deterministic scripted tool/approval fixture, not a model agent or general runtime. See the [harness guide](../HARNESS_GUIDE.md). |
| [`scope-script/examples/`](../crates/scope-script/examples/) `semantic.rs`, `registry.rs` | API embedding demos with fixed declaration/registry fixtures; paired inputs are `semantic.scope`, `conditional-declaration.scope`, `structured-report.scope`, and `registry.scope`. Semantic command: `cargo run -p scope-script --example semantic -- --allow-read crates/scope-script/examples/semantic.scope`. |
| `workspace.rs`, `workspace_semantic.rs`, `exploration.rs` in that directory | Explicit workspace-host demos, not standalone filesystem privileges. Inputs: `workspace.scope`, `workspace_semantic.scope`, `capability-inspector.scope`, `semantic-explorer.scope`, `markdown-inventory.scope`, `pending-review.scope`. `semantic-workspace/example.decl` and `handoff-workspace/` are fixture data. See the [workspace semantic walkthrough](../docs/SCOPE_SCRIPT_WORKSPACE_SEMANTIC.md). |
| `scratch_runner.rs` | Opt-in shell embedding demo: `cargo run -p scope-script --example scratch_runner`. Separate Script source and host target, not a default shell evaluator. |
| `execution_transcript.rs` | Execution-record/output API demo; not replay or proof of semantic acceptance. |
| `trial_runner.rs`, `trial-comparison.scope` | Experimental bounded comparison fixture, not a generally installed trial runtime; [completed trial-runner record](../docs/archive/SCOPE_SCRIPT_SS4_TRIAL_RUNNER.md). |
| `support/mod.rs`, `workspace_support/mod.rs` | Shared example-host implementation, not independent runnable examples. |
| [`scope-ide/examples/`](../crates/scope-ide/examples/) `embedded_shell.rs`, `native_logging.rs` | Native host embedding demos. |
| `selection_description.rs`, `cross_provider_discovery.rs`, `focused_agent_discovery.rs`, `interactive_semantic_inspection.rs` in that directory | Deterministic semantic discovery/inspection fixture hosts; use tools; `interactive_semantic_inspection` additionally requires `--features tui`. `selection_description_support/` and `state_description_support/` are shared fixture providers, not real language servers. |
| `meta_tools.rs`, `meta_providers.rs`, `read_only_trace.rs` | Tool/provider and deterministic trace fixture demos; require `--features tools`. Trace evidence is not a live-model result. |

## Historical live-experiment hosts

Under [`crates/scope-ide/examples/`](../crates/scope-ide/examples/), `m2b_live.rs`,
`m2b_live.py`, `m2c_live.py`, `q2_live.rs`, `q3_live.rs`, and `q3_capture.py` are
experiment/capture hosts, not onboarding commands or supported generic agent
launchers. They require explicit model/backend configuration; live invocation can
send prompts/source to a backend, incur cost, and retain sensitive transcripts.
Do not run them just to validate documentation. The [Q2](../docs/archive/Q2_LIVE_QWEN_PROTOCOL.md)
and [Q3](../docs/archive/Q3_SEQUENTIAL_TOOL_SERVING.md) reports retain their full
original capture directories and limitations. Older capture scripts' default
output directories are unchanged; archived evidence is not a new run destination.
