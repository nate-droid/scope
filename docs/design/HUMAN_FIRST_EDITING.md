# UX1: human editing over the existing scratch owner

## Architecture findings

No replacement editor is needed. The normal `scope` binary in
`crates/scope-workbench/src/main.rs` installs `WorkbenchRunner` and enters the IDE
CLI. `workspace .` uses the existing retained terminal composition; the default
shell also offers `:scope edit`.

Reusable layers:

- `crates/scope-terminal/document.rs`: language-neutral `DocumentEditor` and
  `apply_document_event`, shared by the inline raw editor and retained pane.
  Handles multiline insertion/paste, scalar-safe Left/Right, preferred-column
  Up/Down, Home/End, Backspace/Delete, Enter/Tab, accept/cancel. The command editor
  remains available with `:scope edit --commands` and for redirected input.
- `crates/scope-ide/src/tui/scratch_editor.rs`: pane-local working text, cursor,
  preferred column, parked drafts, viewport tracking, and checked publication via
  `ScratchEditAccess`. No filesystem or execution capability.
- `crates/scope-ide/src/scratch.rs`: sole owner of committed scratch text, identity,
  version, dirty state, and destination baseline. Selection and semantic binding
  are separate. Saving uses existing workspace authority and destination checks.
- `crates/scope-ide/src/proposal.rs` and `source_edit.rs`: existing proposal
  registration/review/acceptance. Acceptance checks pending status, target,
  version, exact original text, and edit validity; configured validation evidence
  stays attached to its exact candidate. Acceptance is one in-memory revision,
  not an evaluation or a file save.
- `crates/scope-ide/src/tui/composition/actions.rs`: F1 discovery, new Script,
  explicit commit, F5 direct run, retained attributed results and history.
  `composition.rs` guards command execution and proposal acceptance against dirty
  editor drafts, including parked and unrelated drafts.
- `crates/scope-script/src/workbench.rs`: existing pure Script execution adapter;
  this change adds no interpreter or language behavior.

The main loop already worked. The concrete integration defect was that even a
**clean** pane draft kept its old source/baseline after committed changes. It could
show a conflict despite having no unpublished human text, or start editing stale
source after an accepted proposal. Creation from output also left focus in output,
requiring an extra discovery step before typing.

## Editing/publication model

1. Keystrokes affect temporary pane-local working text only.
2. Ctrl-S checks the selected/bound target, globally distinct evaluation identity,
   revision, and exact baseline text. If changed, it publishes one full-document
   edit to the same scratch. Unchanged text adds no revision.
3. Any mismatch retains dirty working text and reports conflict. There is no
   implicit merge, cancellation, rebase, save, or evaluation.
4. Clean working text follows committed changes on redraw, and refreshes its
   baseline before input. Its cursor is clamped to a valid UTF-8 boundary; preferred
   vertical column is reset. Dirty working text never follows external changes.
5. F5 explicitly runs committed selected Script only. All dirty drafts still
   block execution. Results retain captured identity, revision, source and output;
   interpreter success is not semantic validation.
6. Proposal acceptance still blocks on any dirty pane draft. After publication,
   existing proposals must pass the unchanged freshness/acceptance checks. Clean
   panes can then follow accepted source without requiring a manual cancellation.
7. File saving remains a separate explicit operation under existing authority.

No scratch registry, revision store, proposal lifecycle, editor plugin mechanism,
filesystem grant, or execution privilege was added. New Script focuses the first
visible editable instance; it does not make read-only instances editable.

## Reproduction and evidence

See `TUTORIAL.md`, **Interactive workspace**, for the hands-on create → type →
Ctrl-S → F5 → inspect → correct → Ctrl-S → F5 loop, error recovery, conflicts,
selection, retained history, and explicit saving.

Run the actual normal executable in a Unix PTY (Python 3 stdlib only):

```sh
cargo test --locked -p scope-workbench --test terminal_pty workspace_end_to_end_pty
```

