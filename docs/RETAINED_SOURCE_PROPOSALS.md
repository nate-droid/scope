# W8 — Retained source proposals

> Historical W8 results below. [W8b](CONTRACTED_WORKSPACE_EDITING.md) now closes
> the ordinary-launcher configuration limitation described here through the
> additive `run_with_registry_and_command_and_editing` API. Existing ownership,
> proposal review, freshness and acceptance boundaries remain unchanged.

## Result and starting point

Starting revision: `3be9d463c15ae7eac8f02be1923e4f6a1761924e` (clean worktree).

The ordinary workspace now retains the **existing** source-proposal owner in its
`WorkspaceCommandOutput`, alongside the command transcript. That object is owned
by `WorkspaceTui`; the caller continues to own the retained `ScratchBuffers` and
interactive session. Clearing output clears presentation only, not proposals.
No App-document copy, new proposal engine, source owner, or database is involved.

This closes the separate-F4-submission retention bug. It does **not** complete
all contracted-agent configuration through the ordinary CLI launcher; see the
precise limitation below. No Path/Hilb/Cell changes are required by this patch.

## Reconciled lifecycle

In this checkout source-edit primitives are in `src/source_edit.rs` (there is no
`src/cli/source_edit.rs`); semantic edit orchestration is in `edit_lifecycle.rs`.
The CLI adapter is `cli/source_proposals.rs`. `cli/retained_agent.rs` remains a
read-only retained-source runner, independent of editing authority.

| Concern | Existing interface / behavior |
| --- | --- |
| Continuous shell ownership | `source_proposals::Proposals`, containing `SourceEditController` and optional `InteractiveEditConfig`, lives for the shell loop |
| Previous workspace failure | `WorkspaceCommandOutput::submit` called the borrowed shell, which constructed a new `Proposals` every time |
| New workspace lifetime | Each submission borrows the workspace's retained `Proposals` through the same shell dispatcher |
| Other shell callers | Existing borrowed/owned shell entry points still construct invocation-local state; their signatures and scope are unchanged |
| Source ownership | `ScratchBuffers`; `ScratchBuffer` implements public `SourceTarget` |
| Proposal identity | Tagged `SourceId::Scratch(id)` plus version and exact original text; paths are destination context only |
| Review resolution | Resolve the recorded scratch ID, not selection or semantic-target binding |
| Acceptance | `SourceEditController::accept` previews/checks Pending status, identity, version, exact baseline and edit ranges, then applies transactionally |
| Result | Original committed scratch text changes, existing version advances, source becomes dirty; no disk save or semantic execution |
| Closure | On the next dispatcher observation, Pending proposals whose scratch is gone become Discarded through the existing proposal state transition |
| Staleness | Revision/baseline failures remain Pending and inert, with normal freshness diagnostics; no automatic rebase |
| Terminal states | Applied, Rejected and Discarded cannot subsequently apply |

IDs are owner-local. Keep a workspace/command session paired with the **same**
`ScratchBuffers` owner. Do not substitute another collection with overlapping IDs
or transfer a controller between source owners. Scratch IDs are not reused within
one collection. Reopening a file does not resurrect a closed source.

## Exact ordinary-workspace sequence

Submit each line separately with F4 (including the `:scope` prefix):

```text
:scope scratch new text
:scope source-propose {"edits":[{"range":{"start":{"line":0,"column":0},"end":{"line":0,"column":0}},"replacement":"candidate"}],"rationale":"human-supplied candidate"}
:scope source-proposals
:scope source-proposal 1
:scope source-diff 1
:scope source-accept 1
:scope scratch show
```

Alternatively, `:scope source-reject 1` leaves committed source unchanged.
Proposal IDs come from the creation output, not necessarily `1` in a running
session. JSON coordinates are zero-based lines and UTF-8 byte columns, half-open.
The existing `:scope agent edit "instruction"` command also uses this retained
controller with feature `agent`. Its default generator is **unevaluated**; do not
confuse it with a host-authorized contracted semantic editing operation.

Listing shows Pending proposals and their original targets/versions. Diff and
proposal detail validate freshness. Acceptance prints `applied target` with the
original source identity and resulting version. These are committed-memory
revisions, not destination-file revisions.

Acceptance is explicit, but Pending does **not** encode whether a human viewed a
diff. No review acknowledgment state is added. Neither model rationale nor
acceptance proves semantic correctness.

## Minimal public bridge for semantic hosts

`WorkspaceCommandOutput::source_edit_controller()` and, with `tui`,
`WorkspaceTui::source_edit_controller()` borrow the **same existing controller**
used by subsequent source-proposal commands. A trusted composition host can pass
that borrow to the already public `agent::run_semantic_edit_with_discovery` (or
`run_semantic_edit`) together with:

1. The specific committed `ScratchBuffer`, borrowed from its retained owner.
2. `AgentEditRequest::new(target, instruction)` (identity and exact baseline).
3. Its language service and explicitly allowed proposal-producing capabilities.
4. Its existing concrete-call authorization callback and execution limits.
5. Appropriate existing validation/task contract configuration.

Keep the immutable target borrow for generation, release it before application,
and inspect `EditRun`/proposal outcome. Successful registration leaves a Pending
proposal, not an edit. A read-only retained-agent evaluation is not a proposal
capability and cannot be promoted into one by this bridge.

The compiling public-consumer example is
[`retained_proposal_bridge.rs`](../crates/scope-ide/tests/retained_proposal_bridge.rs).
It runs a deterministic semantic edit through the retained controller, then uses
separate ordinary command submissions to list, diff and accept. It also proves
that denied semantic authorization dispatches no proposal operation and leaves
source text/version unchanged. The service is deliberately test-local and makes
no domain-specific correctness claim.

