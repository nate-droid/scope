# SI6 — live scratch semantic targets

## Integrated callback

`ScriptRunner` implements `scope_ide::cli::ReadOnlyCommand::invoke_script_with_target`
by forwarding to its inherent public method with the same signature:

```rust
pub fn invoke_script_with_target(
    &mut self,
    source: &ScratchBuffer,
    target: Option<&ScratchBuffer>,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>>
```

The shell dispatches `:scope script run` through this callback. The complete
integration spans upstream scope-ide changes (the trait callback, shell command,
and scratch binding state) and downstream scope-script changes (runner forwarding,
examples, and acceptance tests). This downstream follow-up edits only scope-script;
it consumes the already-landed scope-ide APIs.

* `:scope semantic-target ID` binds an explicit scratch identity independently of
  active scratch selection; `:scope semantic-target none` clears it. With no
  argument, `:scope semantic-target` reports the binding.
* The host resolves source and target live for each invocation and borrows them
  immutably. The runner never retains these references or selects a replacement target.
* Active switching and edits do not renew authority. No App document or runtime
  session is required for scratch-target invocation.

## Exported scratch binding APIs and closed targets

The following methods are exported on `scope_ide::scratch::ScratchBuffers`:

```rust
pub fn bind_semantic_target(&mut self, id: Option<u64>) -> Result<(), ScratchError>
pub fn semantic_target_id(&self) -> Option<u64>
pub fn semantic_target(&self) -> Result<Option<&ScratchBuffer>, ScratchError>
pub fn get(&self, id: u64) -> Result<&ScratchBuffer, ScratchError>
pub fn get_mut(&mut self, id: u64) -> Result<&mut ScratchBuffer, ScratchError>
```

Binding `Some(id)` validates that the buffer is live; a failed bind leaves the
previous binding intact. `None` explicitly clears the binding. The ID accessor
reports the stored identity even after that buffer closes. Closing a target retains
its binding: `semantic_target()` then returns `Err(ScratchError::UnknownBuffer(id))`,
not `Ok(None)`. Consequently shell execution fails resolution before invoking the
runner, rather than falling back to script-only execution or the active scratch.
With no binding, resolution is `Ok(None)`. `get`/`get_mut` resolve explicit IDs
without changing active selection. These are host APIs, not script capabilities.

## Runner behavior and compatibility

`Some(target)` passes the actual `&ScratchBuffer` as `SourceTarget` to the existing
`SemanticToolProvider`. It does not clone/relabel it as a document. Selection is
whole-target `TextRange` derived by `TextBuffer::range_for_span`, including trailing
newlines and correct UTF-8 coordinates. Service language mismatches fail setup.
The example declaration operation requires a single complete line, so use a target
without a trailing newline for successful example results.

**No-target compatibility is unchanged:** `None` delegates to the existing
`ReadOnlyCommand::invoke_script`, which is the default unsupported error for
ScriptRunner. Clearing a binding does not enable pure script-only execution or
fall back to an App document. The shell test verifies the unsupported result and
that no additional record or semantic dispatch is produced.

App `run(source, document, selection, operation)` retains its action check, range,
document clone/adapter behavior and provenance. Both paths share evaluation and
output formatting. Setup errors return directly; interpreter errors remain in owned
records and are rendered as interpreter failure, not mistaken for semantic success.

Existing gates remain: explicit `Some(true)` authorization and read-only semantic
effect, plus provider language/identity/revision/text checks. Nothing grants editing,
saving, workspace access or runtime ownership. Each invocation creates a fresh
provider/session/interpreter. Records own snapshots, envelopes and printed output,
never live references. Immutable scratch borrows prevent safe mutation during a
call. Tests separately exercise live freshness between calls and stale revision/text
preconditions through the same provider/host tooling; no stale-check bypass or
artificial concurrent scratch mutation is introduced.

## Normal downstream CLI

`examples/semantic_target_cli.rs` uses the standard startup API:

```rust
pub fn run_with_registry_and_command(
    args: impl IntoIterator<Item = String>,
    registry: LanguageRegistry,
    command: &mut dyn ReadOnlyCommand,
) -> Result<(), Box<dyn std::error::Error>>
```

Run from the workspace root:

```sh
cargo run -p scope-script --example semantic_target_cli -- --allow-read --workspace crates/scope-script/examples/semantic-workspace shell
```

The following transcript is covered by the enabled shell integration test:

```text
:scope scratch new example.declarations
:scope scratch replace 1 1 1 1 let answer = 42
:scope semantic-target 0
:scope scratch new scope-script
:scope scratch replace 1 1 1 1 scope.invoke("semantic", "language.example.declarations.describe_selection", {}).value.name
:scope script run
:scope scratch select 0
:scope scratch replace 1 5 1 11 renamed
:scope scratch select 1
:scope script run
:scope semantic-target none
:scope script run
```

