# Design contracts and active references

Current user documentation lives in [architecture](../architecture.md),
[development](../development.md), and the [Script guide](../scope-script.md).
Completed results and investigations live in the [archive](../archive/README.md).
These records remain here because they describe detailed implementation contracts
or still-open ownership/composition constraints, not because every proposed
follow-up is implemented.

## Terminal and source ownership

- [Terminal-first interaction](TERMINAL_FIRST_INTERACTION.md)
- [Interactive terminal foundation](INTERACTIVE_TERMINAL_FOUNDATION.md)
- [H1 terminal buffer editing](H1_TERMINAL_BUFFER_EDITING.md)
- [Scratch editor diagnosis and source model](SCRATCH_EDITOR_DIAGNOSIS.md)
  ([short tutorial](../scratch-editing.md))
- [Interactive semantic inspection](INTERACTIVE_SEMANTIC_INSPECTION.md)
- [T1 shell semantic inspection](T1_SHELL_SEMANTIC_INSPECTION.md)
- [T2A explicit source context](T2A_EXPLICIT_SOURCE_CONTEXT.md)

These retain the distinctions between App documents, shell buffers, interactive
sessions, captured requests, freshness, and approval. They remain useful detailed
references for current source/shell behavior.

## Agent composition and observation

- [S1 host-owned semantic context](S1_HOST_OWNED_SEMANTIC_CONTEXT.md)
- [S2 terminal semantic projection](S2_TERMINAL_SEMANTIC_PROJECTION.md)
- [T2B opt-in terminal agent semantics](T2B_TERMINAL_AGENT_SEMANTICS.md)
- [Terminal agent semantic tests](TERMINAL_AGENT_SEMANTICS_TESTS.md)
- [Agent protocol trace](AGENT_PROTOCOL_TRACE.md)

These preserve authority, source binding, model-feedback, and trace contracts and
explicitly distinguish implemented slices from future production work. Live
protocol outcomes and their captures belong in the archive instead.
