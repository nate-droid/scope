# Minimal programming experience: implementation and evidence

> H4 historical report below. See the [H4a follow-up](h4a-follow-up.md) for
> current controls, regression diagnosis, final focused evidence and remaining
> Full verification gaps.

## Scope and baseline

Initial clean revision: `e556d634c12f1bf6718ab7596327b5dc8edfd9f2`.
Initial `git status --short` was empty. All changes are confined to Scope;
no consuming-language repositories were edited. This report describes a partial
H4 implementation, not a release certification.

The intended small loop is Open → Edit → Run → Read → Correct, with Check and
explicit Save available without requiring shell commands. The current behavior
is narrower than a finished minimal programming interface.

## Current controls

| Control | Implemented behavior |
| --- | --- |
| Ctrl-O | Open an App document by path. |
| Ctrl-R | Publish the exact focused App editor draft in memory, then open source review for the unique eligible candidate evaluation. Enter explicitly confirms one invocation. |
| Shift-F5 | Publish/check the App draft, retaining editor focus. |
| Ctrl-S | Existing in-memory Publish, **not disk Save**. Use the explicit Save action in the action picker for persistence. |
| Ctrl-G / F1 | Actions/help; F1 is an alias. The existing compact contextual picker retains More actions and searchable advanced entries. |
| Ctrl-Q | Warn for dirty editor drafts, dirty App document or any dirty scratch; second Ctrl-Q discards and exits. Esc cancels quitting. |
| Esc | App editor workspace stays open. Scratch/legacy contexts still hand off to shell. In a modal, cancel pending confirmation first, then close on a subsequent Esc. |
| F2 | Explicit advanced-shell handoff. |
| d in semantic feedback | Toggle Details with full retained evidence. |

The separate pure Script scratch workflow remains Ctrl-S to publish, then F5 to
run the committed revision. App Run does not silently fall back to that runner.
Keyboard shortcuts depend on terminal delivery; Shift-F5 is the advertised Check
binding rather than relying on a terminal distinguishing shifted control letters.

## Source and authority invariants

- Run/Check reuse the editor's existing checked-baseline publication path; they do
  not copy text into an unrelated source. Publication conflicts retain the draft.
- The App document identity is checked after publication. Other dirty drafts still
  block execution, including parked/unrelated drafts. There is no implicit Save.
- Run requires exactly one eligible zero-argument, read-only document
  `CandidateEvaluation`. Missing or ambiguous eligibility is not guessed; use
  explicit inspection. Source review precedes execution, and Enter grants one
  invocation for the captured source/revision through the existing review path.
- Publication, discovery and evaluation evidence do not themselves grant native
  authority or constitute semantic approval. Stale-source and execution-policy
  checks remain part of the existing invocation boundary.
- Semantic feedback presents the provider summary, limited to 240 characters,
  with outcome and current/stale status. Details retain full evidence, including
  native values. Scope does not parse provider prose to infer success or results.

Implementation references: [workspace input and lifecycle](../../crates/scope-ide/src/tui/composition.rs),
[actions and checked publication](../../crates/scope-ide/src/tui/composition/actions.rs),
and [semantic review and feedback](../../crates/scope-ide/src/tui/semantic.rs).

## Evidence: observed versus verified

**Human observed, supplied in the H4 request:** inconvenient Function keys,
excessive execution choices/text, confusing focus and surprising quit controls.
This change addresses non-Function-key Open/Run/discovery, result verbosity,
Check focus and App-workspace Escape/quit behavior. No new human session was run.
Automated terminal interaction does not prove discoverability or comfort.

**Code inspected for this documentation update:** shortcut dispatch, checked
publication before review/check, unique-operation rejection, quit guarding and
App-versus-scratch Escape behavior; semantic summary truncation, outcome and
Details presentation. Inspection is not a passing test result.

**Reported automated evidence from the implementation work:** the focused H4
real-launcher PTY passed **1 test in 61.16 seconds**. Build warmth and the breakdown
of build versus execution time were not supplied; this is not a performance
benchmark. Its [PTY scenario](../../crates/scope-ide/tests/pty_h4_launcher.py)
exercises Ctrl-O/S/R/G, exact draft revision delivery after explicit confirmation,
no disk save, App Escape staying in the workspace, dirty-quit cancellation and
second-key discard/terminal restoration. This is a deterministic fixture language,
not a production-provider or human usability claim.

