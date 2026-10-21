# SS2 — Pleasant authoring and fresh buffer execution

## Implemented integration

`scope-ide::cli::ReadOnlyCommand::invoke_source` is an opt-in, default-denying borrowed-source callback. The shell resolves its existing selected `ScratchBuffer` independently from the open App `Document`. Existing `invoke` implementations and `:scope host` behavior remain unchanged. Scope IDE does not depend on Scope Script.

`scope-script::runner::ScriptRunner` implements this callback downstream. It installs one explicit language semantic read through the existing `SemanticToolProvider`, `AgentSession`, and `SemanticHost`. The runnable `scratch_runner` example assembles the real registered declaration-language fixture. It requires no model, process launcher, network, or Path installation. This is a supported example integration, not automatic Script support in every downstream shell or the standalone Script REPL.

The target adapter borrows a document clone because `DocumentSource` requires a mutable reference. It preserves document identity/version and unsaved text. Neither the original target nor the active interactive runtime is handed to Script. The provider retains existing operation argument validation, source policy, revision preconditions, and read-effect restrictions. `None` and `Some(false)` authorization deny semantic calls. Discovery is not authority.

## Exact authoring workflow

From the repository root:

```sh
cargo run -p scope-script --locked --offline --example scratch_runner -- crates/scope-script/examples/semantic-workspace --allow-read --terminal
```

```text
open example.decl
:scope scratch new scope-script
:scope edit
let result = scope.invoke("semantic", "language.example.declarations.describe_selection", {});
print(result);
result.value.name
:done
:scope run language.example.declarations.describe_selection --selection 0:0..0:15
:scope run language.example.declarations.describe_selection --selection 0:0..0:15
:scope edit
:at 3 1
:delete 17
:insert result.value.kind
:done
:scope run language.example.declarations.describe_selection --selection 0:0..0:15
show
dirty
quit
```

The first two results are `String("answer")`; the third is `String("integer_literal_binding")`. Prints and complete semantic envelopes are also shown. `:done` commits only the in-memory editor draft. No implicit saves occur.

Existing `:scope open PATH.scope` loads/selects a separate source buffer; `:scope show`, `:scope scratch select ID`, and the existing editor work unchanged. In this example, unprefixed `open PATH` selects the App target; namespaced `:scope open PATH` selects a scratch source. The new command requires the explicit namespace and an installed callback. Its selection uses zero-based, half-open UTF-8 byte coordinates **in the target**, not the script. Editor cursor coordinates are one-based Unicode scalars. The source's language label does not install or replace a runtime.

## Freshness, outcomes, and attribution

Every full-buffer invocation constructs a new `Interpreter`; existing interpreter budgets and persistent standalone REPL semantics are unchanged. Bindings and output cannot cross runs. A fresh interpreter does not create, reset, or switch the downstream language session.

Each `RunRecord` owns:

- Script and target `scope_language::SourceId`, `RevisionId`, and exact text snapshots.
- Installed operation name, target selection, and complete discovery/catalog value.
- Complete semantic return envelopes independently of Script projection or discard.
- Ordered prints, including those before a later runtime failure.
- Final `Result<Value, Error>` with the original interpreter/host error and its source byte offset.

Shell presentation labels source and target identities/revisions, semantic envelopes, ordered prints, final values, and explicit interpreter status. On interpreter failure it additionally labels the source-relative one-based line/Unicode scalar column and original UTF-8 byte offset, followed by the original error message. UTF-8 slicing is boundary-safe. Prints before failure remain visible; the original structured error remains in the owned record. Semantic dispatch success is not labeled semantic success: status, rationale, partial payloads, unknown/unsupported information remain intact.

Setup failures (missing inputs, unknown operation, invalid target selection, provider construction) fail before evaluation and do not append a run record. The synchronous callback holds immutable borrows and refreshes snapshots/provider preconditions on every run; there is no queued stale-buffer execution or reusable historical run request. Tests verify that source and target edits advance independently and previous records retain their old text/revisions. Existing semantic-host tests exercise stale target revision envelopes. There is no claim of atomic freshness against external asynchronous native provider state or filesystem modifications: execution targets the current in-memory document, not a disk reread.

## Type and authority boundaries

The transport remains `ToolValue`; no Script type syntax or structural compatibility implementation was added. The upstream catalog projection is retained verbatim, including available operation metadata, contract/nominal identifiers and versions. Complete result envelopes preserve upstream source provenance, statuses, obligations, unsupported and partial information when exposed. Script does not infer equivalence from object shape or convert unrelated nominal contracts. Deeper type information absent from the upstream descriptor remains absent/unknown; the fixture explicitly reports unknown type analysis and unsupported references.

Existing registry schema validation and nominal-identity checks remain unchanged and are covered by the existing Script regression suites. This runner uses the semantic provider rather than replacing the registry or manufacturing stronger contract claims.

Scripts gain no filesystem writes, source edits, proposal acceptance, process/network access, or agent/model invocation. Only explicit read operations pass both the Script adapter and source-policy gates. The author can still edit source through the shell's existing independently authorized controls.

