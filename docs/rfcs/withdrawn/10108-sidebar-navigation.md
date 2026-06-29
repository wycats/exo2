<!-- exo:10108 ulid:01kmzxey1bxqnmd9ev4yhjeq42 -->


# RFC 10108: Sidebar Navigation

## Core Philosophy

- **Zoom Levels**: Different views for different granularities (Birds-eye vs. Weeds).
- **Context Awareness**: The sidebar should reflect the user's focus, not just the static file structure.
- **History as First-Class**: Past phases should be reviewable in the same UI as the current phase.

## The Three Panes

### 1. Project Plan (The Map)

- **Scope**: Entire project lifecycle (Past, Present, Future).
- **Granularity**: High-level. Shows Epochs, Phases, and **Top-Level Tasks**.
  - _Rationale_: Showing the first level of tasks provides a quick summary of what the phase was about without overwhelming detail.
- **Interaction**: Acts as a navigation controller. Clicking an item here updates the "Epoch Details" and "Phase Details" panes.
- **Persistence**: Always available, global navigation.

### 2. Epoch Details (The Chapter)

- **Scope**: The selected Epoch (Current or Historical).
- **Granularity**: Mid-level. Shows the Phases within the Epoch and their high-level goals.
- **Interaction**: Clicking a Phase here behaves exactly like clicking it in the Project Plan (updates the Phase Details pane).
- **Purpose**: Contextual grounding. "Where are we in this chapter?"

### 3. Phase Details (The Page)

- **Scope**: The selected Phase (Current or Historical).
- **Granularity**: Low-level. Shows detailed Task Lists, Walkthrough status, and specific artifacts.
- **Dynamic Content**:
  - **Current Phase**: Shows live `docs/agent-context/current/task-list.md`. Editable.
  - **Historical Phase**: Shows `docs/agent-context/archive/.../task-list.md`. **Read-Only**.
- **Metadata**: Displays Git SHA for historical phases.
- **Visuals**: Distinct UI/Icon to indicate if the view is "Live" (Current) or "Archived" (Read-Only).

## Interaction Model

### "Focus" Action

Clicking a Phase in the "Project Plan" OR "Epoch Details":

1.  **Populates Epoch Pane**: Loads the parent Epoch of the selected Phase.
2.  **Populates Phase Pane**:
    - **If Current**: Loads live data.
    - **If Past**: Locates archive and loads read-only data.
    - **If Future**: Shows planned state.

### Feature Parity

- Features like "View Walkthrough" or "Open Diff" should work consistently for both Current and Historical phases.
- The UI should abstract away the source (Live vs Archive) so the user experience is seamless.

### "Time Travel" & Read-Only State

- Historical phases are strictly Read-Only for now.
- Future consideration: Allow commenting on historical artifacts.
- **Restoration**: A "Back to Current" button in the Project Plan to quickly reset all panes to the actual current state.

## Data Source Implications

1.  **Project Plan**: Parsed from `plan-outline.md` (including top-level tasks).
2.  **Epoch Details**: Parsed from `plan-outline.md` (filtered to selected Epoch).
3.  **Phase Details**:
    - **Live**: Parsed from `current/task-list.md`.
    - **History**: Parsed from `archive/.../task-list.md`.

## Walkthrough vs. Task List

- **Task List**: The granular "Todo" items. Shown in the Phase View.
- **Walkthrough**: The narrative description of work.
  - **UI**: Should be accessible from the Phase View (e.g., a "View Walkthrough" button or a separate tab/section).
  - **History**: Historical walkthroughs are loaded from the archive alongside the task list.
