<!-- exo:10057 ulid:01kmzxefe4esd8zna8zpzja3va -->


# RFC 10057: Studio UX Polish

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**: This later reconstruction duplicate revives the withdrawn Studio UX Polish proposal in RFC 0028. The retired Studio component surface is not part of the current extension implementation.

- **Superseded by**: RFC 0028


## Meta

- **Status**: Stage 0 (Draft)
- **Created**: 2025-12-09
- **Authors**: GitHub Copilot
- **Epoch**: 18 (Usability Slices)

## Summary

This RFC proposes specific visual refinements to the Studio UI, focusing on the Phase Header responsiveness and the alignment of task list items.

## Motivation

The current Studio implementation has a few rough edges:

1.  **Phase Header**: The text size is static, which causes layout issues or visual clutter in narrow panes (e.g., when the Studio is docked to the side).
2.  **Task List Alignment**: The checkbox in task items is vertically centered relative to the title. When the title wraps or when the visual weight of the item increases (e.g., "two lines" of content), this centering looks unprofessional. Standard UI patterns align checkboxes with the first line of text.

## Proposal

### 1. Responsive Phase Header

We will implement responsive typography for the `PhaseHeader` component.

- **Mechanism**: Use CSS Container Queries (if supported by the webview environment) or Media Queries.
- **Behavior**:
  - **Wide View (> 400px)**: Keep current font size (`0.85rem`).
  - **Narrow View (< 400px)**: Reduce font size to `0.75rem` or smaller.
  - **Very Narrow View**: Consider hiding the Phase ID or stacking the layout.

### 2. Task Item Alignment

We will adjust the CSS Grid alignment for the Task Card variant.

- **Current**: `align-self: center` for the checkbox (Grid Row 1).
- **Proposed**: `align-self: start` with a `margin-top` adjustment to align the checkbox visually with the cap height of the first line of the title text.

### 3. Component Standardization

Ensure these styles are encapsulated within the `PhaseHeader.svelte` and `StudioRenderer.svelte` components respectively, maintaining the "Shared Rendering" architecture.

## Design Details

### Phase Header CSS

```css
.phase-header {
  /* ... */
  container-type: inline-size;
}

@container (max-width: 400px) {
  .phase-header {
    font-size: 0.75rem;
  }
  .phase-status {
    font-size: 0.65rem;
  }
}
```

### Task Checkbox CSS

```css
.studio-container.group.variant-task-card
  .container-children
  > :global(:nth-child(1)) {
  /* ... */
  align-self: start;
  margin-top: 0.2rem; /* Visual adjustment */
}
```

## Drawbacks

- None. These are purely cosmetic improvements.

## Alternatives

- Keep as is (rejected by user feedback).
- Use JavaScript for responsiveness (overkill).
