# Workflow Disconnects and Gaps

_Created: 2026-01-23_
_Status: Draft / Living Document_

## Core Loop Disconnects

The biggest gaps are between the intended workflow and the actual practices/tooling.

1.  **Workflow vs. Practice**: A gap between the intended workflow (Plan → Implement → Verify → Transition) and a clear set of intuitive, easy-to-follow practices. We need habits that naturally result in following the workflow.
2.  **Visibility**: Lack of visibility into the workflow. The user needs a map that maps tightly onto what we are trying to do at any given moment.
3.  **Idea Integration**: The theory of "user provides ideas" vs. actual practices. We need a structured way for the user to provide ideas and for us to implement them in a structured order that isn't overly paternalistic.
4.  **RFC vs. Phase Integration**: Poor integration between the staged RFC process and the phased process. RFCs replaced simpler planning concepts but aren't connected to the workflow.
5.  **Lost Concepts**: De-emphasis of key concepts that were meant to drive things:
    - **Axioms**: Meant to reflect core ideas for vetting future ideas.
    - **Personas (Work Modes)**: Meant to reflect diverse user needs.
    - **Implementation Plan**: Originally a separate file, meant to be fleshed out. Merging it with the task list reduced its planning value.
    - **Walkthroughs**: Meant to be part of the phase structure for validation and feedback.
    - **The Manual**: RFCs were meant to be reified into a "Manual" (source of truth), but currently RFCs serve as the source of truth, leading to staleness.
6.  **Planning Degradation**:
    - No clear way to document the plan for the "next phase" or stage the implementation plan for a future phase.
    - Epochs are vague; we lack a process for sketching them out in detail or planning consecutive phases.
    - No nailed-down process for attaching RFCs to phases and epochs.
7.  **UI/Workflow Representation Gap**:
    - No visualization of the RFC pipeline (phases/epochs processing RFCs).
    - Feedback system is not wired into the core files, so structured feedback isn't part of the workflow.
    - "Cruft" in the UI (infrequently used panes) adds friction to the mental model.
    - Lack of "visceral sense" for the user's experience of the system.

## Tooling vs. Workflow History

We implemented tooling based on interests rather than prioritizing parity with original workflows.

- **Lost Workflows**: "Coherence" passes, "Fresh Eyes" reviews.
- **Goal**: Recover the "lost but important project notions" from the repo history.

## Action Items (Meta)

- [ ] Analyze repo history for lost concepts (Subagent).
- [ ] Research market/competitors for blog post differentiators (Subagent).
- [ ] Reflect these issues into phases/epochs to churn through problems.
