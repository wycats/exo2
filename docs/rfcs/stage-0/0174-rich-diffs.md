<!-- exo:174 ulid:01kg5kp2kkab1xzbxtyym741q9 -->

# RFC 174: Rich Diffs for Context Editors

- **Supersedes**: RFC 10096



# RFC 0174: Rich Diffs for Context Editors

## Summary

Implement a "Rich Diff" view for Exosuit's custom editors (Studio) that displays changes as a single, annotated document rather than two side-by-side windows.

This RFC also tracks a minimal, incremental path toward that goal:

- In the near term, when VS Code forces side-by-side diffs for Custom Text Editors, Studio should render *semantic diff markers* (added/removed/modified) instead of requiring users to visually compare two full renders.
- For TOML-backed context editors, we can compute semantic markers by diffing parsed TOML and matching arrays-of-tables by `id`.
- For RFCs, we should compute semantic markers over the RFC model (frontmatter + sections) rather than line-by-line markdown.

## Motivation

Currently, when a user views a diff of a context file (e.g., `plan.toml`) using the Exosuit Studio editor, VS Code opens two instances of the editor side-by-side. This is the default behavior for Custom Text Editors.

However, this is suboptimal for structured data:

1.  **Redundancy**: The user sees the full document twice, consuming screen space.
2.  **Cognitive Load**: The user must manually compare the two rendered views to find differences.
3.  **Missed Opportunity**: We have the structured object model (SOM); we should be able to show _semantic_ diffs (e.g., "Task status changed from `pending` to `completed`") rather than just visual differences.

The goal is to provide a "Unified Rich Diff" that renders the document once, with visual annotations (highlights, strikethroughs, added/removed markers) indicating what changed.

## Detailed Design

### User Experience (UX)

1.  **Trigger**: The user clicks a file in the Source Control view, or runs "Compare with..."
2.  **View**: Instead of the standard side-by-side diff editor, the user sees a single Exosuit Studio window.
3.  **Visuals**:
    - **Modified Fields**: Highlighted in yellow/blue. The old value might be shown in a tooltip or strikethrough text next to the new value.
    - **Added Items**: Highlighted in green.
    - **Deleted Items**: Highlighted in red (or shown in a "Deleted" section if they are gone from the main tree).
    - **Unchanged Items**: Rendered normally (or dimmed to focus attention on changes).

### Architecture

VS Code does not natively support "Single View Diffs" for Custom Editors in the standard SCM diff view. The standard diff view _always_ creates two editors.

To achieve this, we have two potential strategies:

#### Strategy A: The "Smart" Diff Editor (Side-by-Side Optimization)

We accept the side-by-side layout but optimize it.

- The "Original" (Left) editor detects it is the original.
- The "Modified" (Right) editor detects it is the modified.
- They communicate (via a shared service or state) to synchronize scrolling and highlighting.
- _Drawback_: Still wastes space.

**Status (implemented, minimal):**

- Scroll sync between panes.
- Per-pane semantic markers for TOML-backed documents.
- Marker aggregation at section headers, plus optional per-field badges when only part of a section changed.

#### Strategy B: Custom Diff Command (The "Unified" View)

We implement a custom command `exosuit.openRichDiff` that:

1.  Takes two URIs (left and right).
2.  Reads the content of both.
3.  Computes a semantic diff (using a library like `microdiff` or custom logic on the SOM).
4.  Opens a _single_ webview (Custom Readonly Editor or just a Webview Panel).
5.  Passes the `DiffSOM` (Structured Object Model with Diff annotations) to the frontend.
6.  The Svelte frontend renders the `DiffSOM`.

To integrate with SCM, we might need to contribute a `diff` menu item or intercept the open action (hard/impossible for standard SCM).

#### Strategy C: The "Overlay" Mode

We use the standard Custom Editor. When the editor loads, it checks if there is a "comparable" version available (e.g., from git).

- We add a "Toggle Diff" button in the editor toolbar.
- When active, the editor fetches the HEAD version of the file.
- It computes the diff internally and switches to "Diff Mode" rendering.
- _Benefit_: Seamless integration. User opens the file, clicks "Show Changes", and sees the diff in-place.

### Implementation Details

1.  **Diff Logic**:

    - Need a robust diffing algorithm for our SOM (Structured Object Model).
    - Input: `SOMRoot` (Old), `SOMRoot` (New).
    - Output: `DiffSOM` (a tree where nodes have status: `unchanged`, `added`, `removed`, `modified`).

        **Minimal semantic markers (current approach):**

        Instead of a full `DiffSOM`, compute a sparse map of markers keyed by SOM field path.

        - Output: `fieldMarkers: Record<string, "added" | "removed" | "modified">` where keys are a stable serialization of `SOMField.path`.
        - UI responsibility: show a small `+`/`−`/`~` badge next to changed fields.
        - Container responsibility: aggregate descendant field markers to show a header marker (`+` iff all descendants are added, `−` iff all descendants are removed, otherwise `~`).

        This keeps the first iteration minimal while still being semantic.

        **TOML semantic diff:**

        - Parse both sides as TOML and diff the parsed values.
        - Arrays-of-tables are matched by `id` when present, so reorder does not generate noise.
        - Diff output is materialized back into per-pane concrete field paths.

        **RFC semantic diff (proposed next step):**

        Do *not* diff RFC markdown by line.

        - Parse both sides using the RFC model (frontmatter + `##` sections).
        - Produce markers for:
            - Metadata fields (title/status/stage/authors/epoch).
            - Sections (added/removed/modified) keyed by section id/title.
        - In the RFC Studio view, render markers next to:
            - TOC entries.
            - Section headings.

        This is intentionally shallow; a deeper AST-level markdown diff is future work.

2.  **Frontend (Svelte)**:

    - Update components (`TextField`, `ListField`, etc.) to accept a `diffStatus` prop.
    - Implement visual styles for diff states.

3.  **VS Code Integration**:
    - Implement the "Toggle Diff" command or a specific "Open Rich Diff" command.

## Drawbacks

- **Complexity**: Implementing a semantic diff algorithm for a custom tree structure is non-trivial.
- **VS Code Limitations**: We cannot easily replace the native SCM diff view. Users might still see the side-by-side view by default when clicking in the Source Control panel.

## Alternatives

- **Text Diff**: Just fall back to the standard text diff editor for these files. (Current fallback if we don't register a custom editor, but we lose the rich rendering).
- **Side-by-Side**: Stick with the current behavior but try to sync scrolling.

## Unresolved Questions

- Can we intercept the SCM "Open Changes" click for custom editors? (Likely no).
- Is "Strategy C" (In-place toggle) the best UX?

- For RFCs, what is the stable section identity?
    - Today: derived from heading text.
    - Future: optionally support explicit section IDs to make diffs resilient to renames.

## Future Possibilities

- **Merge Conflict Resolution**: A rich UI for resolving merge conflicts in `plan.toml`.


