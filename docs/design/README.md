# Design contracts and retained evidence

Start with [architecture](../architecture.md), [development](../development.md),
the [integration guide](../integration.md), [agent guide](../agents.md), and
[Script guide](../scope-script.md) for maintained usage documentation. Use the
[workspace tutorial](../../TUTORIAL.md#interactive-workspace) for current controls.
The documents below retain detailed contracts, security constraints and qualified
evaluation evidence that should not be mistaken for current tutorial instructions.

## Terminal and source ownership

- [UX1 workspace assessment](UX1_WORKSPACE_ASSESSMENT.md): historical command-entry
  baseline and counted handoffs, superseded for usage by the tutorial.
- [UX4 source reduction feasibility](UX4_SOURCE_REDUCTION.md): semantic inventory,
  equivalence boundary, experimental local rule and proposal integration limits.
- [UX2 workspace actions](UX2_WORKSPACE_ACTIONS.md): direct-action trace counts,
  result attribution, conservative draft guards and PTY coverage limits.
- [S2 retained-source evidence](S2_RESULTS.md) and
  [S3/TUI1 editor evidence](S3_RESULTS.md): compact lifecycle/security summaries
  distinguishing buffered shell, TestBackend and real-PTY validation.

- [Scratch editor diagnosis and source model](SCRATCH_EDITOR_DIAGNOSIS.md)
  ([short tutorial](../../TUTORIAL.md))
- [Interactive semantic inspection](INTERACTIVE_SEMANTIC_INSPECTION.md)
- [Shell semantic inspection](T1_SHELL_SEMANTIC_INSPECTION.md)
- [Explicit source context](T2A_EXPLICIT_SOURCE_CONTEXT.md)
- [Source-aware evaluation](../SOURCE_EVALUATION.md)

App documents, shell buffers, evaluator sessions, and captured invocation requests
have distinct ownership and freshness contracts. Approval does not transfer
between them.

## Agent composition and observation

- [Terminal semantic projection](S2_TERMINAL_SEMANTIC_PROJECTION.md)
- [Opt-in terminal agent semantics](T2B_TERMINAL_AGENT_SEMANTICS.md)
- [Terminal agent semantic regressions](TERMINAL_AGENT_SEMANTICS_TESTS.md)
- [Projected provider arguments and strict decisions](../AI2_PROTOCOL_ROBUSTNESS.md)
- [Read-only investigations](../READ_ONLY_INVESTIGATIONS.md)
- [Agent protocol tracing](AGENT_PROTOCOL_TRACE.md)
- [Interoperability boundaries](INTEROPERABILITY.md)

Discovery, host binding, concrete-call authorization, semantic acceptance, and
execution remain separate. These are current contracts, not progress reports or
claims about live-model reliability.

## Consolidated historical reports

The former SC1 results narrative is superseded by
[namespaced downstream shell commands](../integration.md#namespaced-downstream-shell-commands-sc1).
Its public consumer and regression evidence remain in
`crates/scope-script/examples/downstream_shell.rs` and
`crates/scope-script/tests/downstream_shell.rs`. The original validation reported
112 CLI tests, Script and terminal suites, example execution, formatting and
Clippy with existing warnings. Buffered-shell coverage included registration
bounds, malformed arguments, independent targets, closure/rebinding, denial parity
with the ordinary Script path, retained state and callback failure/recovery.
It did **not** perform a new PTY/manual interactive check or compile/validate the
sibling Hilb integration. Native callbacks remain trusted; registration grants no
authority. Synchronous callbacks have no cancellation guarantee.

The former `INTERACTIVE_WORKBENCH.md` TUI1 narrative is consolidated into the
[S3/TUI1 evidence summary](S3_RESULTS.md), including its read-only event hook,
dispatch order and terminal-restoration qualifications. Its historical bound-target
editor model must not override the current selected-scratch launcher guide.

Research reports CE1–CE4 and AG1/AI3 remain separate: they qualify deterministic
replay versus live inference, retain negative results and describe opt-in follow-up
experiments. Their JSON artifacts and runnable hosts are indexed in the
[example catalog](../../examples/README.md); generated does not mean orphaned.