### Remaining ordinary-launcher composition limitation

The normal `run_with_registry_and_command` launcher still accepts a
`ReadOnlyCommand`, whose downstream callbacks receive immutable source borrows,
not `WorkspaceTui` or a mutable proposal controller. Its retained read-only
`semantic_agent` binding exposes no proposal operations. Conversely,
`run_with_registry_and_editing` installs an `InteractiveEditConfig` for a **shell**,
not an ordinary retained workspace. There is not yet a combined public launcher
configuration that installs that fixed contracted editing configuration in the
ordinary workspace.

Therefore the new accessor enables host-owned workspace composition, but is not
an automatic agent-response-to-proposal path through `ReadOnlyCommand`. Do not
bypass this limitation by mutating from a read-only callback, copying into App,
or treating read-only evaluation authority as editing permission. A narrow future
launcher configuration hook for the existing `InteractiveEditConfig` is the
remaining integration task, not a new runner or workflow engine.

## Authorization and draft safety

Proposal production uses the operation's own authorization. Existing semantic
providers recheck authorization and freshness at their established boundaries;
visibility/discovery is not permission. Denial leaves source unchanged.

In-memory acceptance retains the existing authority boundary: explicit human
command dispatch with host-supplied mutable `SourceTarget` access. The shell's
`ScratchBuffer` target has no separate revocable acceptance-policy callback.
Neither workspace filesystem read-only policy nor semantic generation policy is
such a callback: filesystem authorization remains at explicit save. W8 does not
invent an additional authorization system, and does not claim to test revocable
workspace acceptance authority that this API does not expose. Generic
`SourceTarget` application errors propagate without transitioning a proposal to
Applied; implementations must honor the transactional source contract.

F4 `source-accept` and `agent edit` now use the existing conservative workspace
draft blocker. Selected, parked, hidden and unrelated editor drafts block these
operations. Diff/list/reject remain available. Commit/cancel remain explicit; no
draft is discarded to enable acceptance. A committed draft can make an existing
proposal stale. Shell/F2 handoff retains its existing boundaries; it does not
borrow the workspace's proposal session or editor-draft authority.

## Verification and limits of evidence

- Workspace command unit tests: separate submissions, transcript clearing,
  identical text in a different selected scratch, original-target acceptance,
  revision advance, duplicate acceptance, rejection, stale revision, source
  closure/Discarded state, unknown and malformed IDs.
- Workspace composition unit test: selected and parked/unrelated dirty drafts
  block acceptance; explicit cancellation restores acceptance without losing
  the Pending proposal.
- Existing source-edit/proposal tests continue to cover baseline mismatch,
  invalid replacement ranges, terminal lifecycle states and transactionality.
- Public-consumer semantic bridge integration: deterministic authorized proposal
  registration, denied generation and authorization revoked after dispatch
  (before registration), no live model.
- Public-consumer **real ordinary launcher + automated PTY**:
  `source_proposal_launcher.rs` / `pty_source_proposal_launcher.py`, using
  `run_with_registry_and_command` and existing PTY transport. Separate F4 lines
  create/list/diff/accept/reject proposals. Public downstream inspections assert
  exact Unicode committed text and revision `0 → 1`, unchanged after duplicate
  acceptance/rejection, unchanged disk content and terminal restoration.
- No human interactive verification is claimed. PTY emitted-text assertions are
  not a screen-cell emulator. The launcher test does not claim contracted-agent
  integration or independent semantic verification.

Commands:

```sh
cargo test -p scope-ide --features tui,agent --lib
cargo test -p scope-ide --features agent --test retained_proposal_bridge
cargo test -p scope-ide --features tui --test source_proposal_launcher -- --nocapture
```

### Recorded validation

- `cargo test -p scope-ide --features tui,agent --lib`: **636 passed**.
- `cargo test -p scope-ide --no-default-features --lib`: **292 passed**.
- `cargo test -p scope-ide --features agent --test retained_proposal_bridge`:
  **3 passed** (authorized, denied, revoked).
- Public `agent_public_api` integration tests: **2 passed**.
- Real-launcher PTY command above: **1 passed**, **1 intentionally ignored child**,
  approximately 226 seconds; parent invokes that child inside the PTY.
- `cargo fmt --all -- --check`: passed.

One repeated full-library run transiently failed the existing
`render_is_io_free_and_failed_refresh_clears_snapshot` test with assertion text
`original`; it passed both an isolated rerun and the subsequent complete
636-test run. No unrelated watcher/workbench code was changed. Builds also emit
existing Cell duplicate-package warnings and, in non-test TUI builds, an unused
`ScratchEditAccess::new` warning.

## Path DX5 recommendation

Path can use the public bridge if it owns its workspace composition: supply its
existing authorized semantic editing service to the retained controller, return
to F4 review, require explicit acceptance, then resolve the **original** scratch
and independently kernel-check its new committed version. Observe `id()`,
`evaluation_source_id()`, `version()` and `text()`; do not validate a newly selected
source by accident. Evaluation identity is distinct from owner-local proposal ID.
Scope owns proposal orchestration; Path owns theorem validity; Cell owns actual
execution/filesystem capabilities.

For Path's **ordinary normal-launcher callback-only** integration, DX5's full
agent → contracted proposal → human acceptance → independent kernel check still
needs the narrow launcher configuration connection described above. Retention
and review are verified here; that full agent integration is not claimed complete.
