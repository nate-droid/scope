---
name: verify-change
description: Select and run risk-based verification for a Scope diff or completed change using docs/verification.md. Use for verification planning, checking existing evidence, or running required checks; not for implementation or an automatic workspace-wide test sweep.
---

# Verify change

1. Establish the exact diff/revision and current worktree status. Use the requested range, or the staged and unstaged diff plus relevant untracked files for a local change. Read repository `AGENTS.md` and the authoritative `docs/verification.md` before selecting checks; use `docs/development.md` for target/feature details when needed.
2. Map changed behavior and boundaries to direct, adjacent, and required system checks under that policy. State the selected scope and escalation rationale briefly; do not invent a second verification matrix.
3. Inspect supplied or recorded evidence first. Distinguish current-source results from historical claims; reuse results only when revision, local changes, target, features, and environment support it. Do not repeat equivalent checks without an unresolved question. If asked for a plan or read-only evidence audit, report commands and gaps without executing them.
4. Otherwise run the smallest meaningful required checks from the repository root, following the policy's target selection, artifact reuse, timeout, and escalation rules. Do not edit source, weaken tests, clean artifacts, or install dependencies to force a pass. Report blockers rather than silently reducing required coverage.
5. Report exact commands, features, actual results/test counts, evidence provenance, omissions with reasons, and remaining assurance gaps. Clearly separate checks run now, reusable evidence, and proposed checks. Stop after verification; leave fixes to a separate implementation task.
