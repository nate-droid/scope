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
  **Ctrl-O Open · Ctrl-R Run · Ctrl-Q Quit · Ctrl-G More** footer.
- Output uses the provider's typed evaluation summary, not inferred values from
  opaque JSON. Blocked results show the blocker and diagnostics; bridge/protocol
  failures show protocol status, message and diagnostics rather than a generic
  completion claim. Invocation errors get a short first-line message. Other native
  result types retain completion/check guidance. Evaluation summaries are not truncated, so
  verbose providers can still require output scrolling.
- Retained output is marked stale after source changes or closure, and while an
  editor has an unpublished draft. **Ctrl-G** → **Evaluate / inspect** exposes
  inspection; **d** in that modal toggles full retained Details.
- **Shift-F5** publishes/checks diagnostics without moving editor focus.
  **Ctrl-S** publishes in memory, **not to disk**. Use **Ctrl-G** → **Save** for
  explicit persistence. **Ctrl-Q** retains dirty-work confirmation; App Escape
  stays in the workspace. **F2** still opens the advanced shell.

The pure Script scratch path is unchanged: Ctrl-S publishes, F5 runs the committed
Script revision. App Run never silently falls back to Script execution.

### Why the inspector could still show the scratch

Investigation distinguished pane composition from source binding: the technical
inspector belongs to the existing workspace composition and can continue showing
scratch context. Opening an App document binds editing and direct Run to that App
document; it does not copy the file into a scratch or rebind every inspector.
The conventional split therefore opts into the quiet editor-only layout instead
of treating unchanged scratch-inspector content as evidence that App Run used the
wrong source. Custom compositions and explicit advanced inspection remain available.

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
