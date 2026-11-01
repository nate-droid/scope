# H1/H2 Path retained-agent harness

Standalone Cargo workspace; does not change Scope's root dependencies. Required checkout layout:

```text
parent/
├── scope/                    # this checkout, including retained-agent APIs
│   └── examples/h1-path-agent/
└── path/                     # Path checkout with crates/path-scope
```

Both the harness and Path must resolve Scope libraries from this same `scope/` checkout. Path's `scope-language` dependency must not point to another checkout or git/registry source. The committed lockfile pins external dependencies; it does not pin the path-dependent working trees. See [the report](../../docs/h1-experiment.md) for original revisions and the current-working-tree caveat.

From the Scope root:

```sh
cargo build --offline --locked --manifest-path examples/h1-path-agent/Cargo.toml
examples/h1-path-agent/target/debug/scope-h1-experiment fake
examples/h1-path-agent/target/debug/scope-h1-experiment fake-h2
curl --noproxy '*' --connect-timeout 3 --max-time 10 -fsS http://127.0.0.1:1234/v1/models
examples/h1-path-agent/target/debug/scope-h1-experiment real
examples/h1-path-agent/target/debug/scope-h1-experiment real-h2
```

`fake` scripts only model decisions; it still calls the real Path LanguageService and kernel. `real` uses CellAgent with local LM Studio model `qwen3-30b-a3b` at `http://127.0.0.1:1234/v1`, without credentials or a paid endpoint. Inspect the model list first. The report provides externally bounded commands; the existing adapter also has its own request timeout. Offline builds require dependencies already cached; otherwise populate the cache with a separately authorized online build.

`fake-h2` and `real-h2` opt into `run_retained_scratch_agent_semantic_observed`
with `SemanticContextOptions::default()`. Their baseline counterparts retain the
original runner. All four share the exact fixture source, task, decision/tool
budgets, and model settings. Fake modes script only decisions, not Path results;
real modes are optional and were not run for the paired evidence below.

The `DecisionModel` wrapper retains counts only, never raw messages or responses.
`initial_model_message_bytes` measures the first message's UTF-8 role plus content
bytes (normally the system message), not the whole initial request. This existing
field is distinct from `initial_user_message_bytes`, which counts only the first
user message's content (no role bytes). `initial_model_input_bytes` is the actual
full first model input: all message roles plus contents (system + user here),
equal to the first element of `model_request_input_bytes`.
`model_request_input_bytes` measures the sum of role and content bytes for every
message at each `decide` call; `total_model_input_bytes` sums those requests,
including repeated conversation history. These are not token counts, HTTP/JSON
wire sizes, or the runner's `usage.conversation_bytes` accounting.
`first_available_operation_count` comes from the first parsed `catalog.tools` or
`semantic_context.operations` array; null means none was observed.
`model_decisions` counts model requests; `tool_attempts` counts retained tool
evidence. Authorization is annotated unknown at discovery time; actual host
approval remains independently counted in `authorizations`.

`context_assembly` copies only payload-free `ContextAssembly` fields: success,
assembled bytes, budget-bounded status, and sanitized failure. An empty baseline
array means no semantic context assembly was used, not complete context.
`context_supplied` copies only safe `ContextSupplied` fields recorded at the first
model-call boundary: `packet_bytes` is the semantic packet's JSON byte count;
`initial_message_bytes` is the initial user-message JSON content byte count,
excluding its role and the system message. It matches the wrapper's
`initial_user_message_bytes`, not `initial_model_input_bytes`. The baseline array
is empty because this evidence applies to semantic context only. Successful
assembly alone does not prove delivery; `ContextSupplied` records handoff, not
successful inference or transport completion. `bounded: false` means no
budget-induced omission, not that all possible evidence was available.

Paired offline/locked build and fake runs against the real downstream Path produced:

| Metric | `fake` baseline | `fake-h2` semantic |
| --- | ---: | ---: |
| Passed / kernel checked / fresh | true | true |
| First message role + content bytes (system) | 1409 | 1476 |
| Initial user-message content bytes | 1355 | 1566 |
| Full first model input bytes (system + user, including roles) | 2768 | 3046 |
| Per-request input bytes | 2768, 9361 | 3046, 9639 |
| Total model input bytes | 12129 | 12685 |
| First available operations | 1 | 1 |
| Model decisions / tool attempts / authorizations | 2 / 1 / 1 | 2 / 1 / 1 |
| Lifecycle callbacks | 10 | 12 |
| Context assembly bytes | not applicable | 1239 |
| Context supplied packet / initial user-message bytes | not applicable | 1239 / 1566 |

Both stopped `Completed`, with one semantic success, zero malformed decisions,
zero publish errors, unchanged source revision 0, matching `scope.agent.get`, and
a successfully deserialized terminal snapshot. H2 assembly succeeded without
budget bounding or failure, with a matching supplied packet byte count. The
optional local-model probe and real pair were skipped for this refresh. H2 adds 556 measured total input bytes for this small
fixture; this is not evidence of context compression or real-model quality.
Sanitized summaries are checked in as `evidence/h2-baseline-summary.json` and
`evidence/h2-semantic-summary.json`; no new snapshots are included.

Source freshness is checked against the live `&ScratchBuffer`: its process-local evaluation identity must match, as must the typed source binding's tagged ID, revision, and language. The serialized typed binding remains owner-local; two independent buffer owners can both have scratch ID 0. Serialized snapshots alone therefore do not establish cross-owner freshness. The private evaluation identity is not serialized.

Runs write sanitized snapshots and summaries to ignored `target/results/`, never over the committed historical `evidence/`. No build outputs are included. Evidence copies preserve the original exercise's results, not a claim that every future model run will behave identically. Raw prompts, responses, traces, native tool outputs, local filesystem metadata, and credentials are not included.
