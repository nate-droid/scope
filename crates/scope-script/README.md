# Scope Script (S4–S8 experiment)

## Exploration Pack 1 (workspace-backed semantics)

Run independently from the repository root:

```sh
cargo run -p scope-script --example exploration --offline -- e1 --allow-read crates/scope-script/examples/semantic-workspace
cargo run -p scope-script --example exploration --offline -- e2 --allow-read crates/scope-script/examples/semantic-workspace
cargo run -p scope-script --example exploration --offline -- e3 --allow-read crates/scope-script/examples/semantic-workspace
```

E1 inspects the selected declaration, E2 reports exposed metadata (not authority),
and E3 composes a nested report from real semantic results. All use S9's existing
read-only, host-selected workspace target. See [execution evidence, assertions,
and limitations](../../docs/design/SCOPE_SCRIPT_EXPLORATION_1.md).

## Read-only language semantics (S8)

```sh
cargo run -p scope-script --locked --offline --example semantic
```

The embedding runs [`examples/semantic.scope`](examples/semantic.scope) against
Scope IDE's existing example declaration language. It explicitly scopes a scratch
document and selection, installs `SemanticToolProvider` in `AgentSession`, and
exposes one read operation through `semantic::SemanticHost`. The script discovers
that operation and reports the binding name, kind, unknown type status and rationale.
This is a real example language operation, not a production language integration.

Hosts select the exact action and capture source/version tokens from the provider's
target. `SemanticHost::new` requires explicit `Some(true)` call authorization;
`None`/`Some(false)` deny invocation. The semantic provider independently enforces
its source policy, scope and revision checks. Discovery grants no capability.
Script arguments cannot change paths, selections or revision tokens. Structured
outcomes pass through unchanged; callers must inspect `status`, not assume success.
The adapter permits at most 64 calls. No semantic capabilities are installed in the
standalone CLI by passing it this script: use the embedding command above.

See [S8 results](../../docs/design/SEMANTIC_SCRIPTING_RESULTS.md) for API findings,
validation and limitations, including provider-owned payload shapes and the current
Scope semantic dispatch API's empty language path.

A small downstream Rust interpreter consuming Scope's existing operation APIs. Scope does **not** depend on this crate. No new registry, type system, invocation machinery, Cell changes, or editing lifecycle is added to Scope.

## Try it

From the workspace root:

```sh
cargo run -p scope-script --offline
```

```text
scope> let x = 5
=> Integer(5)
scope> let y = x * 2
=> Integer(10)
scope> print(y)
Integer(10)
scope> let operations = scope.operations()
=> Array([])
scope> print(operations)
Array([])
scope> :quit
```

The standalone REPL has **no installed providers or authority**. It accepts one statement line at a time (multiple statements may be separated by semicolons), retains immutable bindings, and recovers after evaluation errors. EOF or `:quit` exits. Values are displayed using their Rust structured debug representation, not JSON. Source-string evaluation supports multiline programs and collections. Run a local UTF-8 script (maximum 64 KiB) with `cargo run -p scope-script --offline -- crates/scope-script/examples/pure.scope`. Use `--help` for CLI usage. File execution uses the same interpreter, without providers or authority; it is not a workspace host.

### Interactive editing

When **both stdin and stdout are terminals**, the standalone REPL uses the public
`scope-terminal` reader:

| Keys | Action |
| --- | --- |
| Left / Right | Move one Unicode scalar |
| Home / End, Ctrl+A / Ctrl+E | Move to start / end |
| Backspace / Delete | Delete before / at the cursor |
| Up / Down | Recall history / return to the saved draft and cursor |
| Tab | Complete a name; show ambiguous candidates |
| Enter | Submit the line |
| Ctrl+C or Escape | Discard the line and reprompt without evaluation |
| Ctrl+D | Exit on an empty line; otherwise delete at the cursor |
| `:quit` then Enter | Exit |

History is session-local, in memory, and includes submitted lines (including
those that fail evaluation), not cancelled drafts. Recalled edits do not alter
stored history. Consecutive duplicate and empty entries are omitted.
Pasted terminal escape sequences are filtered; multiline paste becomes one line.

