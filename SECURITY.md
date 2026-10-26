# Security

Scope is an experimental, MIT-licensed Rust project with unstable 0.1 APIs. There is no established supported-version or security-maintenance policy, no established security contact, and no guaranteed response or remediation time. Do not assume older releases receive fixes. This document is not a claim that Scope, its dependencies, or downstream integrations have undergone a security audit.

## Report vulnerabilities privately

A working private vulnerability-reporting channel must be established and documented before public launch; it remains a launch prerequisite, not an available service promised by this policy. No hosting service or private reporting facility is designated here.

1. If the repository where you obtained Scope has private vulnerability reporting enabled, use that facility.
2. Otherwise, do not disclose exploit details publicly. Request a private reporting channel through an available repository communication mechanism, without including exploit steps, vulnerable payloads, credentials, or sensitive data. Wait for a private channel before sending the report.

No email address or other dedicated contact is designated here. Private reporting availability and response times are not guaranteed. If no private channel is available, keep exploit details private rather than treating a public issue as a substitute.

Once a private channel is established, provide the affected Scope and Cell revisions, Rust toolchain, enabled features, deployment context, expected and observed behavior, impact, and a minimal reproduction using synthetic data. Explain which authority boundary is crossed. Remove credentials, private source, personal data, and sensitive logs before sharing artifacts.

## Trust and execution boundaries

### Native extensions are trusted host code

Rust `LanguageService` and `ToolProvider` implementations, interactive language adapters, and other native host integrations run in the host process. Register only implementations you trust. Rust traits, read-only method signatures, tool-effect declarations, and dispatch authorization are contracts and checks, not a sandbox against malicious native code or ambient host access.

Providers must honor host authorization and must not bypass Cell by directly accessing filesystem, network, process, or other host resources. Scope's checks do not make an arbitrary native plugin safe to load. Review native dependencies and integrations as part of the trusted computing base.

### Cell enforces the WASM boundary

Scope describes, validates, discovers, binds, and orchestrates typed operations; Cell supplies the constrained WebAssembly component runtime and enforces installed capabilities and execution policy. The host must choose trusted capability implementations, narrow filesystem roots and permissions, and appropriate resource limits. Native capability implementations remain trusted host code.

Capability requirements, tool declarations, composition manifests, and discovery metadata describe intended operations; they do not grant authority. Importing a manifest or binding a component does not supply permission grants. Actual runtime authority depends on the host's Cell configuration. Do not confuse a validated plan with a security approval, or extend Cell's WASM isolation claims to native `LanguageService` or `ToolProvider` implementations.

See [README.md](README.md) and [Development](docs/development.md) for the architecture and capability setup. Review Cell too: workspace dependencies use https://github.com/nate-droid/cell at tag `v0.0.2`, and Scope's Cargo lockfile pins the resolved Git commit. No sibling Cell checkout is required; Scope crates still use local path dependencies.

## Model data and privacy

Agent functionality is opt-in and uses a host-configured model endpoint. Information included in model context leaves the local process for that endpoint; assess its retention, training, access, and privacy policies before sending sensitive data. Model calls may incur costs. Do not put credentials in source, prompts, fixtures, or logs.

For agent-assisted source editing, the documented context includes the instruction, selected target buffer ID, language identifier, version, and exact source; it does not automatically attach other buffers, workspace listings, paths, or session history. That source may itself contain secrets or personal information. Other agent/tool flows and downstream providers may expose different context; do not treat the source-edit context limit as a universal privacy guarantee. See [structured source edit review](docs/integration.md).

Treat model output and source-embedded instructions as untrusted input, never as authorization. Review proposed edits and tool calls; keep host approval and capability decisions independent of model assertions. Avoid logging sensitive prompts, responses, tool results, or source text.

## Proposal review and persistence

Isolated filesystem text proposals operate on exact host-approved snapshot paths without baseline writes during generation. Human acceptance performs checked, sequential application, not an atomic multi-file transaction. A later conflict or failure can leave earlier changes applied; inspect the resulting workspace before retrying and keep backups or version control appropriate to your workflow. Revision checks are not a guarantee against every concurrent-writer race, and host side effects are not generally rolled back.

Source-buffer proposals are a distinct workflow: acceptance revalidates the original source/version and changes the target buffer in memory; saving and evaluation remain separate explicit operations. Do not assume reviewing or accepting a proposal makes its code safe to execute. See [isolated text proposals](docs/agents.md), [structured source edit review](docs/integration.md), and [scratch buffers](docs/integration.md) for the specific contracts and persistence limitations.