## Interactive input and completion

`cli::run_terminal_with_session_and_command(app, session, command)` is an explicit terminal-only embedding entry point. It rejects non-terminal input/output and uses the existing shell input/history/completion machinery. `scratch_runner --terminal` selects it; without that flag the example and `run_with_session_and_command` remain deterministic buffered interfaces. There is no descriptor-based switching of buffered APIs.

`ReadOnlyCommand::complete_source(&self, source, draft, cursor)` is a default-empty authoring hook returning existing structured `CompletionItem`s. The cooked editor passes its transactional whole draft and UTF-8 cursor independently of the active language session. `:complete` lists suggestions; `:complete INDEX` recomputes and applies a validated replacement. `shell_input` shares range/size validation with the editor. This is intentionally textual cooked-editor completion, not physical-Tab editing inside the cooked editor. Physical Tab remains handled by the existing top-level shell.

`ScriptRunner` reuses `ScriptCompletionProvider` with an empty interpreter and metadata-only host exposing the explicitly installed semantic provider/action. It does not evaluate draft expressions, call discovery or invocation, acquire authorization, or infer bindings from source declarations. The standalone persistent Script REPL still completes its actual interpreter bindings. No new language session is installed, so a selected Script buffer cannot replace the active downstream session.

Example completion within `:scope edit`: enter `:insert scope.`, then `:complete` to list `scope.invoke` and `scope.operations`, and `:complete 2` to apply `scope.operations`. Finish the call with `:insert ()`. `:done` commits only the source draft. Use the explicit run command shown above, even for pure scripts; this minimal host requires a selected target and selection for all runs.

## Validation performed

- `cargo test -p scope-script -p scope-ide --all-targets --locked --offline` — passed, including existing registry, nominal-contract, semantic failure-envelope, budgets, persistent REPL, and IDE shell suites.
- `cargo test -p scope-script --locked --offline --test scratch_runner` — 9 passed. Includes multiline cooked editing, open-existing-source/unsaved execution, repeated fresh bindings, prints before failure, Unicode offsets, independent source/target revisions, default denial, unsaved target snapshots, unchanged files, and preserved runtime ownership.
- Documented example executed through `run_with_session_and_command` with multiline input, two identical runs, edit, and third run — passed; asserted two `answer` results, one `integer_literal_binding` result, and no shell errors.
- `git diff --check` — passed before final documentation update.

Additional SS2 interactive validation:

- `cargo test -p scope-script -p scope-ide -p scope-terminal --all-targets --locked --offline` — passed.
- `cargo test -p scope-ide --lib --no-default-features --locked --offline` — 255 passed; default IDE library tests — 342 passed; all-features check passed.
- `cargo build -p scope-script --example scratch_runner --offline --locked` — passed.
- `python3 crates/scope-script/tests/pty_scratch_runner.py -v` — 2 actual-binary PTY tests passed. Covers multiline authoring, textual Script completion, prints/final/status, repeated source revision, editing/rerun, independent target revision and unchanged files; top-level keyboard Tab/menu dismissal, Ctrl+C cancellation, cursor movement/history, and Ctrl+D exit.
- `cargo build -p scope-ide --locked --offline && python3 crates/scope-ide/tests/pty_shell.py -v` — 7 existing T1 PTY tests passed, including multiline/paste, cursor editing, history, completion cancellation, cooked editing and EOF. The pre-existing untracked T1 test file was not modified.
- `cargo fmt --all -- --check` and `git diff --check` — passed.
- New deterministic tests verify completion does not dispatch or invent bindings and multibyte error columns preserve preceding prints.

Cargo emitted existing duplicate Cell package checkout warnings; no test failures resulted. These are fixture-based checks, not live Path, Forth, or model integration coverage.

## Recommendations for SS3

Install the same explicit terminal/callback assembly in a real downstream Path/Forth host, with that host's service and authorization, and run language-specific PTY/runtime-preservation tests there. Keep Script authoring completion separate from runtime ownership. Consider bounded record retention and richer rendering before asynchronous execution; queued execution would need explicit source as well as target revision preconditions. Preserve upstream contracts/envelopes rather than adding Script-specific type inference or structural adapters.

## Limits and blockers

No new terminal, buffer store, language-session ownership, syntax, or Cell capability was necessary. The concrete blocker identified by SS1 was the target-only host callback; the borrowed-source callback resolves it without a dependency cycle.

This milestone exposes one host-selected read per runner. Production Path/Forth assembly must explicitly install its own service/operation and authorization. The example does not claim those language integrations are installed. Full-buffer execution remains synchronous, without native-call timeout or cancellation. Results use structured Rust debug rendering rather than a new UI/browser. Run history is host-owned and has no automatic eviction; long-lived embeddings should drain/archive `records`. Existing interpreter and per-host-call budgets still apply, not a global session-history budget. Creating new persisted files is not added: scratch buffers are sufficient, and existing save/bind restrictions remain. No orchestration framework, structural conversions, new control flow, or persistent Script language session was introduced.
