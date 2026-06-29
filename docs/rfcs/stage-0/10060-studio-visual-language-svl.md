<!-- exo:10060 ulid:01kmzxefdwmtk59n18pqh2fs49 -->


# RFC 10060: Studio Visual Language (SVL)

## Meta

- **Status**: Stage 0 (Draft)
- **Created**: 2025-12-09
- **Authors**: GitHub Copilot
- **Epoch**: 18 (Usability Slices)

## Summary

This RFC proposes a "Visual Spec Language" (SVL) for Exosuit Studio pages. It defines a set of high-level building blocks and layout primitives to ensure consistency, reduce code duplication, and pave the way for a future Rich Text Object Model (RTOM) styling system.

## Motivation

Currently, Studio pages (`WalkthroughEditor`, `PlanEditor`, `DecisionEditor`) are implemented as ad-hoc Svelte components. While they share some underlying logic, their visual presentation and layout structure are inconsistent:

1.  **Layout Fragmentation**: Each page re-implements the "Sidebar" logic for feedback, the scrolling behavior, and the header layout.
2.  **Visual Inconsistency**: "Cards" in the Plan Editor look different from "Cards" in the Walkthrough Editor. Status badges have varying styles.
3.  **Maintenance Burden**: Adding a global feature (like a "Help" mode or "Breadcrumbs") requires updating every single editor component.

We need a unified "Visual Language" that abstracts these concerns into a standard library of Studio components.

## Proposal

We will introduce a set of "Studio Primitives" that act as the vocabulary for building pages.

### 1. The Page Shell (`<StudioPage>`)

The top-level container for any Studio view. It handles:

- **Global Layout**: The 3-pane structure (Navigation, Content, Context/Sidebar).
- **State Management**: Managing the `selectedContextId` for feedback and other cross-cutting concerns.
- **Scrolling**: Ensuring the main content area scrolls while the header and sidebar remain fixed (or behave predictably).

```svelte
<StudioPage>
  <div slot="header">...</div>
  <div slot="toolbar">...</div>
  <div slot="content">...</div>
  <div slot="sidebar">...</div>
</StudioPage>
```

### 2. The Header Primitive (`<PageHeader>`)

A standardized header component that enforces the "Exosuit Look".

- **Title**: Large, primary identifier.
- **ID**: Subtle, technical identifier (e.g., `RFC 0042`, `phase-55`).
- **Status**: A standardized `<StatusBadge>` slot.
- **Metadata**: A `<PropertyGrid>` for secondary info (Authors, Date, Epoch).

### 3. Content Primitives

#### `<Section>`

A semantic container for a major division of the page.

- **Props**: `title`, `id` (for deep linking/feedback).
- **Behavior**: Collapsible (optional), distinct visual separation.

#### `<Card>`

A container for discrete items in a list (e.g., a Task, a Change, a Phase).

- **Props**: `selected` (boolean), `status` (enum).
- **Slots**: `header`, `details`, `actions`.
- **Behavior**: Hover effects, selection state, integration with Feedback system.

#### `<PropertyGrid>`

A layout for key-value pairs, ensuring alignment and consistent typography.

### 4. Visual Primitives

- **`<StatusBadge>`**: A single source of truth for status colors (Active=Blue, Completed=Green, Draft=Gray, etc.).
- **`<ActionToolbar>`**: A container for buttons and toggles, ensuring consistent spacing and icon sizing.
- **`<RtdBlock>`**: The fundamental unit of text, wrapping the `RTDRenderer` with standard typography margins.

## Future: RTOM Integration

This "Visual Language" is the precursor to a full RTOM (Rich Text Object Model) styling system. Eventually, these Svelte components will be replaced or driven by a declarative styling language defined in `exosuit.toml` or similar.

For now, implementing them as strict Svelte components allows us to "discover" the necessary scope of that future language.

## Implementation Plan (Strawman)

1.  **Phase A**: Create `StudioPage`, `PageHeader`, and `StatusBadge` components.
2.  **Phase B**: Refactor `WalkthroughEditor` to use these new primitives.
3.  **Phase C**: Refactor `PlanEditor` and `DecisionEditor`.
4.  **Phase D**: Extract `FeedbackSidebar` logic into `StudioPage`.

## Example Usage

```svelte
<StudioPage bind:selectedContextId>
  <PageHeader title="Studio Visual Language" id="RFC 0042">
    <StatusBadge slot="status" status="draft" />
    <PropertyGrid slot="meta">
      <Property label="Author" value="GitHub Copilot" />
      <Property label="Epoch" value="18" />
    </PropertyGrid>
  </PageHeader>

  <Section title="Proposal">
    <RtdBlock content={proposalMarkdown} />
  </Section>

  <Section title="Tasks">
    {#each tasks as task}
      <Card status={task.status} id={task.id}>
        <div slot="header">{task.title}</div>
        <div slot="actions">
          <ActionToolbar>
             <IconButton icon="pass" onclick={...} />
          </ActionToolbar>
        </div>
      </Card>
    {/each}
  </Section>
</StudioPage>
```
