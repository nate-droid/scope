# Source origin authority and authorized analysis snapshots

## Authority decision (C1)

Downstream source checking should receive an invocation-scoped, read-only snapshot
capability. Scope owns source selection, draft freshness, origin attestation and
filesystem authorization; the language owns dependency interpretation and checking.
The context combines the exact current editor draft with reads through its
original authorized grant, rejects stale/foreign source bindings, and exposes
neither writes nor arbitrary host filesystem discovery. Local draft checking
remains available when origin authority is unavailable. There is no live filesystem
monitor, persistent trusted cache or dependency graph manager. The host retains
only the latest invocation's input evidence and acceptance association.

## Retained representation

`ScratchBuffer::origin` is optional host-private `Arc<SourceOrigin>` evidence.
It contains the actual globally fresh evaluation `SourceId`, the attested logical
`ScopePath`, and a `SourceReadGrant`. The logical origin is in Cell's fixed
`project` mount; its parent path is the dependency base within that same grant.
Neither the physical `Destination.root` nor a label establishes authority.

`SourceReadGrant` retains a clone of the existing Cell filesystem capability in a
private Arc, with only a revisioned read operation exposed internally. It retains
neither the Cell execution host nor a Scope Workspace. Cell's cloned filesystem
retains the original mounted directory capabilities; it does not reopen root text.
The underlying mount may be writable, but this wrapper exposes no write method.
Workspace clones share grant identity; separately constructed Workspaces do not,
even with identical root strings. Dropping/replacing an active Workspace does not
revoke an already retained grant or retarget it. Cell has no additional revocation
protocol here; actual reads still enforce confinement and return structured errors.

## Lifecycle and validation

- Authorized open reads first and then establishes evidence. Open deduplication
  requires the same grant and path, not equal root text.
- Every successful explicit bind creates a fresh Arc generation, including
  same-path/same-revision binds. Failed binds preserve prior evidence.
- Successful Save As creates fresh evidence, including same-path Save As. Failed
  Save As preserves the previous association and conflict baseline.
- Ordinary edits, selection changes and same-grant save/reload preserve origin
  evidence. Draft versions and selection/owner-binding generations remain separate.
- Existing save/reload behavior with caller-supplied Workspaces remains intact.
  Successful save/reload through a different grant clears origin evidence rather
  than silently assigning that grant. Failed operations preserve evidence.
- Closure drops the live association. Reopen gets a fresh SourceId. Old evidence
  can retain its private grant but cannot validate against the live collection.
- Unsaved scratch and destination-free session copies have no origin. Later
  active Workspaces, startup paths and labels never fill that absence implicitly.

`ScratchBuffers::validate_origin` looks up the live owner and requires both its
SourceId and exact current Arc generation. Forged, foreign, closed and rebound
associations fail even when paths and revisions match. Its returned borrow pins
the owner lifecycle. Validation establishes retained capability identity, not file
existence: usability of any requested read is established by Cell at access time.

## Invocation context (C2b)

With `tui`, `ReadOnlyCommand::check_analysis` receives a host-created mutable
`AnalysisContext` and an unstructured output writer. Its default implementation
bridges to `check_editor_draft` with only `context.draft()`, preserving existing
callbacks without granting them additional authority. Context construction is
crate-private and follows exact selected-editor validation. A shared borrow of
`ScratchBuffers` pins the live source owner during the invocation. Automatic focus
and another editor's draft cannot establish authority.

- `draft()` retains the complete `EditorDraftSnapshot`, including unpublished text,
  owner baseline, editor/selection identities and binding generations.
- `root()` gives that exact text a fresh logical `SourceId` and invocation revision
  zero, never the committed baseline revision. Evidence explicitly maps this
  analysis identity back to the complete editor snapshot.
- `origin()` optionally returns an opaque invocation-local `SourceHandle`. Missing
  origins do not prevent draft-only checking but provide no dependency-read base.
- `capture(&handle, reference)` returns an immutable `DependencySnapshot` with
  text, fresh logical diagnostic identity, revision, logical path, disk revision
  receipt and a new handle for transitive requests.

Handles have private fields and Arc invocation identity. They expose neither a
read grant nor a Workspace, raw Cell capability, execution, publication or save
authority. Retaining a handle cannot authorize another invocation. The only bases
accepted are host-issued handles from the current invocation.

References are generic strict relative `ScopePath`s, resolved against the base
source's parent within the original `project` grant. Empty, absolute, dot/parent
components and invalid paths fail; Cell performs actual confinement and read
policy enforcement. Scope does not interpret import syntax, discover packages,
construct dependency graphs or overlay other editors' unpublished text. Successful
captures are pinned by resolved logical path for this invocation; repeated reads
return the same snapshot even if disk changes, while recording each edge.

### Bounded acquisition and sealing