Tab completes `let`, `true`, `false`, `null`, the built-ins `print`,
`scope.operations`, `scope.invoke`, and bindings from previous evaluations.
For example, enter `let answer = 42`, then type `ans` and press Tab.
Completion works at the cursor and replaces the whole current name, preserving
surrounding text. Ambiguous prefixes display candidates without evaluation.

Hosts can expose provider/action names through the read-only
`Host::operation_names` hook. `RegistryHost` reads the existing operation registry
directly. Inside `scope.invoke("provider", "action", {})`, Tab completes the
provider string or actions for a literal provider. Other strings are not completed.
The standalone CLI has no installed operations. Custom hosts default to no operation
candidates; completion never invokes operations or grants execution authority.
Object fields and bindings in the unsubmitted line are not inferred.

If either stream is redirected, input stays buffered with no raw mode, prompts,
or editing escape sequences. Existing line-size and UTF-8 checks are retained;
an unterminated final line is evaluated. Script-file mode is unchanged.

**Integration note (T3):** only terminal input acquisition and prompt selection
changed. Both input paths share the existing evaluator/output handling; providers,
host registration and authorization are unchanged. Scope Script calls the public
`scope-terminal` API directly, not Scope IDE's shell. The reader's RAII guard
restores terminal state before evaluation, including on reader errors. There is
no multiline editing or persistent history. Unicode editing uses
scalar boundaries, not grapheme clusters; the shared renderer displays non-ASCII
text as escaped ASCII and scrolls a single line. History is currently unbounded.

`cargo test -p scope-script --offline` includes automated Unix PTY tests requiring
`python3` (standard library only). They exercise editing, history/draft cursor
restoration, cancellation, EOF, evaluation-error recovery, and termios restoration
after normal exit. A file-error exit also checks unchanged termios, but does not
exercise a raw-reader I/O failure. Reader I/O-failure fault injection and manual
terminal-emulator testing have not been performed; automated PTY results are not
claims about visual behavior in every emulator.

For an actual capability-backed document inspection:

```sh
cargo run -p scope-script --example registry --offline
```

The embedding example creates a temporary document **in host setup**, opens a read-only Scope workspace, and installs a downstream demonstration provider in `OperationRegistry`. Its script is [`examples/registry.scope`](examples/registry.scope). The provider inspects the real `DocumentSnapshot`, returning source text and UTF-8 byte count. This is a new example provider, not a claimed pre-existing production registry operation: Scope currently has none. The integration test `tests/semantic_inspection.rs` separately reuses Scope's **existing selection-description example operation**, discovers and invokes it through `AgentSession` / `SemanticToolProvider`, and preserves partial-result diagnostics and source preconditions. S8 promotes that wiring into the public, narrow `semantic::SemanticHost` adapter; there is still no automatic semantic-to-registry bridge.

## Practical workspace automation (S6)

```sh
cargo run -p scope-script --example workspace --offline -- --allow-read crates/scope-script/examples/handoff-workspace
```

The checked-in workspace produces a Markdown handoff report with two documents,
headings, revision tokens, two pending items, and `needs_follow_up: true`.
Pass another small workspace root after `--allow-read` to inspect it. Omitting
explicit authorization fails before opening the workspace. Results are structured
Rust debug output, not JSON; embedded consumers receive `ToolValue` directly.

[`workspace.scope`](examples/workspace.scope) discovers the host's two actions,
discovers Markdown documents, requests analysis of that discovered batch, and
assembles a reusable context report. The example host projects existing Scope
`Workspace::list_files` and `load_file_revision` APIs; these are **not existing
registry operations**. Heading and pending-item extraction are downstream,
line-based analysis. No interpreter or Scope language features were added.

