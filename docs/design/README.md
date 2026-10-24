# Detailed implementation contracts

Start with [architecture](../architecture.md), [development](../development.md),
and the [Script guide](../scope-script.md) for maintained usage documentation.
These references cover source ownership, semantic invocation, and trust boundaries.

## Terminal and source ownership

- [Scratch editor diagnosis and source model](SCRATCH_EDITOR_DIAGNOSIS.md)
  ([short tutorial](../scratch-editing.md))
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
