# W8b — Contracted editing through the ordinary launcher

## Reconciled starting point

Scope started at `5643eb925506ba734ed43f2537c2fece5b4c98ce` with a clean worktree.
Cell remains the locked Git dependency at tag `v0.0.2` (`86145d1e`); no Cell or
Path files were changed. No Path checkout revision was inspected or assumed.

The actual public APIs at that revision were:

- `cli::run_with_registry_and_command(args, registry, command)`: normal startup,
  immutable downstream callbacks and W7b `downstream_actions()` registration.
- `cli::run_with_registry_and_editing(args, registry, editing)`: shell only,
  without a downstream command host.
- `InteractiveEditConfig { service, task, options, authorize }`, behind `agent`.
- `WorkspaceCommandOutput` owned the existing private `Proposals`; `WorkspaceTui`
  owned that command session. Both exposed `source_edit_controller()`.
- `SourceEditController`, `AgentEditRequest::new`, `agent::run_semantic_edit` and
  `run_semantic_edit_with_discovery` were already public. CLI `source_proposals`
  already resolved committed scratch identity/baseline and ran contracted edits.
- `cli/retained_agent.rs` exposed an independent read-only retained-agent route;
  its successful evaluation did not grant proposal-production authority.

The missing piece was startup configuration, not proposal orchestration. W8's
public bridge required access to the workspace object, inaccessible to the normal
callback-only launcher. Inspection also found that F4 discarded parsed model
settings and reconstructed the default endpoint on every submission.

## Public composition

Enable `scope-ide` features `agent,tui`. Use:

```rust,ignore
scope_ide::cli::run_with_registry_and_command_and_editing(
    std::env::args().skip(1),
    registry,
    &mut existing_command_host,
    editing,
)?;
```

The compiling host adapter is
[`contracted_workspace.rs`](../crates/scope-ide/examples/contracted_workspace.rs).
The fully instantiated deterministic language, strict contract, capability
inventory, authorization callback and normal-launcher consumer are in
[`configured_proposal_launcher.rs`](../crates/scope-ide/tests/configured_proposal_launcher.rs)
and [`support/configured_editing.rs`](../crates/scope-ide/tests/support/configured_editing.rs).
They use public Scope APIs only. The example adapter deliberately takes the
host's existing configuration rather than inventing Path/Hilb semantics.

This additive API supports `workspace` and `shell`. Unsupported startup modes
return an error; `--help` retains normal short-circuit behavior. Existing launcher
signatures remain unchanged. The older editing-only launcher stays shell-only.
CLI argument parsing, workspace capability setup, language registration, W7b
actions, initial editor and command-bar composition are reused.

The ordinary launcher validates and moves the editing contract into its retained
`WorkspaceCommandOutput` before entering the workbench. Parsed `AgentConfig`
(model, endpoint, credential settings) is retained too. Each F4 dispatch borrows
the same `Proposals`, controller, contract and authorization closure; transcript
clearing does not reset any of them. There is no global state, new agent loop,
source owner, policy engine or proposal engine.

For hosts that already own a custom workspace composition,
`WorkspaceCommandOutput::with_editing(editing)` validates and creates that same
retained session. It must remain paired with the same `ScratchBuffers` owner.
This constructor is not needed for normal-launcher adoption.

## Lifecycle

Open a committed language scratch (F2 `:scope scratch open file` then `:scope quit`
returns to the workspace; F2 has its own shell proposal session). In the editor,
Ctrl-S commits a draft **in memory**. Submit each following line separately in F4:

```text
:scope agent edit "replace old with new"
:scope source-proposals
:scope source-diff 1
:scope source-proposal 1
:scope source-accept 1
:scope scratch show
```

Use the proposal ID actually returned, not an assumed `1`. To reject instead:
`:scope source-reject ID`. To pin a specific scratch explicitly, use the existing
JSON-quoted instruction directive, e.g.
`:scope agent edit "@buffer:1 replace old with new"`.
Selection resolves the default edit target, **not** the independent read-only
semantic-target binding. Changing either after generation cannot retarget it.

The configured branch calls the existing `run_semantic_edit` (the established
non-discovery wrapper), not the unevaluated default generator. It constructs
`AgentEditRequest::new` from the original committed buffer, using its source ID,
revision and exact text. Its allowlisted document operations, budgets, concrete
semantic authorization and strict host-owned accepted mandatory criteria remain
required. Invalid service language or incompatible capabilities fail before the
model runs. Configured failures never downgrade to unevaluated generation.