Finals are `"answer"`, then `"renamed"`; the third run is unsupported. No save is
performed. This assumes a fresh shell assigning IDs 0 and 1.

## Path DX2 handoff and limits

Path can register its existing static language service in `LanguageRegistry`,
install its callback with `cli::run_with_registry_and_command`, and implement
`ReadOnlyCommand::invoke_script_with_target` to obtain the Script and optional
live language scratch separately. Use `target.evaluation_source_id()` for the
existing language identity, `RevisionId::from_version(target.version())` for its
revision, and `SourceTarget::snapshot()` for the existing host source snapshot.
Do not reconstruct an authoritative scratch from text. The supplied immutable
scratch implements `SourceTarget` and can be passed directly to
`SemanticToolProvider::new`, as in `ScriptRunner`.

The launcher does not sandbox trusted native callbacks. The read-only boundary
comes from immutable source borrows and the absence of App/workspace/mutable
source capabilities; the example additionally installs only a semantic provider,
allows only the configured action, requires explicit authorization, and rejects
non-read effects. Previously captured native host capabilities are neither revoked
nor expanded. No automatic apply, proposal approval, or persistence is added.

There is one explicit binding per shell scratch owner, not general multi-document
routing. IDs are owner-local and must come from that shell's buffer list. Closing
and reopening a file creates a different identity and requires explicit rebinding.
Binding an unavailable ID fails without changing the previous binding; a bound
scratch that closes yields `ScratchError::UnknownBuffer` on invocation, whereas
no binding resolves to `Ok(None)`. Language validation occurs at semantic provider
setup, not selection. The generic callback may implement its own Script-only
behavior; this example semantic runner retains its pre-existing unsupported
no-target behavior. It uses whole-target selection; App `:scope run` still supports
an explicit target range. No Path semantics or Path integration has been added.

## Verified tests

```sh
cargo test -p scope-script --test scratch_runner --offline --locked
cargo test -p scope-script --offline --locked
cargo check -p scope-script --example semantic_target_cli --offline --locked
```

The scratch runner suite passes all 17 tests with none ignored. Both previously
ignored shell-contract tests are enabled and pass through trait-object dispatch.
They cover explicit binding across active switching, unsaved edits, fresh revisions,
clearing the target, denial and wrong language. Existing App-shell tests pass too.

### Opened persisted-file acceptance

`opened_scratch_target_uses_unsaved_edits_without_changing_persisted_file` builds
an App with a `LanguageRegistry` containing the declaration service. The registry
requires `&'static dyn LanguageService`, so the test uses an isolated static
`LazyLock<DeclarationLanguage>`. It writes `example.decl` as `let answer = 42`
without a trailing newline, opens it with `:scope open example.decl`, edits the
opened scratch to `let unsaved = 42`, binds target 0, and switches to a Script scratch.
After the first invocation it switches back, edits to `let changed = 42`, switches
to Script again and reruns. Assertions verify both final values, stable separate
script/target identities, unchanged script revision, advancing target revision,
retained target snapshots, two service dispatches, and byte-for-byte unchanged disk.
This exercises registry-based language inference on open, not `scratch new` with an
explicit language label.

The normal launcher was also verified as a real subprocess, not only the owned
shell harness. After `cargo build -p scope-script --example semantic_target_cli
--offline --locked`, a Python driver created a temporary workspace containing the
same persisted file and piped the opened-file sequence above to:

```text
target/debug/examples/semantic_target_cli --allow-read --workspace TEMP_WORKSPACE shell
```

Observed output:

```text
target: SourceId(0) revision RevisionId(1)
final: "unsaved"
status: interpreter success (see semantic envelopes for semantic status)
target: SourceId(0) revision RevisionId(2)
final: "changed"
status: interpreter success (see semantic envelopes for semantic status)
scope> error: script execution is unsupported by this read-only host command
```

The driver additionally cleared the binding with `:scope semantic-target none`
and ran again to verify the compatibility error above. It asserted process exit 0,
both final values, the unsupported message, and unchanged persisted bytes. This was
a manually executed subprocess acceptance check, not an added automated subprocess
test. The temporary workspace was removed after verification.

Direct callback tests cover retained old snapshots, whole-target selection and
unsupported `None`. The stale-precondition regression captures scratch text/version,
edits the scratch, and independently supplies an old revision or old text to
`SemanticHost` over a fresh provider. Both return `revision_conflict` envelopes
without dispatching to the language service or mutating the target. These semantic
conflicts are envelopes, not interpreter failures.
