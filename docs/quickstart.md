# Scope quickstart

This is the canonical first-run guide. Scope is one workbench composed of an
editor, a shell, Script execution, and explicitly installed language capabilities.

## Launch

Clone the repository and run in an interactive terminal from its root:

```sh
cargo run
```

`rust-toolchain.toml` selects the compiler. The first build may download the
compiler and dependencies (including Cell); no model server, API key, or Wasm
target is required. Once dependencies are cached, `cargo run --offline` avoids
Cargo downloads. No application network request or authorization grant is made
by startup. The workspace directory must already exist.

The production binary is `scope`, built by `scope-workbench`, the root default
member. No `-p` or feature flag is needed. `cargo run -- --help` and
`cargo run -- --version` describe the interface and version.

## Write and run your first Script

1. In the workbench, press **F1**, search `New Script`, and press Enter.
   This creates a new selected Script scratch and focuses the editor.
2. Type:
   ```scope
   print(2 + 3)
   ```
3. **Ctrl-S** publishes the local human draft to the authoritative scratch in
   memory, advancing its revision. It does not save a file. **Ctrl-C** cancels
   a local draft instead.
4. **F5** runs the selected committed Script. All editor drafts must be committed
   or cancelled first; F5 never silently publishes a draft.
5. Use **F1 → Focus output** to inspect `print 1: 5`, final value, interpreter
   status, and source/revision attribution. Output stays in the workbench.
   **Alt-Left/Right** selects result history; **Page Up/Down** scrolls output.
6. **F3** returns to the previous pane, retaining the same selected source.
   Edit, Ctrl-S, and F5 again. Older results remain attributed to the revision
   actually run, not relabeled as the current draft.

The inspector shows the selected scratch, source identity, committed revision,
and semantic binding. Selection is not authorization or implicit semantic binding.
Each ordinary Script run uses a fresh interpreter with `NoHost`: no filesystem,
network, native providers, or persistent interpreter bindings.

## Open, switch, publish, and save documents

Use **F1** for all document actions; no specialist shell is needed.

- **Open File** opens the existing command bar with `:scope open ` filled in.
  Append a workspace-relative path and press Enter. Opening selects the same
  authoritative scratch collection used by the editor and Script runner.
- **Switch Document: …** selects an open document and focuses the editor.
  Untitled documents are distinguished by their local ID. Dirty editor drafts are
  parked, not discarded, when switching or creating another document.
- **Return to Workbench** focuses output without cancelling your draft.
  **Focus editor** or **F3** returns to editing.
- **Publish editor draft** / **Ctrl-S** updates the in-memory document only.
- **Save** opens `:scope save` in the command bar; press Enter to explicitly
  write the committed document. **Save As** fills `:scope save-as `; append the
  destination path (existing or new). Publish first: saving is blocked while any editor
  draft is dirty, including parked drafts. A failed save retains committed text.

Launch with `cargo run -- --writable workspace` to explicitly enable workspace
writes. Save As can create a new workspace-relative file under an existing authorized
parent directory, or replace an existing authorized file. It checks the observed
destination revision (including absence) and changes the binding only after success. No action grants filesystem authority or bypasses
Cell confinement. Publish and Save deliberately remain separate; the filesystem
revision check is best-effort, not atomic compare-and-swap.

## Collaborate with an agent

**F1 → Ask Agent** fills `:scope document-ask `; append a question and Enter.
This inspects the **committed selected source**, not unpublished editor text.
**F1 → Propose Edit** fills `:scope document-propose `; append the requested
change. Publish or cancel all drafts first. Neither action silently publishes.

