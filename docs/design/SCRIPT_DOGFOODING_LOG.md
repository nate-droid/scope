# Scope Script dogfooding log

Record actual local attempts, not hypothetical defects. This is a lightweight,
append-only feedback mechanism, not a roadmap or commitment to expand the language.
Do not paste private workspace content, credentials, or absolute personal paths.

## Entry template

- Date / context:
- Task attempted:
- Existing interface used (include reproducible command and selected fixture/root):
- Result:
- Friction encountered (observed; write “none” if none):
- Workaround, if any:
- Candidate improvement (proposal, not an observed fact):
- Ownership: Scope Script / Scope / Cell; explain uncertainty if needed.

## 2026-10-19 — S7 repository review and fixture runs

### Loading a daily-use script

- Task attempted: determine how to launch a checked-in script from a terminal.
- Existing interface used: reviewed `crates/scope-script/src/main.rs`, the Scope
  Script README, and `examples/workspace.rs` before implementation. The main
  entry point unconditionally launched the REPL; the workspace runner evaluated
  only its compiled-in `SCRIPT`.
- Result: no existing terminal argument selected a script file.
- Friction encountered: trying another workspace script required editing the Rust
  embedding example or supplying source through a custom host.
- Workaround, if any: the existing embedded S6 script remained runnable.
- Candidate improvement: local file execution using the existing interpreter.
  Implemented in S7: standalone `scope-script SCRIPT` (no authority), and
  `workspace --allow-read ROOT [SCRIPT]` (existing read-only host).
- Ownership: Scope Script; no Scope or Cell API change was needed.

### Markdown inventory and pending-work review

- Task attempted: list documents/revisions and review pending work in the checked-in
  handoff workspace using two separate scripts.
- Existing interface used, from the repository root:

  ```sh
  cargo run -p scope-script --example workspace --offline -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/markdown-inventory.scope
  cargo run -p scope-script --example workspace --offline -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/pending-review.scope
  ```

- Result: both completed successfully. Inventory returned two document identities;
  review reported two pending lines and `needs_follow_up: true`.
- Friction encountered: structured Rust debug output is verbose (nested enum names
  and indentation), though values and pending counts are readable.
- Workaround, if any: use the inventory script when full document context is not needed.
- Candidate improvement: consider a presentation-only formatter after more daily-use
  feedback; not implemented in S7 and not a reason to add language features.
- Ownership: Scope Script presentation; underlying identity and revision semantics
  remain Scope-owned.

### Launch without authorization

- Task attempted: run workspace inspection without an explicit read grant.
- Existing interface used:

  ```sh
  cargo run -p scope-script --example workspace --offline -- crates/scope-script/examples/handoff-workspace
  ```

- Result: exit status 1; stderr explained that `--allow-read ROOT` is required and
  displayed usage. Authorization was not inferred from the path.
- Friction encountered: none in the revised runner's denial message.
- Workaround, if any: rerun with the explicit grant only for the intended root.
- Candidate improvement: none established by this attempt.
- Ownership: Scope Script CLI messaging; Scope authorization must remain intact.
