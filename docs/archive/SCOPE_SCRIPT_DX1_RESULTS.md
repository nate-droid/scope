# Scope Script DX1 results

## Implemented workflow

Use the existing binary, not a new execution framework:

```sh
cargo install --path crates/scope-script --locked
scope-script crates/scope-script/examples/daily.scope
```

Installation writes the executable into Cargo's bin directory; it is developer
setup, not a privilege granted to scripts. The equivalent checkout invocation is
`cargo run -p scope-script -- crates/scope-script/examples/daily.scope`.
Dependencies must be installed or fetchable; `--offline` works with cached dependencies.

Save edits in Zed or any editor and invoke the same command again. The binary reads
the explicit path each time, instantiates a fresh interpreter and `NoHost`, and
prints only explicit print values. No implicit saves, source edits, proposals, or
persistence occur. The file suffix conveys no authority. UTF-8 and the existing
64 KiB source limit are enforced; interpreter budgets are unchanged.

`daily.scope` uses immutable bindings, an object, an array, and a conditional.
Actual output, identical on repeated execution, exit 0:

```text
Object({"name": String("daily"), "status": String("ok"), "total": Integer(12)})
```

Diagnostics now include the supplied source path and one-based line and Unicode
scalar column while retaining the original UTF-8 byte offset and error description.
Exit 0 means interpreter success, not independent verification of semantic results.
Exit 1 covers input, interpreter, and host failures, whose messages remain distinct.
Earlier prints are flushed before errors; parsing fails before evaluation.

## Entry-point audit and integration choice

- `src/main.rs` already supported `scope-script FILE`, bounded file reads, fresh
  evaluation, output retention on failure, and failure exit status. DX1 reuses it
  and adds line/column attribution rather than creating `scope script run`.
- No arguments preserves terminal REPL/redirected-input behavior. REPL lines retain
  bindings intentionally; file/full-buffer runs do not.
- `ScriptRunner` binds an explicitly supplied `LanguageService`, operation,
  authorization, document and selection. Each invocation creates an interpreter.
  `RunRecord` owns source/target identity, revision and text, catalog, full semantic
  envelopes, printed values and the original interpreter outcome.
- The IDE shell routes selected scratch source through `ReadOnlyCommand::invoke_source`.
  Saved and unsaved source retain shell ownership; `:scope run` is not an implicit
  save. Its source/target provenance and semantic status reporting are preserved.
- `SemanticHost` adapts an already constructed `AgentSession`; operation discovery
  is metadata, never authorization. Registry/workspace/trial example binaries have
  distinct host-owned policies and Cargo invocations, not universally installed services.
- Path SS4-C uses its downstream workbench `ComparisonHost` through the same shell
  callback. Path action names, target binding, trial/runtime policy and result
  classifications stay downstream. It does not require replacing Scope's runner.

## Explicit semantic demonstration

```sh
cargo run -p scope-script --example semantic -- --allow-read crates/scope-script/examples/semantic.scope
```

The existing declaration-language example now accepts optional saved Script source.
The compiled host fixes the target declaration snapshot and selection, constructs
`SemanticToolProvider -> AgentSession -> SemanticHost`, and authorizes only its
expected read operation, snapshot/version and empty arguments. `--allow-read` is
required for invocation; no workspace, process, model, write, or Path service is
installed. Bounded source reads and fresh evaluation apply here too. Output before
failure is retained and file errors receive source locations.

Actual stdout, exit 0:

```text
String("Selected declaration")
String("answer")
String("integer_literal_binding")
String("Type analysis")
String("unknown")
String("Parsed only the selected declaration from the authorized snapshot; no reference or type analysis performed.")
```

Omitting `--allow-read` exits 1 with
`semantic.scope:2:14: tool language.example.declarations.describe_selection denied by policy at byte 52`
(the diagnostic includes the full supplied path). A pure standalone invocation of
`scope.invoke("semantic", "language.path.inspect", {})` exits 1 with
`unknown operation`; it does not load Path. Invalid semantic arguments return a
structured `invalid_arguments` status with interpreter success, tested separately.
Consumers must inspect envelopes rather than treating exit 0 as semantic acceptance.

No Path dependency is introduced into Scope. For the real kernel-backed Path
experiment, see the downstream `path/docs/design/SCOPE_PATH_SS4_C_RESULTS.md` and
`path/crates/path-scope/examples/scope_workbench.rs` in the sibling Path checkout.
These are downstream references, not files shipped in this repository.

## Validation performed

- `cargo test -p scope-script --all-targets --offline`: passed, including CLI,
  REPL/terminal PTY, semantic, runner, transcript and example tests.
- CLI tests cover saved pure execution, identical fresh repeats, saved edits,
  unchanged source/directory contents, syntax failure, retained prints on runtime
  failure, missing file, unavailable host, UTF-8, size limits and source attribution.
- Semantic example: six tests passed, including explicit/missing/denied authority,
  structured rejection, retained prints, fresh bindings and bounded input. Target
  text/version and absence of edit proposals are checked.
- Actual binary invocations: pure and repeat exit 0; parse, runtime, missing file,
  unavailable semantic operation and denied semantic invocation exit 1. Runtime
  `print(7)` before division by zero remained visible. Authorized semantic file
  execution printed the output above and exited 0.
- `cargo build -p scope-script --example semantic --example scratch_runner --locked --offline`
  followed by `python3 crates/scope-script/tests/semantic_cli.py -v`: six process-level
  tests passed. These exercise real authorized/denied invocations, deterministic
  repeats, parse/runtime errors, retained prints, structured rejection with exit 0,
  missing/invalid/oversized input, extra arguments, source attribution and no writes.
  Semantic input diagnostics now consistently identify the supplied path, and its
  executable reports errors without Rust's quoted `main -> Result` debug wrapper.
- `cargo build -p scope-ide --locked --offline` followed by
  `python3 crates/scope-ide/tests/pty_shell.py -v`: all seven terminal tests passed.
- `python3 crates/scope-script/tests/pty_scratch_runner.py -v`: all three passed,
  covering authoring, repeated execution, revisions, multiline conditionals,
  completion cancellation and keyboard behavior.
- Downstream Path, using its existing documented commands:
  `cargo test -p path-scope --example scope_workbench --test comparison_host --locked --offline`
  passed 11 comparison-host and 5 workbench tests;
  `cargo build -p path-scope --example scope_workbench --locked --offline` passed;
  `python3 -u crates/path-scope/tests/workbench_pty.py` passed authorized/denied runs.
  No Path source changes were made.
- `cargo fmt -p scope-script --check` and `git diff --check`: passed.

Builds emitted existing duplicate Cell package warnings and unused example fixture
warnings. Installation into the user's Cargo bin directory was documented, not
performed; checkout-built binaries were exercised directly.

## Deferred deliberately

No host-profile registry, plugin loading, service autodetection, arbitrary target
flag, daemon, watcher, asynchronous execution or automatic persistence was added.
The smallest current environment selection is choosing an existing trusted example
binary; target and authorization remain its responsibility. A future installed
host executable could expose `HOST script FILE --target TARGET` with explicit
trusted policy, without asking Script to load providers.

Script arguments are deferred: extra CLI arguments still fail. A future minimal
interface could bind an immutable host-supplied `args` array after `--`, but that
requires an explicit runtime binding contract and is not implemented. Neither
source extensions nor discovery will authorize such future execution.
