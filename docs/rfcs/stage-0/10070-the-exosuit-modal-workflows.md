<!-- exo:10070 ulid:01kmzxeff066mvn8257t010rj7 -->


# RFC 10070: The Exosuit Modal Workflows

- **Superseded by**: RFC 0053


## Summary

This RFC proposes transforming the Exosuit from a static toolset into a **Context-Adaptive Environment**. It defines a UI architecture that adapts to the user's current focus while maintaining global context, and an **Entropy System** that surfaces system health issues at appropriate natural breakpoints.

## Motivation

The current Exosuit workflow is implicit. Users must remember to "switch hats" from coding to planning to architecture. This leads to:
1.  **Context Contamination**: Planning tasks cluttering the coding view.
2.  **Strategic Drift**: Forgetting to update the Plan or RFCs between Epochs.
3.  **Meta Decay**: System integrity (docs, axioms) degrading because "Meta" is an afterthought.

## The Workflow Philosophy

### 1. Modes are Phases
With the exception of "Maker" (the default state) and "Architect" (a parallel track), most "Modes" (Planning, Meta, Strategy) are simply **Phases of Work**.
*   To do deep planning, you start a **Planning Phase**.
*   To fix system integrity, you start a **Meta Phase**.
*   **Implication**: The UI adapts because the *Phase Type* changes, not because the user toggled a switch.

### 2. The Parallel Track (RFCs)
RFC writing and refinement is the only activity that happens *concurrently* with execution.
*   **Workflow**: A user can open a separate "Architect Agent" chat to draft/refine RFCs while the "Maker Agent" is implementing code.
*   **Integration**: Phases are explicitly linked to the RFCs they advance.

## The UI Architecture

### 1. The Maker HUD (Heads-Up Display)
A persistent, low-profile status bar in the Maker Sidebar.
*   **Current Context**: Active Phase & Epoch.
*   **Rearview**: Last completed Phase.
*   **Horizon**: Next scheduled Phase.
*   **Entropy**: A subtle "Health" indicator (Green/Yellow/Red).
    *   *Integration*: Linked to the **Context Inbox** (RFC 10011). Red status indicates critical items in the Inbox.

### 2. The Dual Sidebars
*   **Primary Sidebar (Context-Adaptive)**:
    *   *In Implementation Phase*: Task List + Walkthrough.
    *   *In Planning Phase*: Gantt View + Dependency Graph.
    *   *In Meta Phase*: Integrity Dashboard.
*   **Secondary Sidebar (The Plan)**:
    *   Always available.
    *   Shows the Global Plan / Epoch Plan.
    *   Allows "peeking" at the future without context-switching the main view.

### 3. View State Persistence (The Desk Metaphor)
Switching Phases (Modes) acts as a **Context Switch**, not a reset.
*   **Behavior**: When switching from "Planning" to "Implementation", the system saves the current editor layout (open tabs, scroll positions) as the "Planning Workspace".
*   **Restoration**: Returning to a phase type restores its specific workspace.
*   **Goal**: "Clean Desk Policy" without data loss. Put the papers in the drawer; don't throw them away.
*   **Axiom**: The View is a representation of the File State. Editing a file in one mode instantly updates the underlying truth for all modes.

## The Entropy System (Context-Aware Steering)

Entropy (System Integrity Debt) is measured deterministically, but surfaced contextually. We do not use "DND" modes; we use **Natural Breakpoints**.

### The Metrics
1.  **Coherence**: Plan vs. Reality mismatch.
2.  **Stagnation**: Stale RFCs.
3.  **Verification**: Missing walkthroughs.
4.  **Technical**: TODO count.

### The Interruption Model
*   **In Flow (During Phase)**:
    *   **Silence**. The HUD shows a subtle color change (Green -> Yellow -> Red), but no notifications.
    *   **Zen Mode**: An explicit toggle to suppress even the HUD color change for maximum focus.
    *   **Signal vs. Content**: The HUD provides the *Signal* (Something is wrong). The Inbox provides the *Content* (What is wrong). The user pulls the content when ready.
*   **Phase Transition (`exo phase finish`)**:
    *   **Soft Gate**. "Warning: Entropy is High (Critical Plan Mismatch). Review Inbox before starting next phase?"
    *   Allows override for perfunctory/urgent phases.
*   **Epoch Transition**:
    *   **Hard Gate**. "Epoch Complete. System Entropy Review Required."
    *   Forces a "Meta Review" step to clean up debt before committing to a new strategic direction.

### Contextual Evaporation (Inbox Hygiene)
*   **Principle**: Inbox items are bound to a context (Phase/Epoch/RFC).
*   **Behavior**: When the context changes (e.g., Phase Completed), relevant alerts are **Evaporated** (removed), not archived.
*   **Goal**: The Inbox reflects the *current* state, not a log of past anxieties.

## Refined Roles & Taxonomy

### 1. Maker (The Default)
*   **Focus**: Execution.
*   **UI**: Task List, Walkthrough, HUD.

### 2. Architect (The Parallel)
*   **Focus**: The Law (RFCs).
*   **UI**: RFC Editor, "Simulated Council" (Linter).
*   **Access**: Available via separate Agent Chat or "Architect View".

### 3. Planner (The Phase Type)
*   **Focus**: Sequencing & Strategy.
*   **UI**: Timeline, RFC Slotting.
*   **Trigger**: `exo phase start --type planning`.

### 4. Meta (The Phase Type)
*   **Focus**: Health & Integrity.
*   **UI**: Entropy Dashboard, Auto-Fix Tools.
*   **Trigger**: `exo phase start --type meta` (or prompted at Epoch boundaries).