The host rejects more than 128 listed files, 16 Markdown files, 32 KiB aggregate
source per batch, or eight host calls. These checks occur after underlying Scope
APIs materialize data, so they are not streaming traversal/allocation limits.
Paths and expected revisions are host-owned; scripts cannot pass replacements.
A changed discovered document aborts analysis. Reads are not an atomic workspace
snapshot and newly added files are not detected after discovery. This example is
for small, trusted, explicitly selected workspaces, not adversarial filesystem
sandboxing. Full source is included in the report; review it before sharing with
an agent or external model.

The direct Rust comparison helper and six tests live alongside the example.
See the [architecture review](../../docs/design/SCRIPTING_ARCHITECTURE_REVIEW.md)
for evidence, limitations, manual/API comparison, and the recommendation.

## Daily use from a terminal (S7)

Run these exact commands from the repository root (the directory containing the workspace `Cargo.toml`):

```sh
# Discover entry points.
cargo run -p scope-script --offline -- --help
cargo run -p scope-script --example workspace --offline -- --help

# Inventory: sorted Markdown paths and host-captured revision tokens, without source output.
cargo run -p scope-script --example workspace --offline -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/markdown-inventory.scope

# Review: pending counts, follow-up flag, headings, pending lines, and full document context.
cargo run -p scope-script --example workspace --offline -- --allow-read crates/scope-script/examples/handoff-workspace crates/scope-script/examples/pending-review.scope
```

Replace only the root immediately after `--allow-read` with an absolute path to a **small, trusted local development workspace**, for example `/absolute/path/to/hobby-project/docs`. Script paths and relative workspace roots resolve from the terminal's current directory. Select a documentation subdirectory if the project exceeds the host limits above. The first Cargo build requires the dependencies to be cached; if offline resolution fails, run the same command without `--offline` with network access. No model, API key, service, installation, or package manager is required.

The inventory example works on empty workspaces; pending review reports zero documents and `needs_follow_up: false`. Pending detection is the existing simple line match for `TODO` or `- [ ]`, not semantic task analysis. Review includes full source: do not share its output without checking for private content. Neither script writes files, accepts paths/revisions as operation arguments, or changes authorization. The host retains revision checks between discovery and inspection.

The workspace runner accepts `--allow-read ROOT [SCRIPT]`; omitting SCRIPT preserves the S6 handoff workflow. It prints script output once (or the final value if nothing was printed). The standalone CLI is deliberately different: it has no workspace capabilities. Running a workspace script there fails with an unknown-operation error; use the workspace runner instead. There is no capability-backed REPL in this milestone.

### Failures and authorization

- Missing `--allow-read ROOT`: nonzero exit with authorization guidance, before opening the workspace. There is no implicit current-directory grant.
- Missing/unreadable script or invalid UTF-8: file-loading error; files above 64 KiB are rejected.
- Syntax/runtime error: script name and UTF-8 byte offset on stderr, with nonzero exit. Prior print output and reads are not rolled back on runtime failure; syntax errors run nothing.
- Host limit: select a smaller root or reduce calls. No partial/truncated report is silently returned.
- Changed document: rerun the script to rediscover revisions. Discovery/inspection are not an atomic workspace snapshot.
- Wrong operation or nonempty arguments: rejected; scripts cannot replace host-owned paths or revisions.

For a quick denied scenario (expected nonzero exit):

```sh
cargo run -p scope-script --example workspace --offline -- crates/scope-script/examples/handoff-workspace
```

Record real attempts and friction in the [dogfooding log](../../docs/design/SCRIPT_DOGFOODING_LOG.md). Add observations only after trying a task; use its template to distinguish Scope Script usability from Scope API or Cell concerns.

## Language

```text
let x = 5
let y = x * 2
print(y)
let operations = scope.operations()
let operation = operations[0]
let result = scope.invoke(operation.provider, operation.action, {options: {include_text: true}})
print(result.text)
```

