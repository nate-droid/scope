---
name: investigate
description: Read-only focused investigation of a Scope question, bug, or proposed change. Use to locate the relevant implementation, establish evidence, and identify the smallest next step; not to implement fixes or run verification suites.
---

# Investigate

1. Establish the question and scope from the prompt. Follow repository `AGENTS.md` and applicable nested instructions; reuse relevant context already established in this thread after checking it is still current.
2. Inspect the worktree status, then search narrowly for the responsible symbols, callers, tests, and directly relevant documentation. Follow `CONTRIBUTING.md` for architectural and authority boundaries. Expand only to resolve a concrete uncertainty.
3. Stay read-only: do not edit files, run builds/tests, install tools, or change configuration. A skill is guidance, not a sandbox; do not assume a Zed profile enforces this boundary for Codex.
4. Return a concise answer with file/line evidence, observed facts versus hypotheses, affected boundaries, and the smallest recommended next step. If checks are needed, consult `docs/verification.md` and suggest them without running them. Stop when the question is answered; do not begin implementation.
