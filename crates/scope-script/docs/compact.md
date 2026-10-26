# Script reduction host integration

`ScriptRunner` and `WorkbenchRunner` implement the IDE's immutable
`ReadOnlyCommand::reduce_script_snapshot` hook through `reduction::reduce`.
The shared adapter converts each original byte span to an IDE `TextRange`
against the captured text, preserving identity, revision, rule and checked
preconditions. It does not evaluate, discover operations, authorize calls,
record executions, or save files. Unsupported and invalid sources are refusals.

In continuous Script context, `:compact` previews an unevaluated proposal.
`:source-accept ID` applies it in memory to its original owner, even after
switching selection. `:source-reject ID` leaves source unchanged. A revision
change after preview prevents acceptance. Acceptance is not execution evidence;
use `:run` explicitly for fresh whole-snapshot execution.

## Discovered proposal contract and assurance policy

`SourceEditProposal::new` captures the edit target's tagged `SourceId::Scratch`,
committed memory version and exact original text. This is separate from the
language evaluation snapshot's `SourceId` and `RevisionId`; the adapter checks
that evaluation identity, revision and text match the selected committed scratch
before attaching edits. Selection does not become proposal identity.

`SourceEditController::register` rejects language-rejected candidates and checks
any configured validation/task contract before registration. Its default permits
unevaluated proposals. The reducer supplies only rationale and locally checked
preconditions: it supplies neither `ValidationEvidence` nor semantic observations.
A configured controller requiring such evidence therefore displays the diff as
**preview only**, without registering an acceptable proposal. There is no fallback
controller and no weakening of the configured contract.

The existing lifecycle checks Pending status, original target, version, baseline
and ranges before transactional acceptance. Acceptance advances one committed
memory revision and marks source dirty; it does not evaluate or save. Stale
proposals remain inert, without rebasing. Rejection is terminal and leaves source
unchanged. Existing workspace draft blockers and source-closure handling remain
in force. Human command dispatch plus mutable target access is the existing
acceptance authority; it is not semantic authorization, a correctness certificate,
or a separate revocable acceptance policy. There is no new lifecycle or review
acknowledgment state.

## Readability

Empty statements were a source-span convenience, not a requirement of the local
assignment-chain argument. The reducer now removes an immediately adjacent `;`
with each removed assignment. It never scans across whitespace/comments to find
one. Newlines and comments remain exact original bytes; separated or repeated
semicolons may remain. The result must still independently reparse as two
statements. Tests replay original-byte edits, preserve Unicode/CRLF trivia, and
compare independent execution of the supported shapes. This is not a formatter.

## Recommendation and remaining boundary

Expand usability cautiously, not rule coverage yet. This experiment provides a
useful explicit review path for the default unevaluated scratch workflow. Trusted
semantic contracts cannot currently consume the reducer's local proof argument;
full integration there needs a deliberately designed host-owned assurance
contract, not fabricated execution evidence. Automated PTY output is evidence of
command behavior, not a manual user study or a terminal screen-cell emulator.

## Reproduce the terminal checks

```sh
cargo build -p scope-workbench --offline --locked
python3 crates/scope-script/tests/pty_continuous_shell.py -v --transcript
```

`compact-pty-transcript.txt` retains the actual ANSI-stripped terminal reads
(including input redraws) from all six tests. The compact lifecycle test checks
Unicode preview, rejection, cross-selection acceptance, unchanged other buffer,
unsupported reduction, stale acceptance, no retained evaluation and no source
files written (history files are allowed).

## Validation (2026-10-29)

- `cargo test -p scope-script --offline --locked`: 176 passed, 0 failed, 0 ignored.
- `cargo test -p scope-ide --lib --offline --locked`: 393 passed.
- Real workbench continuous PTY suite: 6 passed in 29.359 seconds.
- Workbench build: passed.
- `cargo test -p scope-workbench --offline --locked`: fails in existing
  `buffered_shell::syntax_runtime_and_immutable_errors_preserve_ordered_unicode_prints_and_recover`
  at line 111. Reproduced with `--test buffered_shell` (2 passed, 1 failed):
  output correctly reports `runtime error: assignment requires a mutable binding`,
  but the test's expected diagnostic does not match. Left outside this change's
  scope. Cargo also reports duplicate cached `cell-guest` packages and the
  existing unused `ScratchSessionAdapter::new` warning in the workbench build.

## UX5b validation closure (2026-10-29)

The diagnostic mismatch above was a stale test, not a reducer or workbench
regression. Commit `461a64f` introduced `let mut` and assignment on 2026-10-28;
the older buffered-shell test still expected assignment to be invalid syntax.
The parser now accepts `name = expression`. Execution checks the binding's
mutability before evaluating the right-hand side, returning a runtime error for
immutable `let`. Workbench rendering preserves that interpreter error unchanged.

Reproduced the failure through the actual public binary via `buffered_shell`.
The smallest correction changes only that test: it now distinguishes a real
syntax error from immutable assignment, verifies earlier output survives the
runtime failure, verifies an effectful assignment RHS is not evaluated, and
recovers with a successful mutable reassignment yielding 42. No interpreter,
reducer, proposal, or authorization behavior changed.

Validation using cached dependencies and existing build artifacts on macOS:

- `cargo test --locked --offline -p scope-ide -p scope-script -p scope-workbench -j 1`:
  completed successfully, including integration tests and Workbench terminal PTYs.
  IDE library: 393 passed. Workbench: 10 integration tests passed, including all
  three buffered-shell tests and all three terminal-PTY tests.
- `python3 crates/scope-script/tests/pty_continuous_shell.py -v`:
  all six actual-terminal tests passed (31.310 seconds).
- Tutorial examples were also exercised through the buffered public workbench,
  including error repair, proposal rejection/acceptance, and explicit saving in
  a disposable writable workspace.

Coverage includes ordinary entry, completion, multiline/paste/history, editing,
exact reduction edits and Unicode spans, safe refusals, inert previews,
accept/reject, original scratch ownership, stale rejection, explicit-run-only
effects, zero semantic-provider calls during reduction, and no implicit source
saves. PTY isolation permits history storage; “no saves” means no source saves,
not no history writes. Provider boundary tests use local fixtures; no remote
semantic service or credentials were used.

An initial combined run exceeded a 200-second bound during the IDE launcher
coverage, and concurrent follow-up Cargo commands encountered build-lock waits.
The final serial run with a 600-second bound passed, so there is no outstanding
test failure. Existing duplicate cached Cell-package and dead-code warnings remain.
PTY checks inspect actual terminal reads, not screen-cell rendering or a human
usability study.

Hands-on guide: [An evening with continuous Script](../../../docs/tutorials/script-evening.md).
Important limits remain fresh whole-snapshot replay (including repeated effects
on each explicit run), conservative completion/effect classification, narrow
literal-assignment compaction, no automatic saves, and existing-destination plus
writable-workspace requirements for saving. Configured hosts requiring validation
evidence can keep a reduction preview-only; the local rule supplies no execution
or semantic assurance evidence.
