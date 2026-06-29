<!-- exo:24 ulid:01kg5m2y6mf3fm9zze0xtz7fdh -->

# RFC 24: Exosuit UI Architecture


# RFC 0024: Exosuit UI Architecture

- **Status**: Stage 4 (Stable)
- **Created**: 2025-05-20
- **Implemented**: `packages/exosuit-vscode`
- **Related**: RFC 0008 (Sidebar Navigation)

## Summary

The Exosuit UI Architecture defines how the agent and project state are presented to the user within VS Code. It expands upon the original "Sidebar" concept (RFC 0008) to include a multi-view Dashboard and a custom "Studio" editor for rich context files.

## Motivation

As the complexity of the Exosuit context grew (Plan, Design, Artifacts, Logs), a single tree view became insufficient.

1.  **Information Density**: We need specialized views for different aspects of the system (e.g., a dedicated view for the active Phase vs. the long-term Plan).
2.  **Rich Editing**: Editing raw state projections directly is functional but lacks the "Thinking Partner" experience. We need a custom editor that renders the semantic meaning of canonical state.

## Design

### The Sidebar (Activity Bar)

The `exosuit-sidebar` container hosts multiple views:

1.  **Dashboard**: High-level status and quick actions.
2.  **Project Plan**: The long-term roadmap (Epochs/Phases).
3.  **Phase Details**: The active workspace (Tasks/Progress).
4.  **Design Context**: Access to Axioms, Decisions, and RFCs.
5.  **Phase Artifacts**: Outputs of the current phase.
6.  **Activity Log**: Real-time debug and operation logs.

### The Exosuit Studio (Rich Context Editor)

We implement a rich editor surface (`exosuit.richContextEditor`) for managed context artifacts and projections in the context directory.

- **Split View**: It maintains the text editor model (for raw edits) but presents a rich HTML interface (via RTD) for interaction.
- **Live Updates**: Changes in the visual interface (e.g., clicking "Complete Task") write back through the canonical state layer.

## Implementation

- **View Providers**: Each sidebar view is backed by a `TreeDataProvider` in `packages/exosuit-vscode/src`.
- **Message Passing**: The Webviews communicate with the Extension Host via a typed message protocol to trigger commands (e.g., `exosuit.completeGoal`).