The stock launcher has **no default backend**. Compiling `agent` alone does not
install a Script binding or contact a provider. To opt in, start an existing local
OpenAI-compatible provider, then launch from the repository root (replace the
model and port with your provider's settings):

```sh
cargo run --locked --features agent -- \
  --agent-local-binding --agent-model YOUR_LOCAL_MODEL \
  --agent-base-url http://127.0.0.1:1234/v1 \
  --allow-read --allow-edit --writable workspace
```

All flags precede `workspace`. `--agent-local-binding` requires both an explicit
model and an explicit loopback endpoint, plus independent `--allow-read` consent.
`--allow-edit` additionally authorizes proposals; omit it for inspection only.
`--writable` separately authorizes explicit filesystem saves; omit it for in-memory
work. None of these flags grants Script execution/native provider authority.
Startup does not call the model. Ask/propose sends requests to the configured
local provider; inspection can disclose committed source, and a provider may have
its own upstream network, privacy, and cost policies. If required, supply
`--agent-api-key-env VARIABLE_NAME` before the command; do not put keys in source.

These actions use the selected committed Script, semantic read/edit contracts,
budgets, and retained proposal controller, never an App/default-backend fallback.
Without the opt-in binding they report unavailable. The ordinary build without
`--features agent` reports that agent support is disabled.

Use **Review source proposal**, then **Accept reviewed
source proposal** or **Reject source proposal**; each opens the existing command
bar for the proposal ID. Acceptance still checks source freshness, and is blocked
by dirty drafts. Proposed or accepted text is not automatically evaluated or saved. Review the diff
with F4 `:scope source-diff ID`; review details with `:scope source-proposal ID`,
then explicitly `:scope source-accept ID` or `:scope source-reject ID`. Accepted
text becomes the committed source; focus the editor to continue, publish with
Ctrl-S, and run with F5. Script proposal checks establish syntax only—not name or
type resolution, runtime success, effects, safety, or task correctness.

See [UX3 findings](ux3-document-workflow.md), [PTY evidence](ux3-pty-evidence.md),
and the [UX4 live local-model session](ux4-live-session.md) for tested behavior,
reproducible LM Studio steps, and remaining limitations.

## Know which context you are in

| Context | Input and help | State |
| --- | --- | --- |
| Workbench/editor | Editor keys insert language text; F1 actions; F4 commands; F6 focus; F7 layout | Retained workspace and scratch owner; local drafts publish only on Ctrl-S |
| Scope shell | F2 opens it; `help` lists shell commands; `:scope context` shows owners and targets | Borrows the same workspace, session, and scratches; `quit` returns |
| Continuous Script in shell | `:scope script enter`; Script expressions are language input; `:scope help` discovers workbench commands; `:scope script leave` leaves Script context | Retains workspace scratch state; distinct evaluation policy, not a universal grammar |
| Standalone Script interpreter | Developer CLI described below | Separate process, not a context switch into the running workspace |

Use F4 `help` for workbench commands. Do not paste shell commands into the editor.
The initial empty scratch is plain text: explicitly create a Script scratch before
F5. F2 requires editor drafts to be clean. Leaving the borrowed shell resumes the
same workbench; launching another process cannot preserve its in-memory state.
**Ctrl-Q** exits the workbench; unsaved in-memory scratches do not survive exit.

Workspace files are mounted read-only by default. Enabling explicit file saves
requires restarting with `cargo run -- --writable workspace`; publishing a scratch
is not a filesystem write. Agent support is not compiled by default, and enabling
it does not request a model or grant capabilities. Downstream semantic execution
requires installed providers, target binding, and independent runtime policy.
Cell remains the capability and filesystem boundary.

## Saved Script files and other interfaces

The public launcher currently supports the interactive workbench, `--help`, and
`--version`, plus existing explicit CLI commands. `scope run FILE` is **not yet
supported**: the standalone file runner has not been extracted into a shared CLI
implementation. Use the existing specialist interpreter for saved files:

```sh
cargo run -p scope-script -- examples/workbench.scope
```

This prints `{"name": "daily", "status": "ok", "total": 12}` with no host
providers. `cargo run -p scope-script -- --help` documents its output modes.
For piped shell input, explicitly select `cargo run -- shell`; the default
workbench requires a terminal. `--no-default-features` is a developer build that
falls back to the shell, not the recommended normal workflow.

## Next steps

- [Shell/reference tutorial](../TUTORIAL.md): explicit shell commands and longer examples.
- [Script language tutorial](tutorials/SCOPE_SCRIPT_TUTORIAL.md): language exercises,
  not another recommended application launcher.
- [Integration](integration.md) and [downstream example](../examples/downstream/README.md):
  compile-time language registration and launcher composition.
- [Architecture](architecture.md): ownership, revisions, proposals, and boundaries.
- [Entry-point audit](ux0-entry-points.md), [PTY evidence](ux0-pty-evidence.md), and
  [development](development.md): compatibility interfaces, features, validation.
