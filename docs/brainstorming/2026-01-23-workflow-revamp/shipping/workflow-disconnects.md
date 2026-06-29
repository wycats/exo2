# Workflow Disconnects and Gaps - 2026-01-23

> **Update 2026-02-03**: Progress tracked below. See [RFC 00224 (SOAR Loop)](../../../rfcs/stage-1/00224-the-soar-loop-a-workflow-model-for-human-ai-collaboration.md) and [RFC 00225 (Problems Pane)](../../../rfcs/stage-1/00225-problems-pane-integration-with-soar-loop.md).

## Core Disconnects

The biggest gaps are between:

1.  🟢 **Workflow vs. Practice**: The intended workflow and a clear set of intuitive, easy to remember and follow practices (for both of us) that will actually result in following the workflow.
    - **Status**: ADDRESSED by RFC 00224 (SOAR Loop). The loop formalizes Status→Orient→Act→Review as the tactical workflow.
2.  🟡 **Visibility**: Visibility into the workflow for me that maps very tightly onto what we're trying to do at any given moment.
    - **Status**: PARTIALLY ADDRESSED. SOAR provides conceptual visibility; Dashboard Expansion epoch planned for UI work.
3.  🟡 **Idea Integration**: More direct and tight integration between the theory of "the user might want to provide ideas" and actual practices where I provide the ideas and we implement them in a structured order that keeps the wheels on but isn't overly paternalistic.
    - **Status**: PARTIALLY ADDRESSED. Ideas triage completed (60+ ideas categorized), but tooling gap remains.
4.  🔴 **RFC/Phase Integration**: A more systematic integration between the staged RFC process and the phased process. We have quite a few tools for talking about RFCs, but they replaced some earlier "future" planning concepts that were simpler on the grounds that RFCs are more powerful (and they are!), but they're just not connected to the workflow.
    - **Status**: OPEN. RFC 00224 references RFC pipeline but doesn't integrate it into SOAR phases.
5.  🔴 **Lost Concepts**: The loss (or underemphasis) of at least five key concepts from earlier on that were meant to help drive things but have gotten de-emphasized and aren't part of the workflow:
    - **Axioms**: Meant to reflect the core ideas of the project so that we could vet future ideas against them.
    - **Personas/Work Modes**: Meant to reflect groups of people who would use a project, to make sure that we're doing a good job serving the needs of the diverse constituents of a project using exo.
    - **Implementation Plan**: Originally a separate file from the task list. Fleshing it out was meant to be an important part of planning and executing the phase. Merging it with the task list has led to a loss of the "planning" aspect.
    - **Walkthroughs**: Meant to be a dedicated file created with the phase, serving as a validation target and feedback mechanism. Since they weren't consistently created, the feedback loop on "what are we building" deteriorated.
    - **The Manual**: RFCs were meant to be reified into a "manual" which, with Axioms, would be the source of truth. Instead, RFCs (which rot) have become the de facto source of truth.
    - **Status**: OPEN. These concepts remain dormant. New axiom proposed: "Clean Pane = Clear Mind" (RFC 00225).

## Downstream Effects

These gaps have led to a degradation in planning capabilities:

- **Weak Future Planning**: No clear way to document the plan for the "next phase", leading to anemic `exo-plan` data and no staged implementation plan.
- **Vague Epochs**: Epochs serve a conceptually important purpose, but without detailed sketching, they remain vague.
- **RFC Detachment**: No nailed-down process for attaching RFCs to phases and epochs.

## UI/UX Gaps

- **Workflow Visualization**: No way to look at the dashboard and feel out whether it's providing the right kind of information.
- **RFC Pipeline Visibility**: No visualization of the RFC pipeline or how phases/epochs advance them. RFCs feel like constraints rather than a structured workflow.
- **Unwired Feedback**: The feedback system isn't wired into the core files, so structured feedback isn't part of the workflow. Refinement of files (RFCs, plans) lacks a feedback loop.
- **Friction**: Infrequently used panes add friction to the mental model.

## Goal

We need to reconnect these concepts. The ideas were good, but the tooling implementation prioritized other things over parity with the original manual workflows.

---

## Progress Update (2026-02-03)

### Completed

| Work                          | Impact                                                                                     |
| ----------------------------- | ------------------------------------------------------------------------------------------ |
| **RFC 00224: SOAR Loop**      | Formalizes Status→Orient→Act→Review as tactical workflow. Addresses Disconnect 1.          |
| **RFC 00225: Problems Pane**  | Proposes VS Code diagnostics integration for Review phase. Addresses Review tool gap.      |
| **SOAR Tool Audit**           | 30 LM tools categorized into SOAR phases. Found 0 Review tools (critical gap).             |
| **Ideas Triage**              | 60+ ideas categorized: 7 implemented, 12 designed, 14 planned, 3 superseded, 8 uncaptured. |
| **Dashboard Expansion Epoch** | Created 4-phase epoch for visualization work.                                              |
| **PER Protocol**              | Prepare→Execute→Review documented in copilot-instructions.md.                              |

### New Axiom Proposed

> **"Clean Pane = Clear Mind"**: A zero-noise Problems pane is a prerequisite for effective steering. (RFC 00225)

### Still Open

- Lost Concepts (Axioms, Modes, Walkthroughs, Manual) not yet integrated into workflow
- RFC/Phase integration not formalized
- UI/UX visualization gaps remain
