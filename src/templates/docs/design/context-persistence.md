# Design: Agent Context Persistence

## Problem

Where should Exosuit operational state and durable human context live?

## Analysis

### Option A: Repo Policy SQL Projection

**Pros**:

- **Shared State Projection**: Any developer (or agent) cloning the repo gets a git-friendly projection of Exosuit operational state.
- **CI/CD Integration**: Scripts can verify that the context is in a valid state (e.g., `check-docs.sh`) as part of the build.
- **Backup**: The context is backed up along with the code.
- **Review**: Changes to the plan or decisions are reviewed in PRs alongside the code changes.

**Cons**:

- **Merge Conflicts**: SQL projections are generated state and can conflict when multiple agents mutate the same plan.
- **Churn**: High-churn operational state can pollute commit history if committed too often.
- **Staleness**: A projection that is not regenerated becomes misleading generated state.

### Option B: Sidecar or Shadow Policy

**Pros**:

- **Clean History**: Only the "final product" (code) is committed.
- **No Conflicts**: Local state stays local.
- **Personal Operations**: Agents can run without writing Exosuit state into the project tree.

**Cons**:

- **Context Transfer Requires Explicit Sharing**: A new developer/agent needs the sidecar or exported state to resume operations.
- **Drift**: Without version control, the context can easily drift from the code.
- **No Audit Trail**: We lose the history of _how_ we got here (decisions, plans).

## Recommendation

Use the policy that matches the collaboration mode.

The core philosophy of Exosuit is "Context is King", but operational state is not always a repo document. Repo policy writes SQL projections under `docs/agent-context/`. Sidecar and shadow policy keep operational state outside the workspace. Durable human-authored docs belong under normal documentation locations such as `docs/design/`, `docs/research/`, and `docs/specs/`.

To mitigate conflicts:

- **Phased Workflow**: The strict phased approach minimizes concurrent editing of the same state.
- **Generated Projection Discipline**: Treat `docs/agent-context/*.sql` as generated infrastructure, not as human-authored notes.
- **Durable Docs**: Write research, design analysis, and specifications under normal `docs/` locations.

## Decision

`docs/agent-context/` is a repo-policy SQL projection location. It is absent or empty under sidecar/shadow policy. It is not the home for durable human documentation.
