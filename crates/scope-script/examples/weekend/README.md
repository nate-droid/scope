# A weekend of little programs

Start with the [hands-on tutorial](../../../../docs/tutorials/SCOPE_SCRIPT_TUTORIAL.md).
Run commands from the repository root. Install the pinned Rust toolchain and fetch
Cargo dependencies once; the commands below then work offline. No API key or model
service is needed. Examples have no comments because Script doesn't support them.

## Pure programs

```sh
cargo run -p scope-script --locked --offline -- --output summary crates/scope-script/examples/weekend/hello.scope
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/numbers.scope
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/control-flow.scope
cargo run -p scope-script --locked --offline -- crates/scope-script/examples/weekend/little-calculator.scope
```

Expected values: greeting and final `5`; total `36` and integer mean `12`;
a higher hint with distance `5` then `false`; quotient `4` and remainder `5`.
File evaluations are fresh. Default output shows prints only; use `--output summary`
for the final value or `--output json` for a complete execution record.

## Explicit read-only hosts

```sh
cargo run -p scope-script --example workspace --locked --offline -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/weekend/workspace-report.scope
cargo run -p scope-script --example workspace_semantic --locked --offline -- --allow-read crates/scope-script/examples/semantic-workspace crates/scope-script/examples/weekend/semantic-investigator.scope
```

The workspace report uses the existing downstream Markdown batch adapter: two
documents, two pending lines, then `release.md` details. It deliberately indexes
the second document in the supplied sorted fixture, not an arbitrary workspace.

The semantic investigator uses the existing declaration fixture's selected
`answer` binding, returning partial analysis with unknown type and unsupported
references. Neither host installs Forth. Both print Rust-debug-style values.

These host-specific programs cannot run successfully in the ordinary no-host
CLI: its catalog is empty. Discovery does not grant authority; `--allow-read`
belongs to these hosts, not the language or standalone runner. Scripts neither
save source nor modify their targets. Keep fixture edits in personal copies.
