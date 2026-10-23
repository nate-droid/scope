# Scope Script

Script is an embeddable orchestration language with a standalone CLI and the
DX4 default workbench. Root `cargo run` selects package `scope-workbench`, binary
`scope`, depending on `scope-ide` and `scope-script`.

## Unsaved source: the default workbench

Start `cargo run`, then enter:

```text
:scope scratch new script
:scope edit --commands
let experiment = {name: "daily", samples: [2, 4, 6]}
let total = experiment.samples[0] + experiment.samples[1] + experiment.samples[2]
if total == 12 {
    print({name: experiment.name, total: total, status: "ok"})
} else {
    print({name: experiment.name, total: total, status: "unexpected"})
}
:done
:scope script run
```

This exact source is runnable as
`cargo run -p scope-script -- examples/workbench.scope`. On a TTY, plain
`:scope edit` opens a multiline keyboard editor: arrows/Home/End, Enter,
Backspace/Delete, Tab, and bracketed paste edit the draft; Ctrl-S accepts and
Ctrl-C/Escape cancel. The transcript uses `:scope edit --commands` explicitly;
non-TTY input also defaults to this cooked-line editor, which
inserts ordinary lines with a newline. `:done` commits to memory only;
`:cancel` or EOF discards the draft. Inside the editor, use `:at LINE COLUMN`
(one-based Unicode scalars), `:delete COUNT` (forward, including newlines), and
`:insert TEXT` (escapes `\n`, `\t`, `\\`) to revise source.
`:complete` lists completions and `:complete INDEX` applies a current item.
Completion uses fresh Script metadata, not live interpreter bindings, and does
not evaluate source or authorize operations. These colon commands belong to
command mode, not the raw editor. Acceptance creates one revision only if text
changed; cancellation creates none. See the [editing tutorial](scratch-editing.md).

Each `:scope script run` snapshots the selected scratch source and evaluates it
with a fresh interpreter and `NoHost`. Output includes source identity/revision,
ordered prints, final value and interpreter status, or source-located errors.
No bindings persist between runs. Neither editing nor running implicitly saves
source, writes results, or creates a report. Opening `.scope` source grants no
authority and installs no providers.

`:scope run` remains a distinct, configured downstream semantic runner. Scope
Simple (`scope.simple`) supplies analysis; neither its analysis nor pure Script
success is downstream semantic acceptance. Interactive language sessions and
host-backed Script remain explicit embedding choices.

## Composing downstream semantic reads (SI7)

`ScriptRunner` is the reusable `ReadOnlyCommand` adapter for
`scope_ide::cli::run_with_registry_and_command`. A launcher can explicitly expose
several existing operations from one `LanguageService` against one bound target:

```rust,ignore
let mut runner = ScriptRunner::with_operations(
    service,
    [
        ("inspect".to_owned(), Some(true)),
        ("check".to_owned(), None),
    ],
)?;
run_with_registry_and_command(args, registry, &mut runner)?;
```

Use actual language-local operation IDs, not these illustrative names. Exact API:

```rust,ignore
pub fn with_operations(
    service: &'a dyn LanguageService,
    operations: impl IntoIterator<Item = (String, Option<bool>)>,
) -> Result<Self, Box<dyn std::error::Error>>;
```

The existing `ScriptRunner::new(service, operation_id, authorization)` remains
compatible. The collection constructor rejects empty collections, duplicate IDs,
unknown IDs, and non-`Read` operations. It does not replace earlier bindings.
The normal semantic provider also rejects duplicate full identities, including
collisions between providers in an `AgentSession`. Short names are never used for
Script dispatch: identities remain `language.<language_id>.<operation_id>`.
There is no alias or language-wide permission mechanism.

For hosts already constructing a semantic `AgentSession`, the lower-level API is:

```rust,ignore
pub fn with_bindings(
    session: AgentSession<'a>,
    bindings: impl IntoIterator<Item = (String, Option<bool>)>,
    expected_source: impl Into<String>,
    expected_version: u64,
) -> Result<Self, Box<dyn std::error::Error>>;
```

This is `SemanticHost::with_bindings`; it accepts full operation identities and
rejects duplicate bindings. As with `SemanticHost::new`, the embedding must install
trusted semantic providers and capture source/version tokens from their target.
Unknown bindings are not discoverable and invocation fails explicitly. Non-read
bindings are excluded from discovery and explicitly rejected on invocation.
Discovery preserves the underlying session catalog order and original semantic
metadata, including typed arguments, effect, target, purpose, and constraints.

### Exposure is not authorization

There are four distinct boundaries: a language advertises operations; the host
selects exact identities for Script exposure; each selected identity separately
receives `Some(true)` authorization (both `None` and `Some(false)` deny); and the
semantic provider checks its host-bound target, scope, revision, typed arguments,
and source policy. Discovery does not grant authority. The runner installs the
same per-operation read-only restriction at both the Script/session and semantic
provider boundaries. Registered but unexposed operations remain unavailable.
Argument-validation or source-policy failures can be structured semantic envelopes;
a successful interpreter result alone is not semantic success.

