# Terminal agent semantic-loop example and regressions

The executable host-embedding example is
[`cli/agent_semantics_tests.rs`](../../crates/scope-ide/src/cli/agent_semantics_tests.rs).
It uses the actual `cli::run_terminal_agent_model` decision loop with a scripted
`DecisionModel`; no network service or API key is needed.

Start with these functions in that file:

- `Service` implements a registered fixture language with a document read,
  a parameterized disposable candidate read, and a prohibited edit operation.
  The candidate returns domain rejection as structured read-only feedback, not
  a transport failure. These operations are test fixtures, not stock tools.
- `fixture` registers that service with `App::builder`, opens a document, and
  makes an unsaved edit.
- `run` shows the real `TerminalSemanticConfig` setup: explicit operation IDs,
  host authorization callback, and caller-owned `AgentLimits`.
- `discovery_and_live_unsaved_calls_preserve_document_disk_and_authority`
  demonstrates discovery, exact source/version envelopes, invocation, and
  structured candidate feedback without changing source or disk.

## Source identity and selection boundaries

`no_matching_registered_service_fails_before_discovery_or_authorization` checks
that an open document with no matching registered service is denied before any
model decision or host authorization. Opt-in semantic mode does not substitute
a neutral service or fall back to generic tools.

`reopening_identical_path_requires_authorizing_the_new_document_identity` opens
the same path with identical text and version but a new `DocumentId`. An old
identity-bound host policy denies the new document; a new policy can explicitly
authorize it. The tool envelope contains source/version preconditions, not a
model-selected document ID. Reusing those preconditions is not itself an
identity grant. Each run binds the currently open document; this test does not
claim that reopening between runs automatically invalidates identical envelopes.

`conflicting_file_highlight_does_not_replace_the_open_document_target` verifies
that a different highlighted App file does not redirect semantic calls away
from the open, unsaved document.

A selected **shell scratch buffer** is a different concept: `ScratchBuffers` is
local state in `run_shell_owned`, and `run_terminal_agent_model` accepts no
scratch-buffer/selection argument. Consequently these direct API tests cannot
supply a conflicting shell scratch selection. They test the reachable App
highlight boundary instead; they do not claim shell-routing coverage. The
optional `InteractiveSession` argument provides inspection, not a replacement
semantic source target.

Run the focused executable examples and regressions with:

```sh
cargo test -p scope-ide --features agent --lib cli::agent_semantics
```
