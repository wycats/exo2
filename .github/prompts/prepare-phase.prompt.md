---
agent: agent
description: Prepares the Implementation Plan for the *next* phase after the current phase is finished.
---

### Phase Staging

Use this prompt **after** `phase-transition` is complete, but **before** starting the new phase in a new chat.

**Goal**: Set the stage for the next phase so the next agent can hit the ground running.

#### 1. Identify Next Phase

- Read current state with `exo status` and `exo plan review`.
- Identify the next phase in the sequence.

#### 2. Draft Implementation Plan

- Use `exo` CLI commands to inspect or stage the next phase's goals, tasks, and supporting context in SQLite-backed state.
- **Goal**: Carry forward the high-level goal surfaced by `exo plan review`.
- **Proposed Changes**: Draft a high-level outline of changes based on `exo idea list`, RFCs, and known requirements.
- **Verification**: Add a placeholder for verification steps.

#### 3. Clean Up

- Remove or archive any stale future-facing notes that are now superseded by the recorded phase state or RFCs.

#### 4. Handoff

- Do **not** start the phase.
- Do **not** write code.
- Just leave the next phase state and supporting docs ready for the next session to review and refine.
