---
name: review-milestone
description: Independently review a completed Scope milestone or diff and its verification evidence for correctness, regressions, architectural boundaries, and policy gaps. Use for a read-only completion review, not implementation or automatic test reruns.
---

# Review milestone

1. Establish the requested commit range or local diff and completion claims. Inspect worktree status and read the actual diff, including relevant untracked files for a local review; ask for the base only if it cannot be established safely. Follow repository `AGENTS.md` and applicable nested instructions.
2. Review independently of the implementation summary. Trace changed behavior through relevant callers and tests; check correctness, regressions, and the architectural/authority discipline in `CONTRIBUTING.md` and directly relevant docs. Keep the review bounded to the milestone and its affected boundaries.
3. Assess verification evidence against the authoritative `docs/verification.md`, using `docs/development.md` for features/consumers as needed. Match results to source revision, local changes, commands, features, and environment. Historical reports, zero-match tests, timeouts, and missing logs do not prove a current pass.
4. Stay read-only: do not fix findings or run builds/tests. Recommend only checks needed to close specific gaps; use a separate verification task to execute them. For fresh-context independence, prefer a new Codex thread; do not claim a separate reviewer or subagent was used unless one actually was.
5. Lead with actionable findings ordered by severity, with file/line references, impact, and reasoning. Separate confirmed defects from evidence gaps and questions. If none are found, say so and state residual risks/coverage limits; do not turn missing evidence into an invented defect or an unconditional approval.
