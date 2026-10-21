# DX4 — Everyday Script workbench results

## Delivered workflow

From the repository root:

```sh
cargo run
```

```text
:scope scratch new script
:scope edit
let n = 6;
if true { print(n) };
n * 7
:done
:scope script run
:scope script run
:scope edit
:at 1 9
:delete 1
:insert 7
:done
:scope script run
quit
```

The first two runs print `6` and finish with `42`; the edited run prints `7` and finishes with `49`. Nothing is saved. `:scope open examples/workbench.scope` opens the supplied runnable example as a scratch source. The cooked editor also supports `:show`, `:complete`, `:cancel`, and multiline insertion. Terminal history, navigation, paste, and completion remain the existing shell implementation.

## Interface reconciliation and placement

Previously the stock `scope-ide` executable called `cli::run_from_env` without a source callback. `ReadOnlyCommand::invoke_source` supports an explicitly configured semantic runner but requires a separate open document and target selection. `ScriptRunner`/`RunRecord` deliberately retain that semantic target, operation catalog, and envelopes; they are not a suitable pure-script default.

The workspace now defaults to the small `scope-workbench` package, binary `scope`. It depends on both existing libraries; `scope-script` already depends on `scope-ide`, so reversing that dependency would create a cycle. The original IDE binary remains available with `cargo run -p scope-ide`. No core crate split, registry, provider loader, or interpreter replacement was needed.

`cli::run_from_args_with_command` reuses ordinary CLI argument parsing, workspace setup, and terminal detection. `ReadOnlyCommand::invoke_script` is a default-denying, borrowed-source hook without a document or session parameter. Only explicit `:scope script run` invokes it. Legacy `:scope run OPERATION --selection RANGE` and `invoke_source` retain their downstream semantic meaning.

`scope_script::workbench::WorkbenchRunner` installs this hook with standalone `NoHost`. Each run constructs `Interpreter::new`, preserving existing parser/evaluation limits and immutable bindings. Completion uses the existing Script completion provider with a fresh interpreter and no live bindings.

## Ownership, results, and authority

The shell remains the sole scratch/source-buffer owner. A run clones exactly its selected buffer's evaluation `SourceId`, revision derived from version, and full UTF-8 source text into the existing `Snapshot` representation. `PureRunRecord` retains that owned snapshot, ordered printed `ToolValue`s, and the original evaluation result/error. There is no synthetic semantic target or semantic success classification. Unsaved edits do not touch the unrelated App document or downstream interactive language session.

Presentation uses DX2 `format_value` and Unicode-safe `source_location`. Output includes source identity/revision, ordered prints, final value on success, interpreter status, and one-based line/Unicode-scalar column plus original byte offset on failure. Prints survive later runtime errors; parsing still precedes all effects. Evaluation failures are retained as outcomes, whereas writer/setup failures return through the host callback's error path. Full structured values remain in records; human display retains DX2's bounded formatting.

`NoHost` installs no providers or authorization: filesystem, source modification, proposals, semantic operations, model/agent invocation, network, and processes are unavailable. Selecting a `.scope` file neither executes it nor grants authority. Scope Simple analysis remains independent from Script execution. Workspace access still defaults to read-only, and buffered startup/edit/run creates no workspace files.

Explicit downstream hosts can implement the same borrowed-source hook and completion using existing `Host`, authority, `AgentSession`, and semantic adapters. The existing configured `ScriptRunner` remains intact. Availability is not authorization; no automatic Path/Forth registration or structural/nominal type adaptation was added.

## Validation

Commands run successfully:

```sh
cargo build -p scope-workbench --offline
cargo test -p scope-script --lib
cargo test -p scope-workbench --offline
cargo test --workspace --offline
cargo fmt --all -- --check
cargo run --offline -p scope-script -- examples/workbench.scope
printf ':scope scratch new script\n:scope edit\nlet n = 6;\nif true { print(n) };\nn * 7\n:done\n:scope script run\n:scope script run\nquit\n' | cargo run --offline
```

- Script library: 28 tests passed, including five new pure-runner tests.
- Workbench: seven integration tests passed (three buffered binary tests, two in-process state-isolation tests, two PTY launchers).
- Actual-binary PTY coverage includes multiline cooked editing, repeated execution, edit/rerun, and the existing IDE terminal suite against the default workbench binary.
- In-process tests use a stateful downstream evaluator: bindings persist, no reset/replacement occurs, and unrelated unsaved document content remains unchanged. Extension-selected Scope Simple analysis and explicit Script execution stay distinct.
- Buffered tests cover revision provenance, fresh bindings, conditionals, syntax/runtime failures, Unicode locations, ordered prints, recovery, and no workspace writes. Runner tests cover denied operations and output-writer failures.
- Full offline workspace suite passed, including existing Script and terminal regressions. Cargo emits duplicate cached Cell-package warnings; these did not fail validation.

## Limitations

The shell is still a terminal workbench, not a full-screen editor. Pure-run history is retained in memory for the runner lifetime; no persistence/history UI or live Script binding completion is added. Human output may be abbreviated by existing DX2 budgets, while records retain complete values. The default binary has no downstream interactive evaluator registered; preservation is tested with an explicitly configured provider. PTY tests require Unix and Python 3. Optional agent/TUI configurations remain available through the existing IDE package, not new workbench feature forwarding.
