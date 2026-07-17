<!-- exo:108 ulid:01kg5kp2gcvc1zkz6s72s42mtt -->

# RFC 108: Refined Staged RFC Process

- **Supersedes**: RFC 0106, RFC 10151



# RFC 0108: Refined Staged RFC Process

## Summary

This RFC adapts the rigorous [TC39 Process](https://tc39.es/process-document/) for the Exosuit project. It defines a strict set of **Entrance Criteria**, **Exit Criteria**, and **Artifacts** for each stage of the RFC lifecycle. The goal is to replace "YOLO Mode" (unilateral agent decisions) with a "Consensus Mode" where the User (Committee) and Agent (Champion) collaborate through defined stage gates.

## Motivation

- **Ambiguity**: Agents often don't know when to stop planning and start coding.
- **Drift**: Without strict gates, implementation often diverges from the design.
- **Consensus**: We need a formal mechanism for the User to say "Yes, this is exactly what I want" before expensive tokens are spent on implementation.
- **Steering**: The transition between stages is the perfect moment for the User to provide "Steering Instructions" (constraints, preferences, axioms).

## The Roles

- **The Committee (User)**: The ultimate arbiter of consensus. Only the User can approve a transition from Stage 1 to 2 (Design Approval) and Stage 2 to 3 (Implementation Approval).
- **The Champion (Agent)**: Responsible for doing the legwork—drafting, specifying, implementing, and verifying. The Agent drives the process but requires the Committee's key to unlock the next stage.

## The Stages

### Stage 0: Strawman

- **Purpose**: "I have an idea."
- **Entrance Criteria**: None. Any idea can be a Strawman.
- **Artifacts**:
  - A markdown file in `docs/rfcs/stage-0/` (e.g., `my-idea.md`).
  - **No Numbering**: Stage 0 RFCs are identified by their filename handle, not a global ID.
  - Rough `Summary` and `Motivation`.
- **Exit Criteria**: The User agrees the problem is worth solving.

### Stage 1: Proposal

- **Purpose**: "We agree this is worth doing."
- **Entrance Criteria**:
  - User approval of the problem statement ("Vision Steering").
  - **Strategic Alignment**: Does this fit the roadmap defined in **RFC 012**?
- **Artifacts**:
  - **Numbering**: Assign the next available global ID (e.g., `0035-my-idea.md`).
  - Moved to `docs/rfcs/stage-1/`.
  - Expanded `Detailed Design` section (high-level architecture).
  - Identification of "Cross-Cutting Concerns" (e.g., impact on other agents/axioms).
- **Exit Criteria**: The User agrees with the _shape_ of the solution.

### Stage 2: Draft (The Spec)

- **Purpose**: "Here is exactly how it works."
- **Entrance Criteria**:
  - User approval of the high-level design ("Architectural Steering").
- **Artifacts**:
  - Moved to `docs/rfcs/stage-2/`.
  - **Complete Spec Text**: All APIs, data structures, and algorithms defined.
  - **Formal Spec (Optional)**: For complex technical features (e.g., parsers, protocols), a formal spec in `docs/specs/` is drafted.
  - **Implementation Plan**: A generated task/execution artifact backed by canonical SQLite state and validated (RFC 0030).
  - **User Journeys**: Explicit "Zero to One" walkthroughs defined (RFC 0030).
- **Exit Criteria**:
  - The User agrees the spec is complete and the plan is sound.
  - **Axiom Check**: The plan explicitly aligns with `AGENTS.md` and the relevant stabilized axioms RFCs.
  - **No new design work should happen after this point.**

### Stage 3: Candidate (The Build)

- **Purpose**: "It is built."
- **Entrance Criteria**:
  - User approval of the Spec and Plan ("Implementation Steering").
  - **Coherence Check**: `exo check coherence` passes (RFC 0030).
  - **Spec Verification**: If a formal spec exists, the implementation must demonstrably match it.
- **Artifacts**:
  - Moved to `docs/rfcs/stage-3/`.
  - **Implementation**: The code exists in `src/` or `packages/`.
  - **Tests**: Integration tests (as defined in Axiom 7) pass.
  - **Drift Check**: Task views remain consistent with canonical SQLite task state (RFC 0030).
- **Exit Criteria**: The feature is verified to work as designed.

### Stage 4: Stable (The Law)

- **Purpose**: "It is the law."
- **Entrance Criteria**:
  - The feature is verified and "shipped".
  - **Governance Steering**: User confirms readiness for stabilization.
- **Artifacts**:
  - Moved to `docs/rfcs/stage-4/`.
  - **Documentation Sync**: The feature is documented in the relevant Stage 3/4 RFCs (RFC 012).
  - **Spec Finalization**: Any formal specs in `docs/specs/` are finalized and treated as the "Record of Reality" for implementers.
  - **Legacy Purge**: Any superseded design docs are archived or deleted (RFC 012).
  - **Axiom Update**: Any new principles are promoted to the stabilized axioms RFC set.
- **Exit Criteria**: The RFC is now history; the Manual and Specs are the reality.

## The Steering Process

Transitions are not automatic. They are **Steering Events**.

- **Transition 0->1**: User provides "Vision Steering" (Is this the right problem?).
- **Transition 1->2**: User provides "Architectural Steering" (Is this the right solution structure?).
- **Transition 2->3**: User provides "Implementation Steering" (Code review, edge cases).
- **Transition 3->4**: User provides "Governance Steering" (Is this ready to be law?).

## The Planning Cycle (Meta-Workflow)

In addition to the linear lifecycle of a single RFC, there is a periodic **Planning Cycle** that manages the project's roadmap.

- **Goal**: Review the landscape of RFCs (especially new Strawmen) and update the Project Plan to incorporate them effectively.
- **Trigger**: Start of a new Epoch, or on-demand when the backlog grows.
- **Process**:
  1.  **Review**: Scan all Stage 0 RFCs and the existing Backlog.
  2.  **Triage**:
      - _Promote_: Select high-value Strawmen to move to Stage 1 (Proposal).
      - _Defer_: Mark lower-priority items for a future Epoch.
      - _Reject_: Move to `docs/rfcs/withdrawn/`.
  3.  **Proposal**: The Agent builds a proposal for changing the project plan and any derived plan outline artifacts.
      - "I propose we add RFC 'X' to Phase 'Y'."
      - "I propose we defer RFC 'Z' to make room."
  4.  **Consensus**: The User reviews and approves the updated Plan.

## Tooling Implications

The `exo` CLI (RFC 0132) and the Workflow Automation (RFC 0030) are the **Enforcement Mechanisms** for this process.

- **RFC 0030 Relationship**:
  - **Stage 2 Gate**: RFC 0030's `exo phase prepare` is the tool that generates the execution artifact required for Stage 2.
  - **Stage 3 Gate**: RFC 0030's `exo check coherence` is the tool that verifies the "Coherence Check" required for Stage 3.
  - **State Machine**: RFC 0030 manages the _execution_ state (`Active`, `Completed`), while this RFC manages the _decision_ state (`Stage 1`, `Stage 2`).

- `exo rfc promote <id>`: Should prompt for confirmation and run specific checks for the target stage (e.g., "Does the execution artifact exist in canonical state?" for Stage 2).
