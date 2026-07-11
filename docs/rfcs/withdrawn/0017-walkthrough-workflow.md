<!-- exo:17 ulid:01kg5kp2bra3ze5bbqakbe186h -->

# RFC 17: Walkthrough Workflow

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# Walkthrough Editor Workflow

## Philosophy

Verification is a **human responsibility**. The Agent's role is to provide _evidence_ and _context_ to enable the human to make an informed decision. We must strictly distinguish between the Agent _checking_ the system and the User _verifying_ the result.

## The Workflow

1.  **Context**: The user encounters a verification task (e.g., `- [ ] Verify X`).
2.  **Investigation** (Optional):
    - **Explain**: "What does this task mean? What was the original intent?"
    - **Assess**: "Agent, check the current state of the system and tell me if this looks done."
3.  **Decision**:
    - The user toggles the checkbox `[x]` to mark the task as verified.
    - If not verified, the user leaves it unchecked.
4.  **Annotation** (Future):
    - The user leaves a comment explaining _why_ it wasn't verified or noting caveats.

## UI Design

- **Checkbox**: The primary interaction. Represents the **Decision**.
  - _Behavior_: Updates the markdown file immediately. Does **NOT** trigger a chat message (to avoid noise).
- **Action Buttons**: Ephemeral tools that appear on hover/focus. They assist the **Investigation**.
  - _Explain_: Triggers a chat explanation.
  - _Assess_: Triggers an agent evaluation.
- **Footer Actions**:
  - _Submit Review / Complete Phase_: A batch action at the bottom.
  - _Behavior_: Sends a summary to the chat (e.g., "Verified 5/5 tasks") to signal the Agent to proceed to the next phase.

## Terminology Decisions

- **Walkthrough Verification**: We chose **Assess**. It implies evaluating the situation to help the user make a decision.
- **Task List / Plan**: **Status** is reserved for checking the progress of active work items (e.g., "Is this started?", "What's blocking this?").

## Terminology Candidates (Archive)

- **Assess**: Selected.
- **Audit**: Strong runner-up.
- **Inspect**: Implies looking at the code/state.
- **Status**: Reserved for Task List.
- **Dry Run**: Implies trying to verify without committing.

