---
status: canonical
last_updated: { { DATE } }
---

# Design Axioms

These are the fundamental principles that shape the Exosuit architecture and workflow. All code and architectural decisions must align with these Axioms.

## 1. Context is King

**Principle**: The `exo` state model is the single source of truth for project operational state.
**Why**: AI agents have limited memory and context windows. They need a reliable, structured place to read the current state and write their progress.
**Implication**:

- Every phase must start by reading state through `exo`.
- Every significant action must be recorded through `exo` commands or in durable docs.
- `docs/agent-context/` is a generated SQL projection only when repo policy is active; human-authored design, research, and spec notes belong under normal `docs/` locations.

## 2. Phased Execution

**Principle**: Work is performed in distinct, sequential phases (Plan -> Implement -> Verify).
**Why**: Large tasks overwhelm AI agents (and humans). Breaking work into phases ensures that we agree on the "What" before we do the "How", and verify the "Result" before moving on.
**Implication**:

- No code is written until the plan is approved.
- No phase is marked complete until verification passes.
- We do not "jump ahead" to future phases.

## 3. Living Documentation

**Principle**: Documentation is a tool for thinking, not just a record of what happened.
**Why**: Writing down the plan forces clarity. Updating the documentation _during_ the work keeps the context fresh and accurate.
**Implication**:

- Task logs are updated incrementally through `exo`, not just at the end.
- Design documents are created _before_ the code that implements them.

## 4. User in the Loop

**Principle**: The user is the ultimate arbiter and must be consulted at critical junctures.
**Why**: AI agents can hallucinate or drift. Regular checkpoints ensure alignment with the user's vision.
**Implication**:

- Explicit stops for feedback after Planning and before Transition.
- "Fresh Eyes" reviews to simulate user feedback.
