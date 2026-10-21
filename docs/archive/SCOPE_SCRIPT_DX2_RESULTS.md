# Scope Script DX2 — readable results without semantic conversion

## Inventory and ownership

- `Interpreter::print` captures ordered, unchanged `ToolValue`s; it does not render text.
- Standalone `src/main.rs` previously printed Rust Debug values and discarded the final value from display. The stateful REPL also used Debug.
- SS2 `runner::RunRecord` owns source/target identities, revisions, full texts, selection, operation, discovery catalog, complete returned envelopes, ordered prints and original interpreter outcome/error. Its shell callback previously dumped every envelope before printing values again.
- `SemanticHost` returns the provider's complete envelope, after authorization and interpreter size checks. `CapturedHost` retains it before Script projections. Failed host calls do not enter the successful-envelope list.
- `examples/structured-report.scope` already composes nested reports and preserves selected upstream fields; SS3 can branch on explicitly documented fields. Projection is not conversion.
- Path SS4-C is a downstream workbench, not an implementation in this checkout. DX1 documents its `ComparisonHost` ownership. Scope cannot verify or redefine its comparison classifications here. Real declaration-language fixtures exercise nested rejection, diagnostics, unknown/partial information and source correspondence through the same structural value boundary.

Presentation belongs in a reusable **Script output module**, not Path handling or `ToolValue`. `presentation::{format_value, format_value_with}` is available to downstream hosts, including a comparison host that opts in. Existing example-specific Debug transcripts are not silently redefined as durable formats.

## Human output

Default file mode still emits only explicit prints. REPL and SS2 now use the same formatter. Scalar-only collections are compact; collections containing nonempty collections are indented two spaces. Strings and keys use JSON escaping; Unicode remains readable. Arrays preserve order; objects preserve their existing BTreeMap key order (not source insertion order).

Defaults bound expansion to depth 8, 20 entries per collection, 200 Unicode scalars per string/key and 200 visited values globally. Layout lookahead is bounded too. Markers such as `<depth truncated>`, `<items truncated: 3 omitted>`, `<node budget exhausted: 99 omitted>` and `<string truncated>` explicitly identify abbreviation. These markers deliberately make an abbreviated preview **not JSON**. Limits apply only to display; canonical values are unchanged. Hosts can choose `FormatOptions`; excessively generous custom limits remain the caller's resource responsibility.

Actual before/after for `examples/daily.scope`:

```text
Before:
Object({"name": String("daily"), "status": String("ok"), "total": Integer(12)})

After:
{"name": "daily", "status": "ok", "total": 12}
```

Nested formatting example:

```text
{
  "nested": {
    "items": [1, 2]
  }
}
```

A 100,000-element array of `1` displays twenty elements followed by `<items truncated: 99980 omitted>`. Empty values remain `null`, `[]`, `{}` and `""`. Missing projections remain interpreter errors, never invented nulls. Errors are outside value presentation; a semantic object containing an error/status field remains an ordinary object. REPL and CLI errors carry one-based line/Unicode-scalar columns and original byte offsets. SS2 retains its source identity/revision diagnostics and full error.

## Summary versus evidence

```sh
scope-script --output summary crates/scope-script/examples/daily.scope
```

Actual stdout:

```text
{"name": "daily", "status": "ok", "total": 12}
Final value: null
Interpreter status: success
```

The last statement is `print`, hence the final Script result is null. Interpreter completion says nothing about semantic acceptance.

SS2 displays source/target provenance, operation identity, count of returned envelopes retained, integration-defined semantic status, bounded prints and final result or diagnostic. It no longer redundantly dumps every retained envelope. The envelope count is **not** a count of attempted host calls or semantic successes. A denied/failed invocation remains an interpreter error; earlier prints and returned envelopes survive. `RunRecord` does not contain a complete attempted-invocation ledger, and this change does not claim otherwise.

For inspection, use existing bindings and projections: `print(result.value)`, `print(result.status)` only when that field is declared by the integration, or `result.value.diagnostics[0]`. REPL bindings persist; full-buffer SS2 runs are fresh. Host code can inspect `ScriptRunner.records`, `RunRecord.results`, registry retained results and the original outcome. No reflection command or query language is needed. Unknown contracts/types remain unknown; matching shapes never imply semantic compatibility.

## Safe JSON subset

```sh
scope-script --output json crates/scope-script/examples/daily.scope
```

Writes one complete JSON document to stdout, never a result file. It includes:

- `schema: "scope-script.execution"`, `version: 1`;
- complete UTF-8 source text and supplied source path;
- ordered full `printed` values;
- tagged `outcome`: `status: "success"` with `value` (including null), or `status: "error"` with message, original byte offset, line, column and every available error-source-chain message;
- `host: {"mode":"no-host","invocations":[]}`.

This versioned contract is explicitly **standalone/no-host only**, not a serialization of `RunRecord`. Evaluation errors still exit 1, with their record on stdout and no duplicate stderr error. File I/O, argument, UTF-8 and source-size setup failures happen before a record and use stderr/exit 1. Error messages are diagnostics, not stable typed error discriminants. Source paths use serde's path serialization; non-UTF-8 paths may fail JSON serialization rather than being silently replaced.

`presentation::value_json` converts only existing structural variants using `serde_json`; it preserves full nested envelopes, all object fields and signed 64-bit integers. No nominal types, adapters or semantic interpretation are introduced. Tests parse serialized output back into JSON and compare complete values. There is no typed Script decoder or cross-process replay promise.

### Limits and remaining requirements

- Integers are signed i64; JSON consumers using IEEE-754 numbers lose precision outside ±(2^53−1). Use an integer-preserving consumer. There are no floating-point, binary or nominal variants to convert.
- Existing source limit is 64 KiB; interpreter work/retention budgets and host limits still apply. JSON has **no preview truncation** or extra export-size cap and requires memory proportional to full values/source. Recursive conversion requires stack proportional to input depth. Foreign host values must already be safely bounded; arbitrary untrusted-depth values are not a supported serialization input. Standard serde_json readers also have nesting limits.
- JSON includes potentially sensitive full source and printed evidence. It grants no file-writing authority. Shell redirection, if explicitly chosen by a developer, is separate from Script authority.
- A durable host record would need stable identity/revision/selection serialization, complete catalog and envelopes, typed errors with original diagnostics/chains, attempted invocation identity/arguments/status/provenance and contract version policy. `RunRecord` currently lacks a durable schema and complete failed-call ledger. Exporting just its Debug strings would silently weaken evidence, so that is deferred.
- Downstream hosts can serialize their retained envelopes with `value_json`, but must define their own full record/provenance contract. No JSON mode is advertised for Path/Forth or the example runners.

## Validation

`cargo test -p scope-script --offline` passes. Coverage includes deep arrays/objects, Unicode and escaped keys/strings, empty values, huge collections, all preview limits, i64 extremes, JSON roundtrips, source locations, CLI success/runtime/syntax errors, no-host authorization boundaries and no created output files. Existing SS3, real semantic rejection/authorization fixtures, SS2 full envelope/provenance retention, registry, REPL and terminal tests pass with updated presentation expectations. A new real semantic fixture test verifies bounded formatting does not mutate complete envelopes and distinguishes missing/null/error. Path itself is not available in this workspace and was not run.