Successful semantic dispatch and configured observations register a **Pending**
proposal in that same controller. Generation leaves committed source unchanged.
Review, rejection and explicit transactional acceptance reuse W8's commands.
Acceptance checks the original identity, revision, exact baseline and validated
ranges. Closed sources are discarded; stale proposals stay Pending and cannot
apply. Dirty editor drafts block unsafe dispatch and acceptance, including parked
or unrelated drafts. No automatic commit, cancellation, save or semantic recheck
occurs. Applied source revision advances only on the original scratch.

Acceptance prints the original target and resulting version. A downstream
read-only inspection callback can independently validate the committed snapshot
and record `id()`, `evaluation_source_id()`, `version()` and `text()`. The callback
must resolve/bind the original scratch rather than accidentally validate the new
selection. Evaluation identity is distinct from owner-local proposal identity.
Scope does not claim a passing language check proves domain correctness.

## Authority distinction and limits

Read-only retained-agent completion, selection, target binding, discovery and
W7b action registration grant no editing authority. The configured semantic
operation has its own allowlist and concrete authorization callback. Revoking
that callback can block production at the established semantic boundaries; it
is **not** a revocable acceptance policy.

Scratch acceptance still uses explicit human command dispatch plus mutable
`SourceTarget` access. There is no separate revocable acceptance-policy callback,
new application grant, or review acknowledgment state. Acceptance does not save
to disk and does not certify semantics. Filesystem grants remain relevant at
explicit save. The contract is fixed for the retained session; this change does
not add dynamic per-request task replacement or multi-service editing routing.
F2's shell is still a separate proposal session; use F4 throughout proposal
creation/review/application to retain the configured workspace lifecycle.

## Verification

- `configured_public_consumer`: public construction and launcher parsing/help,
  invalid host-contract rejection, deterministic contracted proposal evidence,
  later review/acceptance and stale-revision rejection.
- `configured_proposal_launcher` + `pty_configured_agent_launcher.py`: the real
  combined launcher, a registered deterministic language, existing read-only
  downstream host and isolated local OpenAI-compatible HTTP fixture. F4 actually
  invokes `agent edit`; no live LLM and no raw `source-propose` shortcut.
- Automated PTY verifies generation leaves `old` at revision 0, retained Pending
  listing and diff, configured passing checks, rejection without mutation,
  explicit acceptance to `new` at revision 1, duplicate acceptance safety,
  changed selection without retargeting, denied production with no proposal,
  dirty-draft dispatch (zero model requests) and acceptance guards, unavailable
  language failure (zero model/authorization calls), unchanged disk and terminal
  restoration. Review/application do not ask the model again.
- Existing W8/low-level tests retain baseline, range, transactionality, source
  closure and parked-draft invariants rather than duplicating that machinery.
- No manual interactive session or Path/Hilb semantic verification is claimed.
  PTY assertions inspect emitted text and public downstream source snapshots,
  not a terminal screen-cell emulator. Loopback binding requires an environment
  that permits local sockets.

### Recorded validation

Final recovered-checkout validation:

```sh
cargo test -p scope-ide --features agent,tui --lib \
  --test configured_public_consumer --test registry_command_startup \
  --test retained_proposal_bridge --test agent_public_api --offline
cargo test -p scope-ide --features agent,tui \
  --test configured_proposal_launcher --offline -- --nocapture
cargo check -p scope-ide --features agent,tui --example contracted_workspace --offline
cargo check -p scope-ide --no-default-features --offline
cargo check -p scope-ide --features tui --offline
cargo fmt --all -- --check
git diff --check
```

Passed: **636 library tests**, **11 public integration tests**, and **1 PTY parent
covering four launcher scenarios** (one intentionally ignored child invoked by
the driver). The final PTY rerun took 92 seconds and required permission to bind
loopback outside the terminal sandbox. Example compilation and both non-agent
feature checks passed. Existing Cell duplicate-package and unused
`ScratchEditAccess::new` warnings remain; no unrelated warning cleanup was done.

## Path DX5b adoption

1. Keep the existing registry, `ReadOnlyCommand` and W7b action registrations.
2. Build an `InteractiveEditConfig` with Path's static service, Strict host-owned
   accepted mandatory task criteria, explicit document proposal-operation names,
   bounds and independent concrete-call authorization. Register the same language.
3. Switch only the launcher call to the combined variant, passing that config.
   Do not acquire mutable proposal authority in `invoke_downstream`.
4. Open/select the retained Path scratch and commit/cancel local drafts. Invoke
   existing F4 `:scope agent edit`; optionally pin `@buffer:ID`.
5. Require human F4 review and accept/reject. Record the original scratch identity
   and returned revision. Independently kernel-check that exact committed source
   through Path's existing read-only host operation; do not infer validity from
   acceptance or from another selected scratch.

No separate `WorkspaceTui`, duplicated CLI startup, new runner or Path changes
are implemented here. Scope owns composition/proposals/review; Path owns domain
validation; Cell continues to own actual capability boundaries.
