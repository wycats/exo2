<!-- exo:10153 ulid:01kmzxbcyhgq7ahcxcjevm97c6 -->


# RFC 10153: Design Axioms

- **Supersedes**: RFC 0002


## Summary

Establish the fundamental principles that shape the Exosuit architecture and workflow.

## Motivation

To ensure consistency and alignment across all code and architectural decisions, we need a set of non-negotiable axioms.

## Detailed Design

### 1. Context is King

**Principle**: The workspace context, with SQLite as the canonical state store, is the single source of truth for the project's state and history.
**Why**: AI agents have limited memory and context windows. They need a reliable, structured place to read the current state and write their progress.
**Implication**:

- Every phase must start by reading the context.
- Every significant action must be recorded in the context.
- If it's not in the context, it didn't happen.
- **Structured Data**: Use SQLite-backed schemas for machine-readable canonical context, with files serving as projections or narrative artifacts where needed.

### 2. Phased Execution

**Principle**: Work is performed in distinct, sequential phases (Plan -> Implement -> Verify), grouped into thematic **Epochs**.
**Why**: Large tasks overwhelm AI agents (and humans). Breaking work into phases ensures that we agree on the "What" before we do the "How", and verify the "Result" before moving on.
**Implication**:

- No code is written until the `implementation-plan.md` is approved.
- No phase is marked complete until `verify-phase.sh` passes.
- We do not "jump ahead" to future phases.

### 3. Living Documentation

**Principle**: Documentation is a tool for thinking, not just a record of what happened.
**Why**: Writing down the plan forces clarity. Updating the documentation _during_ the work keeps the context fresh and accurate.
**Implication**:

- The `walkthrough.md` is updated incrementally.
- Design documents are created _before_ the code that implements them.
- **Provenance Labeling**: Documents must carry metadata (e.g., `status: canonical`) to indicate their maturity.

### 4. User in the Loop

**Principle**: The user is the ultimate arbiter and must be consulted at critical junctures.
**Why**: AI agents can hallucinate or drift. Regular checkpoints ensure alignment with the user's vision.
**Implication**:

- Explicit stops for feedback after Planning and before Transition.
- "Fresh Eyes" reviews to simulate user feedback.

### 5. Inverted Source of Truth (Tooling Independence)

**Principle**: The Workspace is the primary unit of existence; the Extension is a servant that adapts to it.
**Why**: The user's project should be self-contained and portable.
**Implication**:

- Core logic (scripts) resides in the workspace (`scripts/agent/`).
- The Extension "ejects" or updates these scripts but does not hide them.

### 6. Evolutionary Context

**Principle**: Agents should focus on the _delta_ (what changed) to maintain coherence without context flooding.
**Why**: Reading the entire history every time is inefficient.
**Implication**:

- Use `git diff` and "Context Deltas".
- The `walkthrough.md` serves as the narrative delta.

### 7. Testing Philosophy

**Principle**: Integration over Unit. Explicit Expectations.
**Implication**:

- No Snapshots.
- No Regex Assertions.
- Full String Elaboration.

### 8. Schema-Driven Context

**Principle**: All core context must be stored in structured canonical state (SQLite) with strict schemas and typed validation.
**Why**: Agents need precise, typed data.
**Implication**:

- Prioritize canonical SQLite state over document projections for queryable state.
- Automated validation.

### 9. Streaming First

**Principle**: All data processing must be designed for streaming.
**Why**: Latency kills flow.
**Implication**:

- RTD parser, UI renderers, and Agent tools must support incremental updates.
