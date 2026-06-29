<!-- exo:114 ulid:01kg5kp2gqwdkg5dyrrz822gyg -->

# RFC 114: Advanced Phase Transition


# RFC 0114: Advanced Phase Transition

## Core Concept

The current "Phase Transition" is a monolithic process. We want to split it into two distinct stages to allow for more flexibility and better long-term planning.

### Stage 1: Close Phase (The "Commit")

This stage is about wrapping up the current unit of work cleanly.

1.  **Verification**: Run automated checks (`verify-phase.sh`).
2.  **Walkthrough Update**: Finalize `walkthrough.md` with the narrative of what was done.
3.  **User Review**: The user signs off on the work.
4.  **Archive**:
    - Commit changes to Git.
    - Move `current/` artifacts to `archive/`.
    - Empty `current/` directory.
    - **State**: The workspace is now "clean" and ready for _something_ new.

### Stage 2: Decide Next Step (The "Pivot")

Once the phase is closed, the user (with AI assistance) decides what happens next.

**Pre-Decision Analysis**:
Before presenting options, the AI reviews:

- Upcoming Epochs/Phases in `plan-outline.md`.
- `deferred_work.md` and `ideas.md`.
- **Goal**: Suggest restructuring or reordering based on "Dependency Order" (logical prerequisites).

**Options**:

1.  **Break for Now**:

    - _Action_: Push code to remote, ensure all documentation is up-to-date, serialize any lingering chat context to `decisions.md`.
    - _Outcome_: Safe to close VS Code.

2.  **Prepare Next Phase/Epoch**:

    - _Action_: Select the next item from the plan.
    - _Prep Work_:
      - If starting a new Epoch: Perform "Epoch Prep" (review goals, high-level strategy).
      - If starting a Phase: Create `implementation-plan.md` and `task-list.md`.
    - _Outcome_: Ready to start a new chat session.

3.  **Fresh Eyes Review**:

    - _Action_: Instantiate a new "Fresh Eyes" phase.
    - _Outcome_: A new phase is created specifically for review, then we loop back to Stage 1.

4.  **Review Ideas / Deferred Work**:

    - _Action_: Interactive session to review the backlog.
    - _Outcome_: Items are promoted to the `plan-outline.md` (inserted into future phases) or dismissed. This is an _inline_ activity that leads back to the "Decide Next Step" menu.

5.  **Meta-Consolidation**:
    - _Action_: Instantiate a new phase for "Housekeeping" (Refining Axioms, Personas, Structure).
    - _Outcome_: A new phase is created, then we loop back to Stage 1.

## Workflow Integration

- **UI Support**: This workflow should be supported by the "Dashboard" (Epoch 6).
- **Chat Participant**: The `@exosuit` participant should guide the user through this decision tree.
- **Serialization**: The critical requirement is that _between_ Stage 1 and Stage 2 (and during the "Break" option), absolutely no context is lost.

## "Dependency Order" Logic

The AI should actively help maintain the plan in a logical dependency order.

- _Trigger_: Between Epochs, or when "Deferred Work" accumulates.
- _Logic_: "We can't do X (Phase 12) because we haven't done Y (Deferred Item) yet."
- _Action_: Suggest inserting a remedial phase or reordering existing phases.

