<!-- exo:12 ulid:01kg5kp2bfkn6748247m95ee2j -->

# RFC 12: North Star User Journey

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

---
status: provisional
---

# North Star User Journey: The Exosuit Experience

## Context

This document describes the "North Star" user journey for Exosuit, assuming the VS Code extension is fully mature and the underlying scripts have been migrated to Node.js (TypeScript) for tighter integration.

## The Setup

**Prerequisites**:

- VS Code with Exosuit Extension installed.
- Node.js v24+ (for direct execution of `.ts` scripts without compilation).

## Chapter 1: Genesis (Initialization)

**The Goal**: Bootstrap a new project with Exosuit's philosophy.

1.  **Action**: User opens a fresh folder in VS Code and runs `Exosuit: Initialize Project`.
2.  **UI**: The **Context Pane** opens with a "Welcome Wizard".
3.  **Interaction**:
    - **Wizard**: "What is the core mission of this project?"
    - **User**: "A personal finance tracker that respects privacy."
    - **Wizard**: "Let's define your primary Mode. What kind of partner do you need?"
    - **User**: "A strict pair programmer."
4.  **System**:
    - Generates `AGENTS.md` with the mission statement.
    - Creates `docs/design/axioms.md` with a "Privacy First" axiom.
    - Scaffolds the `docs/agent-context` directory.
    - Generates `scripts/agent/bootstrap.ts` (Node/TS).

## Chapter 2: Defining the Soul (Modes & Axioms)

**The Goal**: Establish the rules of engagement and design principles.

1.  **User**: Opens Chat (`@exosuit`). "I want to define how we handle database migrations."
2.  **Agent (Thinking Partner Mode)**: "That sounds like a design decision. Shall I draft a design doc?"
3.  **Action**: Agent creates `docs/design/db-migrations.md` and opens it in a split pane.
4.  **Interaction**:
    - User and Agent iterate on the doc in the chat.
    - **Context Pane**: Shows the draft in the "Pending Designs" list.
5.  **Promotion**:
    - User is satisfied.
    - User clicks the **[ Promote to Axiom ]** button in the Context Pane.
    - **System**: Moves content to `docs/design/axioms.md` and marks the design doc as `status: canonical`.

## Chapter 3: The First Epoch (Planning)

**The Goal**: Structure the work into high-level Epochs and Phases.

1.  **User**: "Let's plan our first milestone."
2.  **Agent (Chief of Staff Mode)**: "Let's define Epoch 1. What is the theme?"
3.  **User**: "Epoch 1: Core Data Models."
4.  **UI**: The **Context Pane** switches to the "Plan View".
    - Displays a timeline visualization of Epochs.
    - User clicks "+" to add "Phase 1: Schema Design".
5.  **Action**: User clicks **[ Start Phase 1 ]**.
6.  **System**:
    - Runs `scripts/agent/phase-start.ts`.
    - Scaffolds `docs/agent-context/current/`.
    - Updates Context Pane to show the "Phase Dashboard".

## Chapter 4: Research & Discovery

**The Goal**: Investigate a technical unknown before coding.

1.  **User**: "I'm not sure if we should use SQLite or DuckDB."
2.  **Agent**: "Let's open a research track."
3.  **System**: Creates `docs/agent-context/research/sqlite-vs-duckdb.md`.
4.  **Interaction**:
    - Agent searches the web (using `vscode.lm` tools) and populates the research note.
    - **Context Pane**: Shows a "Research" tab with active investigations.
5.  **Decision**:
    - User: "Let's go with SQLite."
    - Agent: "I'll log that decision." -> Records an RFC under `docs/rfcs/`.

## Chapter 5: The Deep Work Loop (Maker Mode)

**The Goal**: Implement the plan with high coherence.

1.  **Context Pane**: Shows the "Task List" for Phase 1.
2.  **Action**: User clicks the "Play" (▶) button next to "Task 1: Create User Schema".
3.  **System**:
    - Sets "Active Task" in the status bar.
    - Injects "Task 1" context into the Agent's system prompt.
4.  **Coding**:
    - User writes code in `src/schema.ts`.
    - User: `@exosuit Generate a migration for this.`
    - Agent: Generates the migration code, strictly following the "Privacy First" axiom defined in Chapter 1.
5.  **Logging**:
    - User finishes the task.
    - User clicks **[ Complete & Log ]** in the Context Pane.
    - **System**:
      - Marks task as `[x]`.
      - Prompts User: "What should go in the Walkthrough?"
      - Appends the summary to `docs/agent-context/current/walkthrough.md`.

## Chapter 6: The Gatekeeper (Phase Transition)

**The Goal**: Verify quality and move to the next phase.

1.  **Action**: User clicks the **[ Verify Phase ]** button in the Context Pane.
2.  **System**:
    - Executes `scripts/agent/verify-phase.ts` (Node/TS).
    - **UI**: Shows a "Phase Report Card" in the Webview.
      - Tests: Pass.
      - Lint: Pass.
      - **Coherence**: Warning! "An RFC mentions 'SQLite', but `package.json` has `pg` installed."
3.  **Resolution**:
    - User: "Oops, good catch." Fixes the dependency.
    - User clicks **[ Re-Verify ]**. -> All Green.
4.  **Transition**:
    - User clicks **[ Complete Phase ]**.
    - **System**:
      - Runs `scripts/agent/phase-transition.ts`.
      - Archives `current/` to `archive/2025-11-27_Phase1/`.
      - Updates `plan-outline.md` (Marks Phase 1 as Complete).
      - Prompts: "Ready to start Phase 2?"

## Chapter 7: Evolution (Refining the System)

**The Goal**: The system improves itself.

1.  **User**: "I want to add a new check to the verification script."
2.  **Agent**: "Since we are using Node.js scripts, I can add a new `Check` class to `scripts/agent/checks/`."
3.  **Action**: Agent implements the new check.
4.  **Result**: The next time the user clicks **[ Verify Phase ]**, the new check is automatically included.