`crates/scope-workbench/tests/pty_workspace.py` drives the real launcher with key
sequences, including creation from output, Unicode source, interpreter success,
runtime failure, parse failure, correction/rerun, revision-attributed history,
dirty-run blocking, shell handoff and terminal restoration. It is not a mock host
or a renderer-only harness. To see its walkthrough summary directly after building:

```sh
python3 -B crates/scope-workbench/tests/pty_workspace.py --binary target/debug/scope
```

Boundary tests:

```sh
cargo test --locked -p scope-ide --features tui --lib
cargo test --locked -p scope-ide --features tui --test source_edit_controller
cargo test --locked -p scope-ide --features 'tui agent' --test retained_proposal_bridge
cargo test --locked -p scope-terminal
```

The pane tests cover clean baseline refresh with a cursor inside the replacement
Unicode scalar, no publication on typing, same identity/one revision on publish,
stale dirty commit preservation, cancellation, parked drafts and owner mismatch.
Composition tests cover new-Script focus without semantic binding/execution and
acceptance blocking for active, parked and unrelated dirty drafts. Existing
controller/bridge tests retain proposal freshness and acceptance coverage.

### Observed interactive PTY walkthrough (2026-10-29)

Executed the normal binary, not a test-only editor. This was automated interaction
with a real PTY, not a claim of manual human testing:

| Step | Observed result |
| --- | --- |
| Focus output; F1 New Script | Selected scratch `1`, source `SourceId(0)`, editor focused |
| Type `print("first雪"); print("second"); 42` | Dirty draft blocks Run; typing does not publish |
| Ctrl-S, F5 | Revision `1`, ordered prints, final `42`, interpreter success |
| Replace with `print("before failure"); missing`; Ctrl-S, F5 | Revision `2`, retained print and unknown-binding failure |
| Inspect earlier result | Revision `1` still yields `42`, marked older committed revision |
| Replace with `print(`; Ctrl-S, F5 | Revision `3`, parse failure with location, no final value |
| Correct to `print("recovered"); 43`; Ctrl-S, F5 | Revision `4`, final `43`, interpreter success |
| Resize, return to editor, quit | Same selection, semantic target unbound, terminal restored |

The test explicitly asserts the same source identity across all four results,
exactly revisions `1`–`4`, no filesystem output, and zero shell commands in this
loop. Captured summary:

```text
Direct PTY evidence: commands=0, picker actions=6, scratch=1, source=['0'], retained results=4, observed script revisions=['1', '2', '3', '4'], alt-screen enters=1, leaves=1
Ran 3 tests
OK
```

Validation also passed: 456 IDE library tests, a subsequent 59-test targeted TUI
run after the render assertion was added, 8 source-edit controller tests, 3
agent-enabled retained proposal bridge tests, and 29 terminal tests. The broad
IDE integration command timed out in `source_proposal_launcher`; no full-suite
pass is claimed.

## Remaining limitations

### Ergonomic

- Ctrl-S means publish to scratch, **not save to disk**. F5 deliberately does not
  publish implicitly. This is a two-key loop, with no protocol IDs during typing.
- No selection, undo/redo, mouse editing, syntax highlighting, LSP or modal keys.
  Navigation is Unicode-scalar based, not grapheme-cluster based.
- Long pane titles can truncate at narrow widths. F1 help and the tutorial provide
  the full shortcuts. Fresh snapshots show a cursor after the first editor input.
- Opening/selecting files and explicit filesystem saves still use existing command
  routes; they have not been given a second picker or persistence subsystem.

### Architectural boundaries

- Dirty drafts conservatively block all runs and proposal acceptance, even when
  unrelated. There is no automatic conflict merge; retain/cancel/publish is explicit.
- Lower-level integrations invoking proposal acceptance directly must enforce their
  own unpublished-draft guard. Pane draft protection belongs to the retained host;
  the scratch owner cannot inspect arbitrary downstream UI working text.
- Drafts are in-memory, not crash-recovery storage. Saving remains subject to
  existing workspace authorization and destination freshness checks.
