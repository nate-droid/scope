# Scope Script architecture review — S6

## Recommendation: keep experimental

A small read-only automation is useful, but this experiment does not justify a
larger language or a new operation framework. Keep Scope Script experimental,
scoped to explicitly authorized, host-bounded inspection recipes. The strongest
next investment is in Scope's reusable discovery/read interfaces and their
metadata, not loops, a planner, a type system, or editing from scripts.

## Runnable workflow and evidence

From the repository root:

```sh
cargo run -p scope-script --example workspace --offline -- --allow-read crates/scope-script/examples/handoff-workspace
cargo test -p scope-script --offline
cargo fmt --all -- --check
```

The first command was run successfully against the checked-in two-document
workspace. It produced a structured Markdown handoff report containing paths,
revision tokens, full source, headings, pending-item lines, document count 2,
pending count 2, and `needs_follow_up: true`. Running the example without arguments
was also checked: it exited with status 1 and an authorization/usage message.
The full scope-script suite passed, including six new workspace tests. Formatting
checks passed. No model, service, network access, or subjective user study was
involved. These are reproducible functional observations, not performance or
agent-success measurements.

Implementation:

- `crates/scope-script/examples/workspace.scope`: six statements; discover the
  adapter actions, discover documents, inspect that discovered batch, build and
  print a handoff report.
- `crates/scope-script/examples/workspace.rs`: explicit command-line grant and
  workspace selection; embedding/output only.
- `crates/scope-script/examples/workspace_support/mod.rs`: read-only downstream
  projection and direct Rust comparison helper.
- `crates/scope-script/tests/workspace_inspection.rs`: output equivalence with
  direct Rust, Unicode source and headings, pending counts, no writes, empty
  workspace, deny-before-open, rejected caller authority, stale revisions,
  retained typed workspace errors, bounds, and symlink exclusion.

### What is actually reused

The host calls existing `Workspace::read_only`, `list_files`, and
`load_file_revision`. Scope owns confinement and revisioned reads. There is no
new Scope provider, registry, invocation engine, or dependency from Scope back to
Scope Script. The two advertised actions are **new downstream Host projections**,
not pre-existing registry operations. The operation registry currently provides
no production workspace-discovery operation to invoke here. This is evidence of
an interoperability gap, not a reason to label example operations as native ones.
Existing registry and semantic-operation script tests remain separate evidence
that those other entry points work; they are not a workspace bridge.

Heading extraction (`#` at the start of a line) and pending-item extraction
(`TODO` or `- [ ]` anywhere on a line) are downstream analysis. They can include
code blocks or checked-off prose and are not Markdown semantics. The script
assembles the report and computes its follow-up flag; it does not implement the
batch traversal or text analysis itself. Most substantive work is in the host.
No parser, evaluator, type-system, or other language changes were necessary.
This deliberately tests a narrow recipe, not general workspace programming.

## Comparison with manual and programmatic invocation

| Path | Work required | Benefit / cost |
| --- | --- | --- |
| Manual Scope API use | List files, select Markdown paths, read revisions/text, reread and compare, inspect headings and pending items | Good for one document; repetitive for a handoff and easy to omit revision checks |
| Direct Rust | Call `discover`, then `inspect`, assemble the same value (`direct` helper) | Same confinement, bounds and output without interpreter overhead; best for maintained application features |
| Scope Script | Discover two actions, invoke them, project fields into report | Short reusable recipe; host installation and all substantive batch analysis still required |

The equivalence test compares complete values, including revision tokens and
source text, between script and direct Rust. It is not an independent test of
Scope filesystem semantics: both paths deliberately share the same host logic.
The manual comparison is procedural analysis, not a timed human experiment.
Scripting saves report-wiring code for a consumer who already has this host;
it does not save the host implementer from defining the workflow. There is no
measured evidence that it beats direct API use for agents.

## Human usability

The six-statement recipe is readable and editable without Rust. Empty workspaces
work without an index failure because documents are handled as a batch. A
checked-in small workspace makes the run reproducible. The CLI denies by default
and has no implicit current-directory grant.

Drawbacks: output is Rust debug formatting, not JSON; discovery relies on stable
positions in this example's two-element action list; no script-file CLI exists;
and batch behavior and limits require reading host documentation. Calling this
fully ergonomic automation would overstate the result. A future downstream CLI
could add JSON output and an explicit script-file input without changing Scope.
Action selection by stable identity would improve scripts, but adding general
collection constructs solely to replace these two fixed positions is premature.

## Agent discoverability and reusable context

The report supplies source-grounded context with document paths, revisions,
headings, exact source, and one-based pending-item line numbers. That is useful as
a reusable *context recipe*: rerun it rather than trusting a stale saved report.
It is not a durable memory or an instruction authority. Document text is untrusted
data; neither text nor a `TODO` grants execution or edit rights. Full source may
contain sensitive material, so exporting the report to an agent/model requires a
separate host/user data-sharing decision. This example sends nothing externally.

