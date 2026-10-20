# S9: workspace-backed semantic report

Run from the repository root (offline requires cached dependencies):

```sh
cargo run -p scope-script --example workspace_semantic --offline -- --allow-read crates/scope-script/examples/semantic-workspace
```

The host explicitly grants read access to this root, opens `example.decl`, and selects its first declaration (line 0, columns 0–15, excluding the newline). Without `--allow-read ROOT` it exits before opening a workspace. The script cannot select another file, alter the selection, supply revision tokens, or grant authority.

Successful output is one structured value rendered using the existing Rust debug formatter:

```text
Object({"inferred_type": String("unknown"), "kind": String("integer_literal_binding"), "line": Integer(0), "name": String("answer"), "partial": Boolean(true), "references": String("unsupported"), "status": String("read_only"), "title": String("Workspace declaration report")})
```

The checked-in `crates/scope-script/examples/workspace_semantic.scope` discovers the single host-selected read operation, captures its full result, projects nested fields, constructs a report, and calls `print`. Its final expression returns the original result to the embedding; it does not print it again.

## Existing integration interfaces

The runner uses only existing interfaces:

1. `Workspace::read_only` and `load_file_revision` read the actual confined workspace file and opaque disk revision.
2. `Document::from_revisioned_workspace_file` opens that file with a host-owned `DocumentId`, path, saved state and disk revision.
3. `DocumentSource::new` borrows that same document as a `SourceTarget`. It retains `SourceId::Document`, text and in-memory version; it is not a scratch copy. Disk revision and in-memory version are distinct and neither is substituted for the other.
4. `SemanticToolProvider::new` binds the existing example declaration language, document scope, exact selection, source authorization policy and `SourceEditController`.
5. `AgentSession::builder().provider(...).build()` installs the provider in the existing IDE tool session. No chat or agent lifecycle is started.
6. S8's unchanged `SemanticHost` narrows the catalog to `language.example.declarations.describe_selection`, injects source/version captured from the same target, and calls `AgentSession::invoke_with_call_policy`. Explicit host authorization and read effect checks remain required; discovery is not authorization.
7. `Interpreter::eval` receives the unchanged structured semantic envelope. `take_output` drains the report produced by `print`.

The integration seam is example-level host wiring, not a new dispatcher or registry. No production contracts, parser, runtime, terminal, language implementation, or authorization/session infrastructure changed. The scope passed to the provider identifies the document; it does not itself grant mutation authority. The workspace mount is read-only, only a read tool is exposed, and no proposal acceptance occurs. The runner asserts that the entire document, disk contents/revision and empty proposal list remain unchanged.

## Deterministic evidence

`crates/scope-script/tests/workspace_semantic.rs` writes the checked-in fixture into an isolated temporary workspace. It compares the complete script result with direct IDE `invoke_with_call_policy` output from the same session and target; checks catalog metadata, document identity, selection-dependent span, nested fields and exact presented report; and checks unchanged document/disk state and zero proposals.

Boundary cases cover missing/traversal paths, unavailable operations, missing/denied host authorization, denied source policy, stale in-memory version, unexpected arguments and invalid/partial selections. Tool errors remain distinct from returned semantic status envelopes. Rejected calls do not reach language dispatch except a valid-range partial selection, which the language itself rejects as `invalid_arguments`.

Validation commands:

```sh
cargo test -p scope-script --offline
cargo test -p scope-ide --features tools --offline --lib tool::semantic --quiet
cargo run -p scope-script --example workspace_semantic --offline -- --allow-read crates/scope-script/examples/semantic-workspace
cargo run -p scope-script --example semantic --offline
```

Validation on 2026-10-20: all 51 Scope Script tests passed (48 existing, including the two S8 semantic inspection tests, plus three S9 integration tests); all 25 filtered IDE semantic tests passed; both S9 and S8 examples exited successfully. An initial compile failure in new test integer literals was corrected before these successful runs. Cargo also emits existing duplicate-package warnings from the cached Cell dependency. No unrelated test failures were observed; the full IDE suite was not run.

## Limits and blockers

No upstream interface blocker was found. This is an explicit host-selected, single-document, single-operation demonstration, not automatic attachment to a live IDE or generic catalog searching. The standalone Script CLI still has no semantic host. The minimal existing `.decl` provider performs partial declaration parsing only: references are unsupported and inferred type is unknown, not inferred successfully. There are no Forth-specific assumptions, models, network calls, writes or new language primitives.

The semantic freshness boundary checks the open document snapshot, not an atomic disk snapshot or automatic disk reload. Read-only mounts do not prevent external writers. Native language providers remain trusted Rust code, not sandboxed by read metadata. Reports can contain source-derived information; review them before sharing.