- Values: signed 64-bit integers, booleans, double-quoted strings, null, arrays and objects. Arrays/objects are needed to expose descriptors and pass/consume recursive operation payloads.
- Immutable `let` bindings; rebinding and shadowing builtins are errors.
- Checked integer arithmetic `+ - * / %`, unary `-`, comparisons `== != < <= > >=`, boolean `! && ||`, and parentheses. Boolean operators short circuit; there are no implicit coercions.
- Object field projection and string-key indexing; zero-based array indexing. Missing fields/out-of-bounds indices are errors.
- Builtin calls only: `print(value)`, `scope.operations()`, `scope.invoke(provider, action, object)`.
- Newlines or semicolons separate statements. Newlines inside parentheses/collections are allowed. String escapes: `\n`, `\r`, `\t`, `\"`, `\\`.
- No loops, user functions, modules, packages, async, subprocesses, IO primitives, or general-purpose static typing.

## Embed

```rust,ignore
use scope_script::{Interpreter, host::RegistryHost};

// registry, session, expected revision and call-aware policy belong to the host.
let mut host = RegistryHost {
    registry: &registry,
    session: &mut session,
    expected: snapshot.revision.clone(),
    policy: Some(&policy),
    results: vec![],
};
let mut script = Interpreter::new();
let value = script.eval("let x = 5; print(x * 2)", &mut host)?;
let printed = script.take_output();
```

`RegistryHost` projects the actual registry descriptors, including recursive input/output schemas, permissions, effect and execution declarations. It does not cache another registry or implement schema validation: invocation delegates to `OperationRegistry::invoke`, including input/output budgets, closed recursive schemas, source checks and call-aware authorization. A missing policy denies invocation. This experimental adapter additionally rejects non-read operations, even if the supplied policy would allow them; it never proposes, applies, or saves edits.

Scripts supply provider and action as separate strings, not parsed concatenated identities. The host retains the opaque expected revision and document session; scripts cannot fabricate or replace either. `host.results` retains complete accepted `OperationResult`s with original document identity/revision; scripts receive their structured `value`. Structured status/diagnostic envelopes are ordinary values, never interpreted as successful evaluation or authorization merely because invocation succeeded.

`Error { offset, kind }` uses UTF-8 byte offsets. `ErrorKind::Operation` retains the original `OperationError` (validation reason, authorization, stale revision, provider failure, invalid output, etc.); `ErrorKind::Tool` can retain `ToolError` for session consumers. Syntax, runtime and resource errors are separate variants. No flattening to strings is required at the embedding boundary. A source is fully parsed, and duplicate bindings checked, before execution. Runtime failures stop subsequent statements but **do not roll back** earlier bindings, print output, or host calls. Hosts can drain output even after failure.

The minimal `Host` trait is only an interpreter boundary for embedding, not an operation registry. Hosts choose the actual Scope entry point and remain responsible for correct authority and error/status interpretation.

## Limits and trust

Source strings are limited to 64 KiB; nesting/expression depth to 64. Evaluation has finite work/step budgets and approximately 1 MB accounted retained values/output. Drain output between REPL evaluations. Registry history is limited to 64 results and 1 MB accounted value size; hosts can drain `results` explicitly. A retention failure after dispatch does not undo the read. These are logical budgets, not exact allocator-memory limits. Host registry metadata/provider allocations and execution are governed by existing Scope limits and trust boundaries.

The interpreter does not directly access files, environment, network, or subprocesses. The standalone CLI reads stdin or an explicitly selected local script file; only the workspace example host opens an explicitly authorized workspace. Examples/tests perform explicit host-side filesystem setup. Native providers are trusted synchronous code: read metadata is not a purity proof, and scripts do not add provider sandboxing, interruption or rollback. Scripts cannot invoke anything beyond installed host providers and the host's policies. This is an in-process experiment, not an untrusted-code security boundary or remote protocol.

## Validation

```sh
cargo test -p scope-script --offline
cargo fmt --all -- --check
cargo test -p scope-ide --features tools --tests --offline
```

Tests cover pure expressions, bindings/calls, resource limits, parse-before-effects, recursive discovery/payloads, invalid nested input before policy/provider dispatch, unknown operations, absent/denied authorization, retained identity/revision, stale source, structured provider/output failures, persistent REPL bindings, bounded result history, and reuse of the existing semantic inspection operation. No network service or model is needed.