The runner uses independent Script and semantic scratch sources. Each run captures
fresh host-owned snapshots; retained `RunRecord` evidence keeps the original text,
identity, and revision even after later edits. No source is implicitly saved and
scripts supply neither filesystem paths nor revision tokens. A scratch language
mismatch fails provider construction. Without a bound target, the existing pure
Script callback remains available but has no semantic host. Document-based `run`
continues to use the embedding's configured language for its cloned document;
embeddings must select the appropriate service rather than infer authority from a
filename.

The adapter supports one service and one target per runner, not multiple target
routing. All operations share the existing 64-call host budget. For compatibility,
`RunRecord.operation` remains the entry operation label (the lexically first full
binding for scratch callbacks), not a list of calls; `results` retains all returned
semantic envelopes. Provider descriptions are expected to remain stable during a
session, as in the existing semantic provider contract.

Only declared `Read` effects are supported. Candidate evaluation is callable only
when its existing operation is genuinely declared `Read` and independently
authorized; purpose metadata grants nothing. `ProposeChanges`, edit/review flows,
runtime authority, and filesystem capabilities are not added. Native language
providers remain trusted code; this adapter does not sandbox their computation.

### Runnable demonstration

From the repository root:

```sh
cargo run -p scope-script --example multi_semantic --locked --offline
```

The [demo](../crates/scope-script/examples/multi_semantic.rs) installs a small
fixture language, discovers `echo` and `measure`, invokes both against an independent
unsaved target, and combines their envelopes into a report. It then prints explicit
rejections for a registered-but-hidden operation and an exposed-but-denied one;
only the two authorized reads reach the provider. No downstream dependencies or
files are needed.

SI7 extends the two Script adapters, not the underlying semantic system:
`SemanticToolProvider` and `AgentSession` already represented multiple operations,
typed dispatch, source policies, and duplicate rejection. Their existing facilities
were reusable, but the single-action fields in both Script adapters prevented
collection exposure. Downstream adoption only requires replacing the single
constructor with the collection constructor and choosing authorization separately
for each existing read operation; registry and launcher composition stay unchanged.

## Saved files: edit, run, repeat

From the repository root:

```sh
cargo run -p scope-script -- --help
cargo run -p scope-script -- crates/scope-script/examples/daily.scope
```

The general form is `cargo run -p scope-script -- FILE`. Files must be UTF-8 and
at most 64 KiB. Each run reads the saved source into a fresh interpreter with
`NoHost`: no providers, semantic services, workspace grants, or language runtime.
The CLI reads the supplied script file; the script itself receives no filesystem
authority. The `.scope` suffix is only a convention.

Edit and save in Zed or another editor, then repeat the command. The daily example
prints:

```text
{"name": "daily", "status": "ok", "total": 12}
```

No arguments (`cargo run -p scope-script`) starts the stateful REPL instead.
File runs do not preserve REPL bindings or implicitly save source/results.

## Structured values and conditionals

The ordinary, no-host `daily.scope` example uses records, arrays, field/index
access, arithmetic, and a boolean conditional:

```text
let experiment = {name: "daily", samples: [2, 4, 6]}
let total = experiment.samples[0] + experiment.samples[1] + experiment.samples[2]
if total == 12 {
    print({name: experiment.name, total: total, status: "ok"})
} else {
    print({name: experiment.name, total: total, status: "unexpected"})
}
```

Change a sample in your own saved copy and rerun to explore the other branch.
Conditions must be booleans; only the selected branch executes. Script has no
loops or user-defined functions. Host operations use the same structured values,
but their installation and authorization remain the embedding host's decision.

## Output and failure

```sh
cargo run -p scope-script -- --output summary crates/scope-script/examples/daily.scope
cargo run -p scope-script -- --output json crates/scope-script/examples/daily.scope
```

- `prints` (default): explicit `print` output only, with bounded human previews.
- `summary`: prints plus labeled final value and **interpreter** status.
- `json`: one full `scope-script.execution` version 1 record on stdout, including
  source path/text, printed values, tagged outcome, and no-host metadata.

Evaluation failure exits nonzero. Human errors include source path, one-based
line/column, and byte offset. Earlier prints survive runtime failure; parse
failure executes nothing. JSON evaluation errors are on stdout, with error
location and source chain; setup errors use stderr. JSON contains full source
and values, may be large, and is not a semantic proof or host replay record.
Interpreter success does not imply acceptance by a downstream semantic service.

## Host support is explicit

Naming a provider or operation does not install or authorize it. An embedding
host must register services, choose target snapshots, and authorize concrete
calls. A runnable declaration-language example is:

```sh
cargo run -p scope-script --example semantic -- --allow-read crates/scope-script/examples/semantic.scope
```

This is a separate trusted example host, not a flag that enables providers in
the standalone CLI. It installs one declaration read against a fixed host-owned
target and selection. The supplied file is Script source, not the target.
Omitting `--allow-read` denies invocation. It does not install Path, Forth, or
an arbitrary downstream runtime; there is no automatic host loader.

For a downstream, host-backed unsaved-source workflow, the explicit
`scratch_runner` example installs a semantic Script runner in the IDE shell.
Unlike the default pure workbench, it uses a separately configured App target.
Source buffer and App target remain separate; execution neither saves the script
nor edits the target. See the
[example catalog](../examples/README.md) for commands and host classifications.
See also [architecture](architecture.md) and [source evaluation](SOURCE_EVALUATION.md).
