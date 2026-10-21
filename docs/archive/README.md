# Historical records and evidence

This archive preserves completed reports, investigations, and superseded workflow
proposals. It is research/implementation history, not the current setup or command
reference. Start with [getting started](../getting-started.md),
[Script](../scope-script.md), [architecture](../architecture.md), and
[development](../development.md). Active ownership/authority contracts remain in
[design](../design/README.md).

## Script and developer workflows

- [Authoring and experiment discovery](SCOPE_SCRIPT_AUTHORING_AND_EXPERIMENTS.md)
- [Interactive workbench discovery](INTERACTIVE_SCRIPT_WORKBENCH_DISCOVERY.md)
- [SS2 scratch runner](SCOPE_SCRIPT_SS2_RESULTS.md)
- [SS3 conditional orchestration](SCOPE_SCRIPT_SS3_RESULTS.md)
- [SS4 trial runner](SCOPE_SCRIPT_SS4_TRIAL_RUNNER.md)
- [SS4-B downstream Path integration](SCOPE_SCRIPT_SS4_B_RESULTS.md)
- [DX1 saved-file workflow](SCOPE_SCRIPT_DX1_RESULTS.md)
- [DX2 output records](SCOPE_SCRIPT_DX2_RESULTS.md)
- [Rich completion results](RICH_COMPLETION_RESULTS.md)
- [Terminal input integration audit](TERMINAL_INPUT_INTEGRATION_T1.md)

## Discovery and interoperability investigations

- [Focused agent discovery results](FOCUSED_AGENT_DISCOVERY_RESULTS.md), with
  [original deterministic transcript](FOCUSED_AGENT_DISCOVERY_TRANSCRIPT.json)
- [Structural interoperability discovery](STRUCTURAL_INTEROPERABILITY_DISCOVERY.md)
- [Structural interoperability results](STRUCTURAL_INTEROPERABILITY_RESULTS.md)
- [Structural interoperability summary](STRUCTURAL_INTEROPERABILITY_SUMMARY.md)

## Protocol experiments and complete captures

- [Q1 single-decision compatibility](Q1_SINGLE_DECISION_COMPATIBILITY.md)
- [Q2 live Qwen experiment](Q2_LIVE_QWEN_PROTOCOL.md), [full evidence](q2-live/)
- [Q3 sequential tool serving](Q3_SEQUENTIAL_TOOL_SERVING.md), [full evidence](q3-live/)

Both capture directories retain all original manifests, classifications, messages,
results, traces, and transport/process records where present. A moved record's
historical validation claims apply to its recorded revision/environment, not a new
validation run. Shell commands and backticked source locations are generally
repository-root-relative; Markdown links resolve from the containing document.

## Placement audit

Before moving these records, repository Rust/Python/TOML source and tests were
searched for their filenames and `docs/design` references. No moved record or
capture directory was a source/test input. The unrelated `m2b_live.py` and
`m2c_live.py` default output directories remain untouched. No code, tests, or
capture evidence was deleted. Relative Markdown destinations were resolved from
the old document location and rewritten against the new location; references
between moved records continue to point into this archive.

Some older records cite reports/fixtures absent from this checkout. Those missing
items are explicitly labeled rather than silently mapped to unrelated evidence.
