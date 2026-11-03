# Sprout H3 — the ordinary programming loop

Date: 2026-11-04.

## Decision

**Polish narrowly:** the existing-file loop works through the real Sprout editor,
with checked draft publication, source review, contained execution and visible
results. Two concrete usability defects remain: the PTY still selects Open by
its technical action ID rather than proving the natural-language discovery path,
and Check moves focus to output, requiring an explicit return to editing.
Human dogfooding is still needed; this is not comprehensive release readiness.

## Baseline and human observations

The human H2 session reported overwhelming actions, unclear Run, a confusing
in-memory Ctrl-S publication step, and exact-path opening without suggestions.
These are observed frustrations. Improved discoverability and reduced cognitive
load are hypotheses, not outcomes established by automated tests.

Initial revisions (no fetch, no branches or commits):

| Repository | HEAD | Initial status |
| --- | --- | --- |
| Scope | `0a067f4b23b1c9b3d2f90b4388d62c0ed7ec35be` | clean |
| Sprout | `603873b27063aea441d27801de559d7f765249a7` | clean |
| Cell | `8ece6bf88497b402a6a417c95adf9bf1a6bd6aae` | clean |
| Lisp | `1ba8ffbc7a9a99899c0f507a2b25a05c726ac8fb` | existing fixture/service/doc changes |
| Path | `e98c6d66e4a15c87fcf9d4e5e364e8aa1bd37d17` | existing symbolic example/test/doc changes |

Lisp and Path were not edited. Their final status retains the existing changes.
Sprout and Scope are sibling path dependencies: reproduce with both modified
worktrees, not HEADs alone. The Sprout companion report describes Scope changes
as already present because its delegated consumer task began after implementation.

Inspected Sprout's H2 report, getting-started guide, ordinary launcher,
`src/scope_integration.rs`, retained evaluation envelope and existing PTY;
Scope's composition, actions, quick-open, semantic inspection, App opening/saving,
source editor publication, filesystem discovery, and verification contract.
Production evaluation remains the existing Cell-contained service, never native.

## Changes

Scope changes are generic, in `crates/scope-ide/src/tui/`:

- `composition/actions.rs`: Run and Check explicitly use the focused App editor's
  checked publication hook before semantic capture or diagnostics. Primary F1
  actions are Run, Check, Open, Save, Inspect and More actions; advanced actions
  remain accessible through More actions and nonempty searches.
- `composition.rs`: F5 runs an eligible App program; Shift-F5 checks. Script
  scratch F5 remains supported. Run is absent without exactly one eligible
  zero-argument, document-targeted, read-only candidate operation. Availability
  hints do not replace invocation-time discovery and capture.
- `quick_open.rs`: reuse cached authorized `app.files()` suggestions for Open,
  case-insensitive substring filtering, arrows/Home/End, Tab completion of the
  selected literal path, Enter opening and no-match feedback. Spaces are literal.
  No new filesystem abstraction or per-keystroke traversal was introduced.
- `semantic.rs`: Run immediately opens **Run current program?** source review;
  Enter authorizes the captured operation. Concise language-owned feedback and
  full read-only evidence remain available. Tests cover the candidate path.
- `composition/tests.rs`: draft publication, contextual availability, picker
  behavior and menu regression coverage.

Sprout changes are only `tests/pty_editor_experience.py`,
`docs/GETTING_STARTED.md`, and the companion
`docs/experiments/sprout-h3-launcher-experience.md`. No syntax, runtime, launcher,
component, dependencies or evaluation implementation changed. The guide teaches
Open suggestions → edit → F5 → review/Enter → result → experiment; technical
reference remains separate. Success is still `main() = 16`; failures retain
Sprout check error, Sprout execution fault, and Cell host execution failure.

## Source and filesystem guarantees

Run/Check publish in memory, not to disk. Ctrl-S remains explicitly in-memory;
Save is the separate authorized disk operation. Publication reuses complete
editor baseline validation, document identity, language, revision and text checks.
Rejected publication retains the draft and prevents capture/check of an older
published source presented as current. Other dirty drafts conservatively block.
No scratch-source fallback is introduced.

