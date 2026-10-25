# SC1 — Namespaced downstream shell commands

## Checked baseline and investigation

Scope baseline: `95742a28e0f5a45197d540fbeda9b8bed3480a0b`
(`working on some simple tutorials`), initially clean. Sibling Hilb was inspected
read-only at HEAD `258e355426d5b502c3edb14baf2cc3b77e2eb88d`; its working-tree
`src/launcher.rs`, `docs/design/U1_SCOPE_REQUEST.md`, and `U2_RESULTS.md` were
reviewed. HEAD alone does not describe all sibling working-tree milestone code.
No Hilb files were changed or Hilb interactive validation claimed.

The existing public composition APIs are `cli::run_with_registry_and_command`
and `cli::run_with_session_and_command`, accepting a retained
`&mut dyn ReadOnlyCommand`. Before SC1 its callbacks included `invoke`,
`invoke_source`, `invoke_script`, `invoke_script_with_target`, and
`complete_source`. Host/run operations require an App document; Script receives
selected scratch and independently bound semantic scratch. Neither supplied a
namespace registration or shell alias against the independent target. The fixed
shell parser rejected unknown commands; source completion is not command routing.
Existing extension points therefore supplied ownership and authorization
composition, but not the requested interaction boundary.

`ScratchBuffers` owns live buffers, selection and a separate semantic-target ID.
`ScratchBuffer` exposes `evaluation_source_id`, `version`, and exact `text`.
The existing immutable `SourceTarget` implementation also supplies snapshots.
Changing selection does not rebind the semantic target. A closed explicit target
remains a failed resolution, not an invitation to select another source.
`ScriptRunner::with_operations` carries explicit per-operation bindings;
`SemanticHost` and `SemanticToolProvider` enforce authorized Read invocation.
There was no reason to replace these mechanisms.

## Public contract and shell integration

Two default methods extend `ReadOnlyCommand`:

```rust,ignore
fn downstream_namespaces(&self) -> &[&str]; // default []
fn invoke_downstream(
    &mut self,
    namespace: &str,
    command: &str,
    args: &[&str],
    semantic_target: Option<&ScratchBuffer>,
    output: &mut dyn Write,
) -> Result<DownstreamOutcome, DownstreamError>; // default Unhandled
```

`cli::DownstreamOutcome` is `Handled | Unhandled`.
`cli::DownstreamError` implements `Error` and has public String `code` and
`message` fields plus `new(code, message)`.

Shell syntax is `:scope downstream NAME COMMAND ARGS`.
`:scope downstream` lists namespaces; normal help advertises it. Registration is
snapshotted/validated before input, with at most 64 names, each 1–64 ASCII bytes
matching `[a-z][a-z0-9_-]*`. Duplicate and invalid names fail startup with
structured errors. Names are local metadata on the installed host, not a global
registry. A namespace named `session` cannot shadow `:scope session`.

Arguments are Unicode whitespace-delimited borrowed tokens. Quotes and
backslashes are explicitly rejected, not silently split as apparent quoted
values. Dollar signs are literal; no escaping or expansion occurs. There is no
expression parser. Empty command input and unknown namespaces have specific
errors; unhandled callbacks become `unknown_command`. Callback errors print their
code/message and the shell continues. The namespace cannot intercept ordinary
source, bare commands, or unknown built-ins. No terminal reader, completion,
history, paste, or line-editor implementation changed.

## Source and authority

Dispatch resolves only `ScratchBuffers::semantic_target()` and supplies its
immutable invocation-local borrow. Identity, version and unsaved text are pinned
together for that call. Selected Script, App document, filenames and retained
execution records are not source-resolution fallbacks. Unbound is `None`; hosts
can require a target. A closed explicit binding fails with `target_unavailable`
before callback invocation. Rebinding is visible on the next call.

There is no mutable source interface, persistent source registry, Scope session
store, filesystem capability, or execution permit in this hook. The host remains
trusted native code, as it was for existing callbacks; Scope cannot sandbox a
host that deliberately ignores its own policy. Registration and inspection grant
no semantic authority. Hosts must reuse their authorized invocation path.

