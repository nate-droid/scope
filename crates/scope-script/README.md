# Scope Script (S4 experiment)

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

The standalone REPL has **no installed providers or authority**. It accepts one statement line at a time (multiple statements may be separated by semicolons), retains immutable bindings, and recovers after evaluation errors. EOF or `:quit` exits. Values are displayed using their Rust structured debug representation, not JSON. Source-string evaluation supports multiline programs and collections. There is deliberately no script-file loader; hosts supply source strings or streams.

For an actual capability-backed document inspection:

```sh
cargo run -p scope-script --example registry --offline
```

The embedding example creates a temporary document **in host setup**, opens a read-only Scope workspace, and installs a downstream demonstration provider in `OperationRegistry`. Its script is [`examples/registry.scope`](examples/registry.scope). The provider inspects the real `DocumentSnapshot`, returning source text and UTF-8 byte count. This is a new example provider, not a claimed pre-existing production registry operation: Scope currently has none. The integration test `tests/semantic_inspection.rs` separately reuses Scope's **existing selection-description example operation**, discovers and invokes it through `AgentSession` / `SemanticToolProvider`, and preserves partial-result diagnostics and source preconditions. Its adapter is explicitly test-local; there is no automatic semantic-to-registry bridge.

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

The interpreter does not directly access files, environment, network, or subprocesses. The CLI uses stdin/stdout only; examples/tests perform explicit host-side filesystem setup. Native providers are trusted synchronous code: read metadata is not a purity proof, and scripts do not add provider sandboxing, interruption or rollback. Scripts cannot invoke anything beyond installed host providers and the host's policies. This is an in-process experiment, not an untrusted-code security boundary or remote protocol.

## Validation

```sh
cargo test -p scope-script --offline
cargo test -p scope-ide --features tools --tests --offline
```

Tests cover pure expressions, bindings/calls, resource limits, parse-before-effects, recursive discovery/payloads, invalid nested input before policy/provider dispatch, unknown operations, absent/denied authorization, retained identity/revision, stale source, structured provider/output failures, persistent REPL bindings, bounded result history, and reuse of the existing semantic inspection operation. No network service or model is needed.
