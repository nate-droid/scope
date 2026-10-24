# Workspace-backed semantic report

This example explicitly binds Script to one read-only workspace document and one
selection-description operation. The standalone Script CLI does not acquire
semantic services from the script filename or `scope.invoke` spelling.

From the repository root (`--offline` requires cached dependencies):

```sh
cargo run -p scope-script --example workspace_semantic --offline -- --allow-read crates/scope-script/examples/semantic-workspace
```

The host requires `--allow-read ROOT`, mounts that root read-only, opens
`example.decl`, and selects its first declaration at zero-based UTF-8 byte range
`0:0..0:15`, excluding the newline. Script cannot choose another file, replace the
selection, supply revision tokens, or grant authority.

## Host wiring

1. `Workspace::read_only` and `load_file_revision` read the confined file and its
   disk revision.
2. `Document::from_revisioned_workspace_file` creates the host-owned document.
   `DocumentSource` borrows that exact document, preserving its tagged edit
   identity, text, and in-memory version. Disk revision is a separate contract.
3. `SemanticToolProvider` binds the example declaration service, document scope,
   selection, controller, and concrete-call source policy.
4. `AgentSession` installs the provider as a tool dispatcher. This does not start
   an agent or call a model.
5. `SemanticHost` exposes the selected read, inserts host-captured source/version
   preconditions, and requires explicit authorization plus the read-effect gate.
6. `Interpreter::eval` runs `crates/scope-script/examples/workspace_semantic.scope`.
   The script discovers the exposed operation, retains its full envelope, prints
   a projected report, and returns the original envelope. The host drains output
   with `take_output`.

The report describes `answer` as an integer-literal binding, retaining partial
analysis, unknown inferred type, and unsupported references. It is not a proof,
full type analysis, or semantic acceptance inferred from interpreter success.
Tool errors and returned rejection statuses remain different outcomes.

## Evidence and limits

`crates/scope-script/tests/workspace_semantic.rs` compares complete Script and
direct-session results and covers metadata, identity, selection, nested payloads,
source/disk preservation, zero proposals, path confinement, denied authority,
stale versions, unexpected arguments, and invalid selections. Run that target with:

```sh
cargo test -p scope-script --offline --test workspace_semantic
```

This is explicit example-level wiring, not automatic attachment to a live IDE or
generic catalog search. Current multi-read embedding is documented in the
[Script guide](scope-script.md#composing-downstream-semantic-reads-si7).
The read-only mount does not prevent external writers; freshness is the bound
in-memory source contract, not atomic disk freshness. Native providers are trusted
Rust code, not sandboxed by effect metadata. Reports may contain source-derived
information and need review before sharing.