Run does not itself grant execution approval: captured source is reviewed before
one-shot authorization. Existing pre/post-dispatch freshness, catalog and policy
checks remain in force. Inspect is inert/read-only; results do not install code,
save files, or grant reusable authorization. Subsequent publication invalidates
retained results; unpublished edits block inspection rather than presenting an
old answer as current draft evidence.

Suggestions use the existing authorized workspace listing and opening path.
They do not enumerate ambient host paths or bypass confined opening/symlink
checks. The pre-existing initial recursive listing has no depth/entry budget;
this change does not make it a background index or repeat it on keystrokes.
Permission-denied and symlink behavior was not newly exercised by the downstream
PTY; do not infer exhaustive confinement coverage from picker navigation tests.

## New Document: deferred with a concrete blocker

App opening loads an existing revisioned file; App saving requires its disk
revision. Existing scratch Save As can create exclusively under writable Cell
mount authority, but that is a different source owner from App diagnostics and
execution. An untitled App document needs explicit identity/language ownership,
first-save transition, collision handling and draft/rebinding regression coverage.
A scratch copy or synthetic disk revision would bypass the actual lifecycle.
This is not a small menu addition, so H3 does not implement New Document.

## Verification evidence

Commands ran from Scope root, using root target artifacts and deterministic
fixtures, without paid/live model calls or `cargo clean`.

Passing implementation checks (earlier implementation stage):

```sh
cargo test --locked -p scope-ide --features tui --lib
cargo test --locked -p scope-ide --all-features --lib
cargo test --locked -p scope-ide --no-default-features --lib
cargo test --locked -p scope-execution --all-features
cargo clippy --locked -p scope-ide --features tui --lib -- -D warnings
```

Respectively 499, 775 and 310 library tests passed; execution suite passed.
After final contextual menu refinement, the narrower direct/adjacent command
`cargo test --locked -p scope-ide --features tui --lib tui::` passed **95 tests**.
Earlier broad library counts are not a rerun of the final diff.

Real consumer commands:

```sh
cargo build --manifest-path ../sprout/Cargo.toml --locked --offline --example scope --target-dir /Users/nathaniel.ham/Projects/THEWORK/scope/target
python3 -B ../sprout/tests/pty_editor_experience.py target/debug/examples/scope
cargo test --manifest-path ../sprout/Cargo.toml --locked --offline --test scope_integration --target-dir /Users/nathaniel.ham/Projects/THEWORK/scope/target
```

Build and real PTY passed after final menu refinement (build 5.24s warm/incremental,
200s bound; PTY 105s bound, 90s scenario deadline). Integration passed **10 tests**
at the preceding implementation stage, including authorization denial,
different-document rejection, stale/captured identity, oversized source,
bounded recursion and recovery. The PTY verifies keyboard editing, arrows/Tab,
unpublished-draft Run, exact source/revisions 0–3, source confirmation,
cancellation, values 16 and 7, Budget fault and recovery, retained stale evidence,
unchanged disk and terminal restoration. It uses the actual launcher, not shell
commands as a UI substitute. Single-match arrow checks do not establish
multi-candidate UX; unit tests cover ambiguity and spaces.

Full policy remains outstanding: default and all-features workspace test attempts
each timed out at 200s during integration/PTY coverage. All-features workspace
Clippy reported diagnostics in unchanged agent/tool code. Remaining Full matrix,
standalone/scaffold checks and hosted platform parity are not established.
H2's incomplete broad verification is not closed by these focused passes.
No exhaustive new terminal scenarios for type errors, permission denial,
publication rejection or other-language execution were added; these boundaries
have focused unit/integration evidence, not full real-consumer terminal coverage.
One-shot provider authorization remains existing lifecycle coverage rather than
being established by negative terminal text checks alone.

The companion Sprout report records failed initial PTY sequencing attempts and
corrections, exact consumer commands, timing provenance and documentation checks.
Final formatting, direct TUI tests, Clippy and documentation checks are recorded
in the task completion response. No human usability trial has yet occurred.
