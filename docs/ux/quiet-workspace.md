# UX5: quiet workspace

## Current user loop

Open → Edit → Run → Read → Correct is the App-document path. See the
[README](../../README.md#start-here) and [workspace tutorial](../../TUTORIAL.md#interactive-workspace)
for user instructions. The [earlier experience report](minimal-programming-experience.md)
is historical; its review-before-Run description does not describe UX5 direct Run.

- **Ctrl-O** opens an App document. Opening from the launcher's conventional
  equal editor/inspector split selects its editor-only layout. This is not a
  global layout replacement: custom layouts remain unchanged and **F7** still
  cycles layouts, including the technical workspace view.
- **Ctrl-R** publishes the exact focused draft in memory through checked-baseline
  publication, then directly runs the unique eligible candidate. No extra Enter
  is required. Editing focus remains in place; **F5** is an App Run alias.
- The quiet view has a small output strip (at most five rows) and a short
  **Ctrl-O Open · Ctrl-R Run · Ctrl-T Check · Ctrl-Q Quit · Ctrl-G More** footer.
- Output uses the provider's typed evaluation summary, not inferred values from
  opaque JSON. Blocked results show the blocker and diagnostics; bridge/protocol
  failures show protocol status, message and diagnostics rather than a generic
  completion claim. Invocation errors get a short first-line message. Other native
  result types retain completion/check guidance. Evaluation summaries are not truncated, so
  verbose providers can still require output scrolling.
- Retained output is marked stale after source changes or closure, and while an
  editor has an unpublished draft. **Ctrl-G** → **Evaluate / inspect** exposes
  inspection; **d** in that modal toggles full retained Details.
- **Ctrl-T** (Alt-Enter / Shift-F5 aliases) publishes/checks diagnostics without
  moving editor focus. The quiet App view shows **No errors** when clean, or
  diagnostic locations and messages, without the technical document/revision header.
  **Ctrl-S** publishes in memory, **not to disk**. Use **Ctrl-G** → **Save** for
  explicit persistence. **Ctrl-Q** retains dirty-work confirmation; App Escape
  stays in the workspace. **F2** still opens the advanced shell.

The pure Script scratch path is unchanged: Ctrl-S publishes, F5 runs the committed
Script revision. App Run never silently falls back to Script execution.

## Reviewed local Format edits

**Ctrl-G → Format — preview current editor draft** formats unpublished native
editor text when the registered language advertises a whole-document `format`
operation with `ProposeChanges` effect and no required arguments. This does not
reclassify formatting as a read operation. The review overlay shows current and
proposed source; **Enter** explicitly applies, **Esc / c** cancels, and arrows,
PageUp/PageDown and Home/End navigate the bounded preview. Formatter errors and
already-formatted source cannot be applied. No publication, disk save, Check,
Run, execution approval or language-specific revision mechanism is involved.

### Local Apply ownership contract

`WorkspaceTui::apply_reviewed_editor_draft` accepts a pinned `EditorDraftSnapshot`,
whole-document replacement and `DraftApplyDecision`. It validates the complete
snapshot while holding exclusive workspace/editor ownership, then dispatches to
the selected retained editor's `Pane::apply_editor_draft` implementation. Default
panes decline; the built-in `EditableScratchPane` records one ordinary local undo
transaction. There is no second history stack. `Draft::replace` alone is only a
text primitive, `ScratchEditAccess::commit` publishes authoritative source, and
committed-source proposal acceptance updates a source revision; none substitutes
for this local edit boundary.

Identity includes the authoritative document/baseline, local edit identity,
editor instance/generation, source/editor selections, owner-binding generation,
cursor and selection anchor. Typing, undo/redo, a source round trip, rebinding or
editor replacement invalidates the old pin even after byte-identical restoration.
Stale rejection changes neither draft nor history. Custom editor implementations
remain trusted native host code and must honor the hook's local identity/history
contract; this API is not a sandbox or an agent authorization mechanism.

Apply clears redo, advances edit identity and separates subsequent typing. Cursor
keeps its byte offset clamped backward to a replacement grapheme boundary;
selection clears. Undo/redo restore exact text, cursor and selection. Cancel and
exact no-op preserve identity and both history branches. The baseline and source
revision remain unchanged. Providers or future reviewed agent edits can reuse
this boundary, but agent review/authorization and multi-range proposals are not
implemented here.

Existing Check/Run results remain inspectable. Scratch Check uses its existing
full-snapshot validator; retained App diagnostics are now also pinned and labeled
historical after a draft identity change, including byte-identical Undo. Retained
Run keeps its existing source-revision and unpublished-draft presentation rules.
No execution approval is renewed. Changes to unrelated sources do not invalidate
App diagnostic evidence.

The UI accepts no edits or one whole-document replacement, caps each source at
256 KiB, sanitizes terminal controls, and uses a cell-wrapped, usize-indexed
viewport so proposed text remains reachable beyond 65,535 display rows. Provider
resource limits and diagnostics still apply. Production syntax support is solely
the language provider's responsibility; Sprout's production formatter does not
accept its experimental module grammar.

### S1 verification and remaining limits

Commands below ran against this implementation, reusing existing Cargo targets
(no clean/cold-build claim). Scope commands run from `scope`; Sprout commands
from the sibling `sprout` checkout. No guest artifacts, dependency manifests,
execution policies or language semantics changed.

| Command | Result |
| --- | --- |
| `cargo test --locked -p scope-ide --features tui --lib` | Passed, including local Apply identity/history, Format review and retained diagnostics regressions. |
| `cargo test --locked -p scope-ide --all-features --lib` | Passed, including registered-editor integration and policy rejection tests. |
| `cargo test --locked -p scope-ide --features tui --test retained_workspace --test source_edit_controller --test source_evaluation` | Passed: 4 + 8 + 3 tests. |
| `cargo test --locked -p scope-execution --all-features` | Passed: 14 real-Wasm execution + 13 policy tests. |
| `cargo test -p scope-ide --locked --features tools` | Passed, complete package. |
| `cargo test -p scope-ide --locked --no-default-features` | Passed, complete package. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed after correcting the new preview test-module ordering. |
| `cargo clippy -p scope-ide --all-targets --features tui --locked -- -D warnings` | Passed. |
| `cargo clippy -p scope-ide --all-targets --features tools --locked -- -D warnings` | Passed. |
| `cargo clippy -p scope-ide --all-targets --no-default-features --locked -- -D warnings` | Passed. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked` | Passed after quoting the existing `AppState<P>` documentation in the changed pane file. |
| `cargo test --locked --manifest-path examples/downstream/Cargo.toml` | Passed: 4 tests. |
| `cargo test --locked --manifest-path examples/harness/Cargo.toml` | Passed: 7 tests. |
| `cargo run --locked --manifest-path examples/harness/Cargo.toml` | Passed approved/denied fixture journeys. |
| `cargo fmt --manifest-path examples/managed-transfer/Cargo.toml -- --check` | Passed. |
| `cargo test --locked --manifest-path examples/managed-transfer/Cargo.toml` | Passed: 1 test. |
| `cargo run --locked -p xtask -- check-downstream --offline` | Passed downstream/harness checks, generated-language tests and binary smoke runs. |
| `cargo fmt --all -- --check` | Passed in both checkouts. |
| `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v` | Passed: 5 tests. |
| `python3 -B .github/scripts/check_markdown_links.py` | Passed. |
| Sprout: `cargo test --locked --test scope_formatting` | Passed: 4 canonical adapter tests; also passed with `--features closure-discovery,bundle-check`. |
| Sprout: `cargo build --locked --features closure-discovery,bundle-check --example scope` | Passed; no guest build. |
| Sprout: `cargo test --locked --features closure-discovery,bundle-check --test interactive_workflow native_format_preview_apply_cancel_undo_redo_through_actual_launcher -- --exact --nocapture` | Passed actual native PTY: unsaved preview, Esc/c Cancel, Apply, one-step Undo/Redo, already-formatted no-op, unchanged disk, unsaved quit guard and terminal restoration. |

**Full remains incomplete**, not replaced by the passing focused checks:

- `cargo test --workspace --locked` timed out at 240 seconds in
  `source_proposals_through_real_launcher`.
- `cargo test --workspace --all-features --locked` timed out at 240 seconds during
  `h4_shortcuts_through_real_launcher`.
- `cargo test -p scope-ide --locked --features tui` timed out at 180 seconds in
  `source_proposals_through_real_launcher`. These broad timed-out commands were
  not repeated; their full PTY closure remains unverified.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  failed. A narrowed diagnostic follow-up,
  `cargo clippy -p scope-ide --lib --all-features --locked -- -D warnings`, reports
  five errors in unchanged `app/mod.rs`, `registered_document_closure.rs` and
  `tui/registered.rs` (large error/type-complexity and collapsible-if lints).
- Sprout's `cargo test --locked --test scope_formatting --test formatting`
  stopped at `maintained_production_examples_round_trip`: 10 formatter tests
  passed, one failed with `expected fn`; the corpus includes experimental module
  files. The adapter target was subsequently run independently and passed.
- Sprout's `cargo test --locked --features closure-discovery,bundle-check --test scope_integration --test scope_formatting`
  stopped in the execution consumer target (4 passed, 15 failed). A narrowed
  `cargo test --locked --features closure-discovery,bundle-check --test scope_integration maintained_programs_and_entry_outcomes_through_app -- --exact --nocapture`
  confirms an absent `execution.preview.text` expectation at `tests/scope_integration.rs:393`.
  That execution-path failure was not repaired or worked around by rebuilding
  guests; it does not establish a formatter regression or a passing execution suite.

Automated native PTY evidence is not human usability testing or cross-platform
terminal certification. Custom panes must populate the new `cursor`/`anchor`
snapshot fields and explicitly implement the local Apply hook to support edits.
Untitled scratches without a language path and general multi-range proposals are
not supported by this Format UI. There is no agent auto-Apply or new execution
authorization, and B1/B2/candidate boundaries are unchanged.

### Why the inspector could still show the scratch

Investigation distinguished pane composition from source binding: the technical
inspector belongs to the existing workspace composition and can continue showing
scratch context. Opening an App document binds editing and direct Run to that App
document; it does not copy the file into a scratch or rebind every inspector.
The conventional split therefore opts into the quiet editor-only layout instead
of treating unchanged scratch-inspector content as evidence that App Run used the
wrong source. Custom compositions and explicit advanced inspection remain available.

## UX6b: Check shortcut and quiet diagnostics

Implemented from clean baseline `6fa677564e38a54bc65cad9dd07b31bd5c5a6c62`.
**Ctrl-T** is the primary Check gesture: it has an ordinary terminal control-byte
encoding and was unowned in the inspected workspace/editor/shell input paths.
**Ctrl-K** is deliberately not used: the shell owns it for kill-line. Alt-Enter
and Shift-F5 remain compatibility aliases; Ctrl-T avoids relying on terminal
Alt-Enter or modified-function-key delivery. Command input and inspection/picker
modals retain their existing input precedence. No real-terminal portability
claim is made from unit tests alone.

Check retains the exact-baseline editor publication path, unrelated-draft
blocking, App source identity checks, and editor focus. It neither saves nor
executes. Quiet App diagnostics replace older output, reset scrolling, and show
**No errors** only for a clean diagnostic result; nonempty diagnostics retain
locations, severity, attribution and messages. The advanced non-App output keeps
its document/revision header. Existing retained Run evidence is not discarded.

Ownership was limited to composition actions/input/footer/diagnostics, focused
composition unit tests, and shortcut documentation. No edits were made to
`scratch_editor`, workbench PTY tests, or `docs/ux/comfortable-editing.md`.

Current-change verification (not the historical UX5 evidence below):

| Exact command | Result |
| --- | --- |
| `cargo test --locked -p scope-ide --features tui --lib tui::composition::tests` | 58 passed, 0 failed, 0 ignored; 456 filtered out. Includes Ctrl-T/aliases/modifier rejection, footer, clean Check, diagnostic rendering, publication conflict, draft blocking and adjacent Run behavior. |
| `cargo clippy --locked -p scope-ide --features tui --lib -- -D warnings` | Passed; Cargo emitted duplicate-package notices for the existing Cell checkout. |
| `git diff --check` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v` | 5 passed. |
| `python3 -B .github/scripts/check_markdown_links.py` | Passed. |

Verification scope is direct and adjacent for isolated UI input/presentation under
the [verification policy](../verification.md). No source lifecycle, execution,
authorization, dependency or public contract was changed. Full and PTY suites
were not run; these results do not establish terminal/platform parity or release
confidence, and do not close historical UX5 Full-verification gaps. No timing or
build-warmth claim is made.

## Exact authority boundary

Direct Run is an explicit **trusted native-host gesture** authorizing one captured
candidate request. Selection requires exactly one eligible zero-argument,
document-targeted operation with declared `Read` effect and `CandidateEvaluation`
purpose. Missing or ambiguous candidates are not guessed. Operations requiring
arguments, other targets/effects or capabilities are not automatically selected
as a fallback; they need their appropriate explicit host workflow.

The one-shot authorization matches operation identity, source identity, version,
text, language and empty arguments. The pending binding is consumed before
invocation; repeating its execution does not recapture or retry. Source freshness
and the existing invocation/execution-policy checks remain in force. Checked
publication conflicts retain the draft, and other dirty drafts still block Run.
Neither publication nor execution saves, accepts proposals or certifies results.

`Read` is a provider declaration, **not a sandbox**. Native language services are
trusted host Rust code; this UI does not confine ambient filesystem, process or
network access. Capability declarations and policy rejection must not be treated
as authority granted by discovery. Advanced **Evaluate / inspect** still requires
explicit capture/review followed by confirmation; opening it does not execute.
Agents remain subject to their **separate allowlist and authorization policy**:
a human Run gesture does not authorize later agent calls or create a session grant.
See [contributor boundaries](../../CONTRIBUTING.md#keep-changes-focused-and-authority-explicit)
and the [agents guide](../agents.md).

Implementation inspected for this account:
[semantic selection, one-shot invocation and summaries](../../crates/scope-ide/src/tui/semantic.rs),
[checked publication and direct dispatch](../../crates/scope-ide/src/tui/composition/actions.rs),
[layout and output rendering](../../crates/scope-ide/src/tui/composition.rs), and
[inspection eligibility and invocation binding](../../crates/scope-ide/src/semantic_inspection.rs).
Code inspection is not test execution.

## Evidence and decision

The implementation agents supplied the following command/result evidence; Rust
checks were not rerun by this documentation task. Counts from earlier runs are
kept distinct from later targeted additions. No build-warmth or performance claim
is made.

| Command | Reported result |
| --- | --- |
| `cargo test --locked -p scope-ide --features tui --lib tui::semantic::tests` | 6 passed. |
| `cargo test --locked -p scope-ide --features tui --lib tui::composition::tests` | 57 passed. |
| `cargo test --locked -p scope-ide --features tui --lib` | Final rerun: 510 passed. |
| `cargo test --workspace --all-features --locked` | Bounded timeout at 240 seconds, having reached `interactive_semantic`; IDE library reported 786 passed and downstream launcher reported 3 passed, 2 ignored before timeout. The workspace command did not complete. |
| `cargo test --workspace --locked` | Failed at the stale H4 header assertion. The assertion was subsequently fixed; the full default workspace command was not repeated. |
| `cargo test --locked -p scope-ide --features tui --test downstream_launcher` | After the H4 assertion fix, 3 passed, 2 ignored. |
| `cargo test --locked -p scope-execution --all-features` | 27 passed: 14 real-Wasm execution tests and 13 policy tests. |
| `cargo test --locked -p scope-ide --no-default-features --lib` | 310 passed; library-only minimal-feature coverage. |
| `cargo test --locked -p scope-ide --features tui --test interactive_semantic_inspection --test shell_semantic_inspection` | Final rerun: 8 passed, 1 ignored PTY child; 26 passed. An obsolete editor-header assertion failed initially and was updated to assert source text and absence of technical headers. |
| `cargo clippy --locked -p scope-ide --features tui --lib -- -D warnings` | Passed. |
| `cargo fmt --all -- --check` | Passed. |
| `python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v` | 5 passed. |
| `python3 -B .github/scripts/check_markdown_links.py` | Failed on the pre-existing missing `h4a-follow-up.md` link in `docs/ux/minimal-programming-experience.md:3`. |

Earlier supplied adjacent evidence also reported `workspace_ui`: **5 passed**.
Ignored tests are not passes. The later launcher rerun closes the reported H4
assertion failure locally, not the entire default workspace run.

**Full verification is incomplete.** The all-features workspace run exhausted its
240-second bound; reported free disk space is now about **9 GiB**, so no broad
repeat was attempted. Remaining work includes complete default/all-features
workspace closure, the remaining isolated feature/target configurations, Full
lint and rustdoc checks, and standalone consumer/scaffold matrix checks required
by the [verification contract](../verification.md). Targeted library lint and
minimal-feature library tests do not substitute for those all-target checks.
The documentation link failure also remains open. Preserve rejection, stale-source,
execution-policy and relevant PTY coverage when completing verification.

**Ready for downstream adoption:** The ordinary programming experience is minimal,
direct and source-safe. This decision is scoped to the inspected App Run boundary,
supported by direct, adjacent, real-launcher and real-Wasm policy evidence. This is readiness to adopt that focused workflow, **not release
readiness, Full completion, or full regression/security confidence**. Downstream
hosts must retain their native-provider trust and authorization boundaries and
validate their own integration. No new human usability evidence was supplied;
provider summary quality remains a visible limitation, and no language-specific
output interpretation was added.