The corresponding [launcher test target](../../crates/scope-ide/tests/downstream_launcher.rs)
contains the fixture. Builds reused root artifacts; the 61.16-second measurement
is test runtime, not a cold-build benchmark. **Full verification is not complete.**

| Exact command | Result |
| --- | --- |
| `cargo test --locked -p scope-ide --features tui --test downstream_launcher h4_shortcuts_through_real_launcher -- --exact` | 1 passed; also passed in the default workspace attempt. |
| `cargo test --locked -p scope-ide --features tui --lib` | 501 passed. |
| `cargo test --locked -p scope-ide --features tui,agent --lib` | 761 passed, including existing source/publication/authorization rejections. |
| `cargo test -p scope-ide --locked --no-default-features` | Passed. |
| `cargo test --locked -p scope-ide --features tui,agent --test configured_proposal_launcher` | 1 passed, 1 intentional child ignored; 88.45 seconds test runtime after dirty-quit expectations were updated. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed after fixing the introduced collapsible-if warning. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo test --workspace --locked` | Failed in `downstream_actions_through_real_launcher`, shell post-submission prompt synchronization. |
| `cargo test --locked -p scope-ide --features tui --test downstream_launcher downstream_actions_through_real_launcher -- --exact` | Still fails in shell synchronization, before quit; cause not established. |
| `cargo test --workspace --all-features --locked` | Failed at configured launcher dirty quit; that target was subsequently corrected and passed, but the broad command was not repeated. |

Commands were bounded at 120 seconds. No broad timeout was repeated. Full's
remaining isolated configurations, all-feature Clippy, docs, standalone/scaffold
and complete terminal/Script closure were not completed. The advanced-shell
failure prevents claiming preserved end-to-end advanced workflows. It may be a
readiness race, but is not established as pre-existing or unrelated.

Documentation verification commands, run separately for these prose/link edits:

```sh
python3 -B -m unittest discover -s .github/scripts -p 'test_*.py' -v
python3 -B .github/scripts/check_markdown_links.py
```

Both documentation checks passed (5 Python unit tests and local links). Follow the
[verification contract](../verification.md) for implementation escalation; these
documentation checks cannot substitute for the required Rust/PTY coverage.

## Decision: Polish narrowly, not completion

The inspected implementation and focused launcher evidence support **Polish
narrowly** as the next step, not a broad redesign or a completion claim:

1. Ctrl-S still does not mean actual Save. Either make a separately reviewed
   persistence change or keep Publish and explicit Save unambiguous in guidance.
2. The existing default composition still includes a technical inspector and
   output panel. The contextual picker is already compact, but a truly minimal
   editor-first composition and portable non-Function-key Check remain unfinished.
3. Complete the required PTY matrix and policy/source-lifecycle regression coverage,
   including Check, rejection/ambiguity, stale-source and legacy/scratch behavior;
   the focused H4 path does not establish that matrix. Full remains outstanding.
4. Obtain human usability observations before claiming that the experience is
   intuitive or successful for users.

## Modified files and provider follow-up

Implementation: `crates/scope-ide/src/tui/composition.rs`,
`crates/scope-ide/src/tui/composition/actions.rs`,
`crates/scope-ide/src/tui/semantic.rs`.
Tests: `crates/scope-ide/src/tui/composition/tests.rs`,
`crates/scope-ide/tests/downstream_launcher.rs`, and PTY scripts
`pty_h4_launcher.py`, `pty_app_document_editor.py`,
`pty_configured_agent_launcher.py`, `pty_downstream_launcher.py`,
`pty_editable_workspace.py`, `pty_source_proposal_launcher.py` in that tests directory.
Documentation: `README.md`, `TUTORIAL.md`, and this report.

Providers should supply concise human-facing evaluation summaries and meaningful
diagnostics through existing typed structures. A verbose-only provider remains
verbose (bounded in the default view); Scope cannot safely extract a scalar from
arbitrary text or JSON. Downstream adoption should wait for the remaining shell
regression investigation, full verification and a human programming session.
No language-specific execution or result handling was added.
