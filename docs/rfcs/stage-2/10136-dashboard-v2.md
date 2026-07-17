<!-- exo:10136 ulid:01kmzxbcytxqwb659ztpse8y2q -->


# RFC 10136: Dashboard V2

- **Superseded by**: RFC 00184


- Feature Name: `dashboard_v2`
- Start Date: 2025-12-07
- RFC PR: (leave empty)
- Exosuit Issue: (leave empty)

## Summary

A high-density, sidebar-resident dashboard for visualizing and navigating Exosuit project context. It serves as the "Head-Up Display" (HUD) for the active developer, providing immediate access to the Current Phase, Feedback loops, and RFCs.

## Motivation

The Exosuit workflow relies heavily on context (Plans, RFCs, Feedback). Previously, this information was either buried in files or displayed in a low-density, "web-page like" dashboard that felt out of place in the VS Code sidebar.

Developers need a way to:

1.  **Orient**: "What phase am I in?"
2.  **React**: "Is there new feedback?"
3.  **Reference**: "What is the status of that RFC?"

This must happen without context switching or scrolling through large documents. The UI must be "Sidebar-First" — optimized for narrow widths, high information density, and native visual integration.

## Guide-level explanation

The Dashboard V2 is a panel in the "Exosuit: Run" sidebar view. It is always available and updates as the project state changes.

### Sections

1.  **Current Phase**: A "Hero" card showing the active phase title and ID. This anchors the developer in the current task.
2.  **Feedback**: A list of active feedback threads.
    - **Status Indicators**: Color-coded dots (Blue=Open, Green=Resolved).
    - **Context**: Shows the target file/ID and the latest message preview.
    - **Action**: Clicking a thread opens the relevant file.
3.  **RFCs**: A comprehensive list of Request for Comments documents.
    - **Grouping**: RFCs are grouped by Stage (Stage 4 down to Stage 0) to separate "Laws" from "Ideas".
    - **Badges**: Stage counts are displayed in section headers.
    - **Navigation**: Clicking an RFC opens the markdown file.

### Interaction Model

The dashboard is read-only but interactive. It acts as a navigation hub.

- **Click**: Opens the corresponding resource in the editor.
- **Hover**: Reveals native VS Code hover states.

## Reference-level explanation

### Architecture

The dashboard is implemented as a **WebviewView** (not a TreeView) to allow for custom layout and styling while maintaining the sidebar form factor.

- **Host**: `DashboardProvider` (VS Code Extension)
  - Implements `vscode.WebviewViewProvider`.
  - Instantiates `@exosuit/core` services (`ContextService`, `FeedbackService`).
  - Handles message passing (`postMessage`) to/from the webview.
- **Client**: Svelte 5 Application
  - **Build**: Vite-bundled, targeting ES modules.
  - **State**: `DashboardService` (Svelte 5 `.svelte.ts` module) manages reactive state using Runes (`$state`, `$derived`).
  - **UI**: `App.svelte` renders the view using "Sidebar-First" CSS.

### Data Flow

1.  **Hydration**: On `resolveWebviewView`, the provider calls `refresh()`.
2.  **Fetch**: Services read TOML/Markdown files from the workspace.
3.  **Push**: Data is sent via `webview.postMessage({ type: "UPDATE_...", payload: ... })`.
4.  **Render**: Svelte updates the DOM efficiently.
5.  **Action**: User clicks an item -> `vscode.postMessage({ type: "OPEN_FILE", payload: { path: ... } })` -> Extension opens editor.

### Styling Specification

To ensure "Native Camouflage", the dashboard **must** use VS Code CSS variables exclusively.

- **Backgrounds**: `--vscode-sideBar-background`, `--vscode-list-hoverBackground`.
- **Foregrounds**: `--vscode-foreground`, `--vscode-descriptionForeground`.
- **Borders**: `--vscode-panel-border`, `--vscode-sideBarSectionHeader-border`.
- **Accents**: `--vscode-textLink-foreground`, `--vscode-charts-*`.

Hardcoded colors (hex/rgb) are strictly forbidden to support themes (Light, Dark, High Contrast).

## Drawbacks

- **Performance**: Webviews are heavier than native TreeViews. However, for a single dashboard, the overhead is negligible compared to the flexibility gained.
- **Complexity**: Requires a build step (Vite) and message passing, whereas TreeViews are purely API-driven.

## Rationale and alternatives

- **Alternative: TreeView**:
  - _Pros_: Native performance, zero build step.
  - _Cons_: Rigid layout (only tree nodes), limited styling (cannot do "Hero" cards or complex row layouts with badges/timestamps).
  - _Verdict_: Rejected. The density and metadata requirements (status dots, message previews) require HTML/CSS control.

## Unresolved questions

- **Real-time Updates**: Currently, the dashboard refreshes on visibility changes or manual triggers. We should implement file watchers in `ContextService` to push updates immediately when `plan.toml` or feedback files change.