Discovery exposes provider/action identity and read effect, but lacks recursive
schemas and rich descriptions. A generic agent could not reliably infer the
required discover-before-inspect lifecycle from this catalog alone. The supplied
script knows that lifecycle. Real agent discoverability should improve through
Scope-owned canonical metadata for reusable Scope operations; downstream batch
recipes should separately document their state and failure contracts. No agent
benchmark was run, so agent-context usefulness remains plausible, not proven.

## Type/schema interoperability

`ToolValue` is shared directly: arrays, objects, strings, booleans and integers
survive without JSON round-tripping or flattened text. Paths, revisions and source
are preserved in output. Host-owned expected revisions cannot be replaced by
script arguments. The direct Rust comparison verifies value-level compatibility.

This adapter accepts only empty argument objects and does not implement a schema
registry. The existing `RegistryHost` remains the route for registry-backed schema
validation and recursive descriptor projection. There is no automatic conversion
between workspace APIs, semantic tools and registry operations. If a reusable
workspace operation becomes warranted, its schemas and validation belong in Scope,
with the script host projecting them rather than inventing another schema layer.
No speculative static types are needed downstream.

## Error quality and revision behavior

Unknown adapter operations preserve `OperationError::UnknownOperation`; stale
content preserves `OperationError::StaleRevision`; budget failures remain
`ErrorKind::LimitExceeded`. Interpreter errors retain byte offsets and stop later
statements. The example keeps original `InspectionError::Workspace(WorkspaceError)`
causes in `host.last_error`, and the CLI prints the host cause on failure.

This is an explicit weakness: `ErrorKind` has no workspace-error variant, so some
workspace failures become runtime text at the interpreter boundary while retaining
the original typed error in a host side channel. Consumers must inspect both.
A downstream extensible host-error envelope would be preferable if more adapters
need this; it is not evidence for a new Scope error framework. Scope's own typed
filesystem/revision failures should remain intact.

Discovery reads documents to record revision tokens. Inspection rereads and
compares before returning any batch result. Deleted files fail with the original
workspace read error retained. Revision constraints cover discovered documents,
not an atomic multi-file snapshot: later additions are not detected and an early
read can change after it is checked. Saved reports are evidence of read revisions,
not guarantees about future state. A new discovery intentionally starts a new
baseline. Failed reads do not roll back consumed call budget.

## Authorization and bounded execution

- `--allow-read ROOT` is mandatory; the host denies before opening a root without
  a grant. The grant is limited to this host's fixed read-only actions/workspace.
- Scope opens the workspace read-only. Scripts have no filesystem, environment,
  shell, network, save, editing, or proposal primitives. No source-edit validation
  or host authorization is bypassed.
- Scripts cannot select arbitrary paths, provide revisions, or add arguments.
  Unknown actions and nonempty argument objects fail. Listing does not follow
  symlinks; the test verifies an outside symlink is not discovered.
- The host rejects over 128 listed regular files, 16 Markdown documents, 32 KiB
  aggregate source per batch, and eight host calls. Oversized batches fail, never
  silently truncate. Existing interpreter source/depth/step/value budgets remain.

Important limitation: Scope currently materializes the recursive listing and each
file before this adapter can check size. These are logical batch/output limits,
not hard bounds on filesystem traversal, individual allocation, or wall-clock
provider execution. The fixed script has no loops and makes three host calls,
but native synchronous reads are not interruptible by interpreter step budgets.
Use only small trusted workspaces; do not claim an adversarial filesystem sandbox.
Hard bounded discovery and streaming size-limited reads, if required generally,
belong in Scope/its capability layer rather than a downstream filesystem bypass.

## Complexity and ownership of improvements

The interpreter and Scope are unchanged. Almost all new Rust complexity is
workspace selection, grant handling, bounded batching, revision tracking, summary
construction and error projection. This host-heavy result is the central finding:
the script is useful packaging but contributes little new analysis capability.

| Improvement | Owner | Reason |
| --- | --- | --- |
| Bounded traversal/reads and reusable workspace discovery contracts | Scope / capability layer | Required by Rust and tool consumers too; must respect confinement |
| Canonical operation schemas and revision-aware discovery metadata | Scope, if promoted to reusable operations | Avoid divergent downstream frameworks |
| Markdown-specific report extraction and context recipe | Downstream host/example | Domain workflow, not core invocation machinery |
| JSON rendering, script input UX, richer host-error transport | Downstream language/embedding | Presentation and interpreter-boundary concerns |
| Agent context freshness and data-sharing policy | Embedding host | Scripts cannot grant authority or model access |
| Planner, unrestricted IO, speculative static typing | Neither for S6 | No demonstrated need; violates the experiment's narrow scope |

Continue only experimentally, with the next evaluation comparing multiple useful
recipes over the *same* host operations and checking whether agents discover them
without source knowledge. If each recipe requires another bespoke batch action,
narrow further to host-provided context reports instead of expanding the language.
