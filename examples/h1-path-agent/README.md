# H1 Path retained-agent harness

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
curl --noproxy '*' --connect-timeout 3 --max-time 10 -fsS http://127.0.0.1:1234/v1/models
examples/h1-path-agent/target/debug/scope-h1-experiment real
```

`fake` scripts only model decisions; it still calls the real Path LanguageService and kernel. `real` uses CellAgent with local LM Studio model `qwen3-30b-a3b` at `http://127.0.0.1:1234/v1`, without credentials or a paid endpoint. Inspect the model list first. The report provides externally bounded commands; the existing adapter also has its own request timeout. Offline builds require dependencies already cached; otherwise populate the cache with a separately authorized online build.

Source freshness is checked against the live `&ScratchBuffer`: its process-local evaluation identity must match, as must the typed source binding's tagged ID, revision, and language. The serialized typed binding remains owner-local; two independent buffer owners can both have scratch ID 0. Serialized snapshots alone therefore do not establish cross-owner freshness. The private evaluation identity is not serialized.

Runs write sanitized snapshots and summaries to ignored `target/results/`, never over the committed historical `evidence/`. No build outputs are included. Evidence copies preserve the original exercise's results, not a claim that every future model run will behave identically. Raw prompts, responses, traces, native tool outputs, local filesystem metadata, and credentials are not included.
