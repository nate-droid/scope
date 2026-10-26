# Bounded host command compositions (SC1)

## Existing boundaries reconciled

- `InteractiveSession::route` still routes non-`:scope` input to the active language. `cli.rs` owns explicit shell dispatch; `ShellCommand` continues to parse legacy shell operations.
- `ReadOnlyCommand` remains the native host extension. Existing `downstream_namespaces` and `invoke_downstream` registrations/callbacks are unchanged. Namespace registration alone still creates no shell alias.
- `ScratchBuffers` owns scratch identities, selection, in-memory text, monotonic source revisions, and the independent semantic-target identity. Its `create`, `open`, `select`, and `bind_semantic_target` methods are reused, not replicated.
- Opening retains workspace path confinement, read authority, language detection, and existing-source behavior. In-memory creation, selection, and editing do not grant disk-write authority. Unregistered scratch language tokens remain supported, as with `scratch new`.
- The existing transactional command editor owns drafts, cancellation, commit validation, and revision changes. Its internal outcome now distinguishes commit from cancellation; ordinary shell editing otherwise behaves as before.
- Explicit `semantic-target`, Scope Script, retained semantic agents, and downstream dispatch remain independently usable. The semantic target is not inferred from selection outside an explicit composition binding step.
- Native downstream callbacks remain trusted code responsible for concrete semantic authorization. Neither registration nor binding supplies semantic permission. Existing authorized operation adapters should be called unchanged from `invoke_downstream`.

## Public contract

`scope_ide::cli` exports `Composition`, `CompositionStep`, `SelectedMetadata`, `CompositionReport`, and `BindingTransition`. Implement `ReadOnlyCommand::compositions()` to return a small, fixed list of registrations. Registrations are copied and validated once at shell entry (64 compositions, 32 steps each, bounded metadata). Names must be explicit, nonreserved namespaces.

A composition is a literal typed list, not a command string, recursive shell evaluator, or general workflow language:

- `Create { language }`: creates/selects an empty source using the scratch owner.
- `Open { path }`: opens/selects using existing workspace authorization and language detection.
- `Select { id }`: selects an existing live identity.
- `Edit`: enters the existing command editor, including host completion. Cancellation/EOF terminates the composition without subsequent binding or dispatch.
- `BindSelected { expected_language }`: resolves and validates the current selection, invokes the existing semantic binding machinery, and pins its actual identity, language, and revision. Binding transitions are printed and included in the report.
- `Downstream { namespace, command, args }`: requires a composition-local binding. Revalidates the current selection and semantic binding against that exact metadata before borrowing the live source for `invoke_downstream`. Missing, switched, closed, or revision-changed sources cannot fall back to an earlier binding. The callback must authorize the operation normally.

`composition_completed(&CompositionReport)` receives immutable metadata on success or editor cancellation, including actual create/open identities (never guessed numeric IDs), final selected metadata, binding transitions, and cancellation status. This callback is observational: it receives no `App`, workspace, mutable source storage, or lifecycle handle. An error stops immediately and does not issue a completion report. Earlier successful steps are not rolled back.

Binding is intentionally observable even when a later host authorization check denies execution. Failed language/selection validation does not change the old binding, but also never dispatches against it.

## Downstream integration / migration

Existing hosts require no changes: the new methods default to empty/no-op. Keep advanced manual commands under `:scope downstream DOMAIN COMMAND` and explicit `:scope semantic-target ID`.

For a selected-source convenience command, register, for example:

```rust
Composition {
    namespace: "demo".into(),
    command: "run-selected".into(),
    steps: vec![
        CompositionStep::BindSelected { expected_language: "text".into() },
        CompositionStep::Downstream {
            namespace: "demo".into(),
            command: "inspect".into(),
            args: vec![],
        },
    ],
}
```

Invoke only as `:scope demo run-selected`. This does not change arbitrary unprefixed input, reserve global `run`/`open`/`edit` aliases, or turn existing downstream namespaces into aliases. Compositions may share a domain name with an existing downstream namespace; the explicit `downstream` route is unchanged. The same shell entry points support buffered/noninteractive input; composition registration does not add a new top-level CLI mode or Script lifecycle operations.

See `crates/scope-ide/examples/bounded_composition.rs` for a complete host using only public APIs, including create/bind/edit, switching actual retained identities, selected dispatch, independent manual binding, and explicit authorization denial:

```text
cargo run -p scope-ide --example bounded_composition
cargo test -p scope-ide --example bounded_composition
```

## Deliberate limitations

- Registrations contain literal operands: there is no dynamic argument expansion, branching, looping, recursive evaluation, or command-text rewriting. Hosts needing dynamic path/identity arguments should use existing manual Scope commands until a concrete integration justifies a typed argument extension.
- Composition editing uses the existing command editor, not keyboard editing. The keyboard editor does not currently expose a commit/cancel result; automatically chaining execution after it would be unsafe. Manual `:scope edit` remains unchanged.
- Cancellation and errors stop later steps but do not undo earlier creation, selection, edits, or binding. An edit-only canceled composition cannot execute anything; a subsequent independent explicit command is a new invocation.
- Source revisions are pinned at binding; editing after binding invalidates dispatch. Put editing before binding when dispatching the edited source. Borrowed native invocation cannot concurrently mutate Scope-owned source through this interface.
- This is a trusted native extension, not a sandbox for malicious host code. Hosts still enforce their semantic operation allowlists and authorization, external effects, and resource limits.
