<!-- exo:29 ulid:01kg5m2xkk238cjd19td9r2p3h -->

# RFC 29: Studio RFC View


# RFC 0029: Studio RFC View

## Meta

- **Status**: Stage 0 (Draft)
- **Created**: 2025-12-09
- **Authors**: GitHub Copilot
- **Epoch**: 18 (Usability Slices)

## Summary

This RFC proposes a dedicated view within the Studio for visualizing, reading, and managing Request for Comments (RFC) documents.

## Motivation

RFCs are the "Laws" of the Exosuit project. Currently, they are just Markdown files. To elevate their status and make them easier to work with, we need a specialized view that:

1.  Highlights their metadata (Status, Stage, Epoch).
2.  Visualizes their lifecycle state.
3.  Provides easy access to transition actions (Promote, Withdraw).
4.  Integrates with the feedback system.

## Proposal

### 1. The RFC Model

We will define a `RfcModel` in the Studio that wraps the parsed RFC data (frontmatter + content). We will explicitly parse standard sections to enable rich rendering.

```typescript
interface RfcSection {
  type:
    | "summary"
    | "motivation"
    | "proposal"
    | "drawbacks"
    | "alternatives"
    | "generic";
  title: string;
  content: string; // Markdown content of the section
  level: number;
}

interface RfcModel {
  id: string;
  title: string;
  status:
    | "stage-0"
    | "stage-1"
    | "stage-2"
    | "stage-3"
    | "stage-4"
    | "withdrawn";
  meta: {
    created: string;
    authors: string[];
    epoch?: string;
  };
  sections: RfcSection[];
  rawContent: string;
}
```

### 2. The RFC View Component

A new `RfcView.svelte` component will be the top-level container.

#### Layout

- **Header**:
  - **Compact Design**: Title and Status Badge on one line. Metadata (Authors, Epoch) in a subtle sub-row.
  - **Lifecycle Bar**: A slim, non-intrusive progress indicator.
- **Navigation**:
  - **Sticky TOC**: A bar that sticks to the top of the content area, showing the active section and allowing quick jumps to other sections. This ensures navigation is accessible even on narrow screens.
- **Content Area**:
  - **Clean Body**: The view will strip out redundant "Meta" sections and duplicate Title headers, displaying only the core content sections.
  - **Summary**: Rendered as a "Lead" paragraph / Hero section.
  - **Motivation**: Rendered with a distinct "Problem Statement" visual style.
  - **Proposal**: The main body.
  - **Drawbacks**: Rendered with a "Cautionary" visual theme.
  - **Generic Sections**: Standard markdown rendering.
- **Code Rendering**:
  - **Engine**: Use `shiki` for high-fidelity syntax highlighting.
  - **Native Theme Integration**: Instead of shipping specific themes, map `shiki` tokens to VS Code's ambient CSS variables (e.g., `--vscode-symbolIcon-constantForeground`). This ensures code blocks look "native" and respect the user's active color theme automatically.

### 3. Integration

- **Opening**: The `RichEditorProvider` will detect `.md` files in `docs/rfcs/` and open them using the `RfcView` instead of the generic Markdown editor.
- **Navigation**: Links to RFCs (e.g., `RFC 0009`) in other documents should open this view.

## Creative Enhancements & Workflow

Using the Studio, we can transform RFCs from static text into dynamic dashboards for decision-making.

1.  **Task Extraction**: If an RFC contains a task list (e.g., in an "Implementation" section), the Studio should extract these into an interactive "Progress" widget in the header.
2.  **"Living Status"**:
    - **Stage 3 (Candidate)**: Link to the active `plan.toml` tasks that implement this RFC.
    - **Stage 4 (Stable)**: Link to the `docs/manual/` entries that codified this RFC.
3.  **Impact Analysis**: If the RFC links to specific Axioms or other RFCs, show these relationships in a "Context" sidebar.
4.  **Interactive Lifecycle**: "Promote" buttons that actually run the `exo` command to move the file and update the status.

## Structure Implications

To support this, we should standardize the RFC structure:

1.  **Strict Headers**: Enforce `## Summary`, `## Motivation`, etc., via linter or template.
2.  **Frontmatter First**: Move all metadata (Status, Authors, Epoch) to TOML frontmatter, deprecating the `## Meta` list in the body.
