# Scope Script SS4-B — Downstream Path integration

SS4-B is implemented in the sibling Path repository, not Scope core. The full integration report is **Path: `docs/archive/SCOPE_SCRIPT_SS4_B_RESULTS.md`**, including the exact runnable Script, ownership/authorization contract, bounds, tests and limitations.

## Placement decision

Inspection covered `ScriptRunner`/`RunRecord`, `SemanticHost`, `SemanticToolProvider`, `AgentSession`, source targets/snapshots, scratch ownership/revisions, `ReadOnlyCommand::invoke_source`, and semantic/operation registry policies. Existing lower-level APIs suffice. `ScriptRunner` installs one action against an App snapshot; it is not an injectable two-trial host. Widening it or `SemanticHost` is unnecessary.

Path's existing `scope-script` and `scope-ide` dev-dependencies support a downstream `ComparisonHost`, exposing only:

```text
scope.invoke("path_experiment", "compare_candidates", {})
```

Rust configures exactly two candidates, `refl x` and `x`, against goal 0 of one captured reflexivity baseline. Fresh trial owners/providers/sessions/semantic hosts preserve existing authorization and freshness checks. Both full Path envelopes are returned with owner-qualified provenance; host failure categories remain separate from Path classifications. Dispatches are capped at two per host lifetime, with one bounded retained comparison. No original source edit, proposal acceptance, filesystem write, or model invocation is exposed.

## Implementation in Path

- `crates/path-scope/examples/support/comparison.rs`
- `crates/path-scope/examples/scope_compare.rs`
- `crates/path-scope/examples/compare_candidates.scope`
- `crates/path-scope/tests/comparison_host.rs`

Run from Path:

```sh
cargo run -p path-scope --example scope_compare --locked --offline
```

The real Script conditionally reports the documented `Completed` and `RejectedCandidate` classifications, guards host failures, prints the structured comparison, and retains both original envelopes. The full Script is reproduced in Path's report.

## Validation

- Scope: `cargo test -p scope-script --all-targets --locked --offline` — passed; no Scope implementation or SS3 tests changed.
- Path: final focused integration suite — 10 passing tests; runnable example passed. Final `cargo test -p path-scope -p path-syntax -p path-kernel --locked --offline` — passed after all refinements.

No upstream API request is required. Remaining limitations include synchronous execution without hard deadlines/cancellation, ephemeral owner tokens, absence of typed output schemas, existing projection limits, and no installation into Scope's interactive shell. See the full Path report for failure policy and retention details.
