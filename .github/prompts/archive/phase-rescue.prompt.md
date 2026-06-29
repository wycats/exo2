---
agent: agent
description: Rescue a phase when a previous session was abandoned or corrupted.
---

### Phase Rescue & Restoration

Use this prompt when starting a new chat to continue a phase after a problematic session.

**Goal**: Re-establish truth by reconciling **Context** (what the tools say) with **Reality** (what's in the code).

#### 1. Orient

- Call `exo-status` to get the current project state.
- Call `exo-phase` to see phase details and task status.

#### 2. Reality Check

For the last "completed" task and current "pending" tasks:

- **Verify in Code**: Check if the expected files/changes actually exist.
- **Verify in Tests**: Check if tests exist and pass.

#### 3. Reconcile

- **If code exists but task is pending**: Use `exo-task-complete` to mark it done.
- **If task is done but code is missing**: Report this as a critical inconsistency.
- **If context is stale**: Describe the discrepancy to the user.

#### 4. Resume

- Identify the next true objective.
- Continue execution.

**Note**: If the tools don't give you what you need to reconcile, report that friction. It's valuable feedback.