The complete runnable public consumer is
`crates/scope-script/examples/downstream_shell.rs`, tested by
`crates/scope-script/tests/downstream_shell.rs`. It retains a `ScriptRunner` and
selection counter, captures arguments/snapshots, writes through the shell writer,
and maps fixed commands to fixed trusted Scripts. Local Script buffers are
invocation-local controllers, never replacement authoritative semantic sources.
The same runner handles ordinary `:scope script run`; tests compare permitted
and denied aliases against original Script results/outcomes and verify denied
native operations never dispatch. Arguments are not interpolated into code.
Interpreter errors remain in existing runner records/output, and semantic domain
errors still require envelope inspection; SC1 does not redefine these layers.

Hilb's inspected launcher can implement these methods by mapping commands to its
existing actions and routing through its current `invoke_script_with_target`.
That path binds `SourceSnapshot` to its language service and constructs the same
allow-list using `allow_read`. The language service retains run/boundary state;
Scope must not recreate that service or authorize direct native calls instead.
No extra authorized-invocation API was necessary.

## Alternatives rejected and compatibility

- Bare `:hilb` interception or a generic unknown-command fallback: unnecessary
  precedence/source-language ambiguity. A dedicated `:scope` branch is explicit.
- A second command trait, launcher registry, or global plugin system: the existing
  retained host callback already supplies composition and state ownership.
- Passing selected Script alongside the target: unnecessary for an alias and
  encourages confusing the two owners.
- Direct semantic execution API for aliases: duplicates existing authorization.
- Persistent source snapshots/shared mutable registry: live immutable borrows
  already provide consistency.
- General shell quoting/expression syntax: unnecessary expansion of this scope.

Existing implementors inherit empty registration and Unhandled behavior. Existing
launcher signatures and built-ins are unchanged. Only the new explicit
`downstream` branch and help text are added. Namespace metadata is fixed for a
shell lifetime; dispatch is synchronous and native callbacks cannot be cancelled.
Discovery lists namespaces, not individual commands; hosts may provide their own
help command. Structured callback errors are displayed, not persisted as a new
machine protocol. Native output remains the host's terminal-safety responsibility.

## Validation

Executed successfully:

- `cargo test -p scope-ide --lib cli::` (112 tests after final lifecycle additions).
- `cargo test -p scope-script` (unit, integration and doc tests).
- `cargo test -p scope-terminal` (29 terminal unit tests).
- `cargo run -p scope-script --example downstream_shell`.
- `cargo fmt --all -- --check`.
- `cargo clippy -p scope-ide -p scope-script --all-targets --no-deps`.

Clippy completes with existing warnings in unrelated semantic/interpreter,
example-support and test code; this is not a warning-free workspace claim. Cargo
also reports duplicate vendored Cell packages in the cached git dependency.
Dependencies were available locally; no dependency changes were needed.

New real buffered-shell lifecycle tests cover dispatch, arguments, unknown and
malformed input, duplicate/bounded registrations, built-in-named namespaces,
no host installed, default legacy callbacks, unsaved edits/revisions, independent
Script selection, rebinding, source closure, unbound targets, repeated retained
state, permission parity, and callback failure/recovery. These exercise the public
shell launcher, not merely callback calls. Existing terminal tests cover history,
completion, paste and editing. No new PTY test or manual interactive validation
was performed; buffered-shell and terminal-unit evidence should not be described
as such.

## Final architectural review

**A. Convenient commands without replacing the shell?** Yes, through explicit
`:scope downstream hilb next`-style routing on the existing launcher callback.

**B. Exact independently bound semantic source?** Yes: immutable live scratch
identity/revision/text, no selected-Script or document fallback.

**C. Ownership and authorization unchanged?** Yes. Mutation stays with the source
owner; the hook supplies no execution grant. The example reuses and tests the
same authorized Script path. Native hosts remain trusted to preserve policy.

**D. Downstream retained state?** Yes: callback fields or shared downstream
service handles survive submissions. Scope adds no state-store abstraction.

**E. Unnecessary generalization?** No new framework, parser, registry, interpreter
or semantic invocation path. Two default host methods and two small result types.

**F. Sufficient for Hilb run/circuit/next/previous/inspect?** Yes at the composition
boundary: fixed commands can call the existing authorized actions against the
bound snapshot while the Hilb service retains exploration state. Hilb still needs
to implement its adapter and validate action-specific arguments; it was not
modified or compiled against SC1 here.

**G. Future synchronized editor/circuit/debugger TUI?** Still needs a separate
source-view publication/lifecycle contract, downstream session attachment or
transfer, and explicit revision-aware selection/highlighting synchronization.
Source views must distinguish current edits from historical execution evidence.
None of those contracts, panes, layouts, or event buses were implemented in SC1.