Default `AnalysisLimits` allow 128 attempts, 4,096 path bytes, 64 dependency
snapshots, 1 MiB per source, 4 MiB retained text including the root, and 8 MiB
reserved read byte-work. Invalid and repeated requests consume attempts. Before
new I/O the host reserves the worst-case bounded read allowance plus Cell's
one-byte overflow probe, without refund on short reads or failure. Pinned reads
cost no I/O budget. Cell's existing bounded revisioned-read API enforces the
allowance; oversized files cannot cause an unbounded content read. Limits bound
content acquisition, not filesystem operation latency or continuous monitoring.

The host seals evidence itself, including on callback failure. `CheckEvidence`
retains the full editor, original private origin/grant generation, diagnostic
snapshots, every successful resolution edge, ordered acquisition outcomes and disk
revision receipts. Oversized rejected requests retain a bounded prefix plus their
original length. Calls after the attempt limit set an exhaustion marker without
unbounded record growth. Consumers cannot omit observations from this evidence.
Any failed capture or exhaustion makes that invocation historical-only, even if
the checker catches the error and returns success. This conservative rule avoids
silently accepting checks based on missing optional sources that later appear.

Evidence implements `SourceResolver` for exact root/dependency diagnostic pairs;
historical mappings remain usable after changes or source closure. Logical source
identity means path under the retained grant, not inode identity: Cell's revision
is content-based, so same-byte replacement at the same authorized logical path is
not claimed to be detectable as an inode replacement.

### Acceptance and presentation

`WorkspaceTui::check_pinned_editor_draft` validates the complete editor snapshot
before dispatch and again afterward. Only callback success followed by successful
editor and dependency revalidation marks the retained invocation accepted. A
rejected invocation cannot later become accepted merely by restoring its inputs.
A new attempt removes the previous latest-result association.

`check_evidence()` exposes historical mappings only. Every call to
`current_check_evidence(app, sources)` additionally validates the exact selected
editor and live owner/origin generation, re-resolves every recorded successful
edge under the original grant, and compares logical identity, disk revision and
exact bytes. Each validation pass has a bounded byte-work allowance and reads at
most the captured text length plus an overflow probe per edge. Repeated edges can
conservatively exhaust this allowance. Closure, rebinding, owner replacement,
selection changes, unavailable reads or changed dependencies reject currentness.

The public `CheckEvidence::validate` validates source/dependency authority only;
it does not substitute for the workbench's complete editor validation. Raw callback
output is historical, unstructured text and never establishes persistent current
success. These checks are point-in-time best-effort evidence validation, not an
atomic multi-file snapshot or a promise of ongoing freshness.

### Downstream pane presentation (C2c)

`AnalysisContext::invocation_id()` supplies an opaque, cloneable
`CheckInvocationId`. Sealing retains that exact identity in `CheckEvidence` even
for draft-only checks without dependencies. Store the token alongside the
adapter's own mathematical feedback; identical draft text or source names do not
identify an invocation.

`TuiContext::check_presentation()` exposes a host-populated, read-only
`CheckPresentation`. Use `for_invocation(&feedback.invocation)` for feedback:

- `Current`: this exact invocation passed full host validation for this presentation.
- `Historical`: this invocation was rejected or is no longer the latest evidence.
- `ValidationUnavailable`: validation failed or no source owner was supplied.
- `NoEvidence`: the host has no applicable sealed Check evidence (`status()` only;
  previously retained feedback is historical).

The host calls the existing `current_check_evidence` validator once before each
workspace render and downstream view-input dispatch, then shares the result with
panes. Neither the context nor pane reimplements validation. Standalone contexts
are unverified. The view is a point-in-time presentation, not a cacheable freshness
promise, capability, filesystem monitor, or mathematical acceptance result. A
later validation may establish freshness again, but never for an invocation
rejected during final Check validation. A new Check supersedes the old token
without discarding downstream-owned historical feedback. Legacy draft callbacks
continue through the existing bridge; to associate new feedback exactly, adapters
opt into `check_analysis` and capture its token.

## Downstream adapter handoff

Path integration is not implemented. Its adapter must opt into `check_analysis`,
check `context.root()` rather than committed/disk root text, request its own
language-specific references via issued handles, and associate diagnostics with
the supplied exact source/revision identities. It must propagate capture failures
and use retained evidence for diagnostic resolution. Any UI presenting success as
current must retain `context.invocation_id()` with that feedback and consult
`TuiContext::check_presentation().for_invocation(&id)` on every presentation,
not infer status from output text. Host-side consumers can use
`WorkspaceTui::check_presentation(app, Some(sources))` directly.
Package discovery, import interpretation and dependency graph policy stay in the
language adapter; no additional filesystem authority or draft overlays are needed.

## Baseline

Implementation starts at Scope `678f7e2b2aef5a55fc9064cda77df6583219700b`.
Cell remains tag `v0.0.4`, locked revision
`8ece6bf88497b402a6a417c95adf9bf1a6bd6aae`. No Path or Cell source changes.
