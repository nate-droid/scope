# W7b: contextual downstream workbench actions through the public launcher

## Contract reconciliation

Starting revision: `3324822c28ee397d365428ba1d32156cc57618e7`.

W7b connects W7's workspace action registration to the ordinary public launcher. The signature of `cli::run_with_registry_and_command` is unchanged. Its workspace branch now calls the host's `#[cfg(feature = "tui")]` hook, `ReadOnlyCommand::downstream_actions(&self) -> Vec<DownstreamAction>`, whose default is empty. It installs each returned action through the existing `WorkspaceTui::register_downstream_action` validation before the terminal run starts. Invalid, duplicate, or excessive registrations fail startup; shell and other non-workspace commands ignore this hook.

The F1 catalog was a closed `WorkspaceAction` enum. `WorkspaceTui` already owned registration, picker presentation, command-bar submission, and retained output. `ReadOnlyCommand` already supplied namespace registration and authorized native downstream dispatch; compositions already supplied `BindSelected` plus revision-checked dispatch. See [bounded compositions](BOUNDED_HOST_COMPOSITIONS.md).

`ScratchBuffers` remains the sole source owner. Scratch identity, evaluation source identity, registered language, monotonically increasing version, and committed text already exist. `TuiContext::selected_source` observes selection independently of `bound_semantic_source`. Neither grants authority. `ScratchEditAccess` and editable pane registration remain the explicit editing boundary. Editor drafts belong to panes; commit validates the baseline and updates memory, cancel drops the draft, and conflicts retain it. Hidden and parked drafts already participate in `draft_blocker`.

Semantic operation adapters still enforce authorization and read/write policy. Source proposals still require explicit review/acceptance; retained evidence cannot authorize edits. Filesystem access remains capability-backed. No changes to these mechanisms or Cell were needed.

## Public registration

Use `scope_ide::tui::{DownstreamAction, WorkspaceAction, WorkspaceTui}`. The `tui::composition` module is private; downstream consumers must use the public re-exports. Hosts using the ordinary launcher return metadata from `downstream_actions`; hosts building their own workspace can still call `WorkspaceTui::register_downstream_action` directly.

The host must separately register its namespace through `ReadOnlyCommand::downstream_namespaces` and implement `invoke_downstream`. Registration does not install a namespace or authorize an operation. IDs must be unique; at most 64 actions and 32 arguments per action are accepted. IDs, namespaces, and commands are nonempty, at most 64 bytes, and use lowercase ASCII letters, digits, `-`, or `_` (namespace registration also applies its existing leading-letter rule). Labels are nonempty and at most 256 bytes; languages are nonempty and at most 64 bytes. Each argument is nonempty, at most 1024 bytes, and contains no whitespace, quote, or backslash. Arguments are fixed literal presets using the existing whitespace-only downstream syntax, without quoting or expansion. There is no dynamic input, interpolation, or new prompt/form API.

### Complete public consumer example

This executable requires the `scope-ide` dependency with feature `tui` enabled. It registers a language and a trusted read-only host, then uses the unchanged launcher. Open a `.consumer` scratch in the workspace and select its F1 inspect action after committing or canceling any draft.

```rust
use scope_ide::{
    cli::{self, DownstreamError, DownstreamOutcome, ReadOnlyCommand},
    document::Document,
    language::{
        LanguageAnalysis, LanguageDocument, LanguagePath, LanguageRegistry, LanguageService,
    },
    scratch::ScratchBuffer,
    text::TextRange,
    tui::DownstreamAction,
};
use std::io::Write;

struct ConsumerLanguage;

impl LanguageService for ConsumerLanguage {
    fn language_id(&self) -> &'static str {
        "consumer-language"
    }

    fn language_name(&self) -> &'static str {
        "Consumer language"
    }

    fn matches_path(&self, path: &LanguagePath<'_>) -> bool {
        path.extension() == Some("consumer")
    }

    fn analyze(&self, _: LanguageDocument<'_>) -> LanguageAnalysis {
        LanguageAnalysis::default()
    }
}

static CONSUMER_LANGUAGE: ConsumerLanguage = ConsumerLanguage;

struct Consumer;

impl ReadOnlyCommand for Consumer {
    fn downstream_namespaces(&self) -> &[&str] {
        &["consumer"]
    }

    fn downstream_actions(&self) -> Vec<DownstreamAction> {
        vec![DownstreamAction {
            id: "inspect".into(),
            label: "Consumer inspect committed text".into(),
            language: "consumer-language".into(),
            namespace: "consumer".into(),
            command: "inspect".into(),
            args: vec!["literal-argument".into()],
        }]
    }

    fn invoke_downstream(
        &mut self,
        namespace: &str,
        command: &str,
        args: &[&str],
        target: Option<&ScratchBuffer>,
        output: &mut dyn Write,
    ) -> Result<DownstreamOutcome, DownstreamError> {
        if (namespace, command, args)
            != ("consumer", "inspect", &["literal-argument"][..])
        {
            return Ok(DownstreamOutcome::Unhandled);
        }
        let Some(target) = target else {
            return Ok(DownstreamOutcome::Unhandled);
        };
        if target.language_id() != "consumer-language" {
            return Ok(DownstreamOutcome::Unhandled);
        }
        // This host permits only inspection of the committed live-memory source.
        writeln!(output, "Committed consumer source: {}", target.text())
            .map_err(|error| DownstreamError::new("output", error.to_string()))?;
        Ok(DownstreamOutcome::Handled)
    }

    fn invoke(
        &mut self,
        _: &Document,
        _: TextRange,
        _: &str,
        _: &mut dyn Write,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("document operations are unsupported by this host".into())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut consumer = Consumer;
    cli::run_with_registry_and_command(
        ["workspace", "."].map(str::to_owned),
        LanguageRegistry::builtins().with_language(&CONSUMER_LANGUAGE),
        &mut consumer,
    )
}
```

The language ID drives contextual discovery; the namespace drives downstream dispatch. They need not be identical. The host above explicitly limits its operation to inspection; a real domain host must still apply its own authorization and existing semantic adapters.

Discovery compares only the selected scratch's language to registration metadata. It performs no semantic operation. Built-ins remain in the same catalog. Each discovered `WorkspaceAction::Downstream` token carries scratch ID, evaluation source ID, language, and version. Keeping a token and invoking it after selection, revision, language, or closure changes fails before binding or callback. Equal text in another scratch is not equal identity.

Invocation rechecks availability and all dirty drafts, explicitly binds the pinned selected scratch using `ScratchBuffers::bind_semantic_target`, and submits one normal `:scope downstream ...` command. **This intentionally changes the independent semantic target**, just like `BindSelected`; it is not a temporary or implicit fallback. Namespace validation and host authorization still apply, including denial after binding. The callback receives an immutable live-memory borrow, pinning committed text and revision for the synchronous invocation. It receives no editor, mutable source owner, filesystem, or new semantic capability.

## Output and evidence

Human output uses the existing retained command transcript (F1 Show command transcript). An attribution header records scratch, evaluation identity, language, and committed revision, explicitly as historical attribution rather than semantic approval. Shell diagnostics and host errors remain normal retained output. A successfully submitted command is not a claim about the domain outcome.

Structured evidence remains host-owned through existing downstream semantic reports, and can be exposed by another registered evidence command. Scope does not parse or reclassify domain reports. This milestone does **not** add a generic structured-evidence envelope to the workspace transcript or merge downstream results into Script result history. Downstream hosts must retain the original source identity/revision with owned evidence, and label delayed results historical when stale. Existing semantic mutation/proposal adapters must still validate the live revision; evidence alone is never mutation authority.

## Draft safety across F1 and F4

Both action dispatch and F4 `:scope downstream ...`, registered composition commands, and `:scope script run` reject while any registered pane reports a dirty draft. The guard runs before composition creation/selection/binding. The existing message identifies blocking drafts and commit/cancel interactions. No automatic commit or draft evaluation occurs. Read-only help/evidence downstream commands are conservatively blocked too.

The standalone shell and `WorkspaceCommandOutput` cannot inspect pane drafts: they do not own panes. Hosts invoking those lower-level APIs outside `WorkspaceTui` remain responsible for their own draft boundary. F2 handoff already guards dirty drafts. Custom panes must accurately implement the existing dirty-draft reporting contract.

## Adoption and verification

Path or Hilb can register one shortcut per language-specific command, leaving mathematical/scientific behavior in their existing `invoke_downstream` implementation and authorized operation adapters. For example use language/namespace `path` and command `goals`, or the corresponding Hilb operation. Do not add a new semantic callback. Register an evidence shortcut separately if useful.

Literal arguments support bounded fixed presets. For variable candidates use F4 with the existing command form after commit/cancel; there is no new prompt/form framework. Selected-source convenience commands should retain the existing `BindSelected` composition pattern. Raw downstream commands still deliberately address the independent semantic binding, not selection.

The test-local `DownstreamActionHost` and downstream action tests in `crates/scope-ide/src/tui/composition/tests.rs` demonstrate discovery, invocation, retained output, authorization denial, revision/selection/closure invalidation, dirty drafts, and coexistence with built-ins. Existing composition, retained semantic, proposal, and editor tests continue to exercise their original guards.

W7's recorded validation was `cargo test -p scope-ide --features tui --lib` (438 tests). W7b adds launcher registration validation in `crates/scope-ide/src/cli/workspace_ui.rs` and an external-consumer PTY integration test in `crates/scope-ide/tests/downstream_launcher.rs`, driven by `crates/scope-ide/tests/pty_downstream_launcher.py`. The latter uses only public imports and the ordinary `run_with_registry_and_command` launcher, not direct workspace registration, to exercise real F1 dispatch. It requires Unix, Python 3, and PTY support; run it with `cargo test -p scope-ide --features tui --test downstream_launcher`. Its ignored `launcher_child` is invoked by the PTY driver, not directly.

Native callbacks remain trusted synchronous code, not sandboxed plugins; asynchronous evidence freshness and domain classifications remain downstream responsibilities.
