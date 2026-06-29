<!-- exo:86 ulid:01kg5kp2f70ke18kf0wpjabqrt -->

# RFC 86: Studio UI Polish and Visual Language


# RFC 0086: Studio UI Polish and Visual Language

## Summary

This RFC proposes a comprehensive approach to Studio UI refinement, combining specific visual fixes with a unified "Visual Spec Language" (SVL) for consistency across all Studio pages. It addresses immediate UX issues (responsive headers, checkbox alignment) while establishing primitives for long-term maintainability.

## Consolidated From

This RFC merges:

- **RFC 0009: Studio UX Polish** - Specific fixes for Phase Header and task alignment
- **RFC 0017: Studio Visual Language** - Comprehensive component library and layout primitives

## Motivation

The current Studio implementation has both specific rough edges and systemic consistency issues:

### Immediate Issues

1. **Phase Header**: Static text size causes layout issues in narrow panes (e.g., when Studio is docked to the side).
2. **Task List Alignment**: Checkboxes are vertically centered relative to titles. When titles wrap, this looks unprofessional. Standard UI patterns align checkboxes with the first line of text.

### Systemic Issues

1. **Layout Fragmentation**: Each page (`WalkthroughEditor`, `PlanEditor`, `DecisionEditor`) re-implements sidebar logic, scrolling behavior, and header layout.
2. **Visual Inconsistency**: "Cards" in the Plan Editor look different from "Cards" in the Walkthrough Editor. Status badges vary in style.
3. **Maintenance Burden**: Adding global features (like "Help" mode or breadcrumbs) requires updating every editor component.

We need both **tactical fixes** and a **strategic framework** for Studio UI.

## Detailed Design

### Part 1: Immediate Visual Fixes

#### 1.1 Responsive Phase Header

Implement responsive typography for the `PhaseHeader` component.

**CSS Implementation:**

```css
.phase-header {
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

@container (max-width: 280px) {
  .phase-id {
    display: none; /* Hide ID in very narrow views */
  }
}
```

**Behavior**:

- **Wide View (> 400px)**: Keep current font size (`0.85rem`).
- **Narrow View (280-400px)**: Reduce font size to `0.75rem`.
- **Very Narrow View (< 280px)**: Hide Phase ID or stack layout.

#### 1.2 Task Item Checkbox Alignment

Adjust CSS Grid alignment for the Task Card variant.

**Current**: `align-self: center` for the checkbox (Grid Row 1).
**Proposed**: `align-self: start` with margin adjustment.

**CSS Implementation:**

```css
.studio-container.group.variant-task-card
  .container-children
  > :global(:nth-child(1)) {
  align-self: start;
  margin-top: 0.2rem; /* Align with cap height of text */
}
```

### Part 2: Studio Visual Language (Component Library)

#### 2.1 The Page Shell (`<StudioPage>`)

The top-level container for any Studio view.

**Responsibilities**:

- **Global Layout**: 3-pane structure (Navigation, Content, Context/Sidebar).
- **State Management**: Managing `selectedContextId` for feedback and cross-cutting concerns.
- **Scrolling**: Main content area scrolls while header and sidebar remain fixed.

**API:**

```svelte
<StudioPage bind:selectedContextId>
  <div slot="header">...</div>
  <div slot="toolbar">...</div>
  <div slot="content">...</div>
  <div slot="sidebar">...</div>
</StudioPage>
```

#### 2.2 The Header Primitive (`<PageHeader>`)

A standardized header component enforcing the "Exosuit Look".

**Slots**:

- **Title**: Large, primary identifier.
- **ID**: Subtle, technical identifier (e.g., `RFC 0017`, `phase-55`).
- **Status**: A standardized `<StatusBadge>` slot.
- **Metadata**: A `<PropertyGrid>` for secondary info (Authors, Date, Epoch).

#### 2.3 Content Primitives

##### `<Section>`

A semantic container for major page divisions.

- **Props**: `title`, `id` (for deep linking/feedback).
- **Behavior**: Collapsible (optional), distinct visual separation.

##### `<Card>`

A container for discrete items in a list (e.g., Task, Change, Phase).

- **Props**: `selected` (boolean), `status` (enum).
- **Slots**: `header`, `details`, `actions`.
- **Behavior**: Hover effects, selection state, feedback integration.

##### `<PropertyGrid>`

A layout for key-value pairs, ensuring alignment and consistent typography.

#### 2.4 Visual Primitives

- **`<StatusBadge>`**: Single source of truth for status colors (Active=Blue, Completed=Green, Draft=Gray).
- **`<ActionToolbar>`**: Container for buttons/toggles with consistent spacing and icon sizing.
- **`<RtdBlock>`**: Fundamental text unit wrapping `RTDRenderer` with standard typography margins.

### Part 3: Migration Strategy

#### Phase A: Create Core Primitives

1. Implement `StudioPage`, `PageHeader`, and `StatusBadge` components.
2. Apply immediate fixes (responsive header, checkbox alignment) to existing components.

#### Phase B: Refactor WalkthroughEditor

1. Migrate `WalkthroughEditor` to use new primitives.
2. Validate that all functionality is preserved.
3. Document any new patterns discovered.

#### Phase C: Refactor PlanEditor and DecisionEditor

1. Apply lessons from Phase B to remaining editors.
2. Extract common patterns into additional primitives as needed.

#### Phase D: Extract Sidebar Logic

1. Move `FeedbackSidebar` logic into `StudioPage`.
2. Ensure sidebar behavior is consistent across all pages.

## Example Usage

**Complete Studio Page:**

```svelte
<StudioPage bind:selectedContextId>
  <PageHeader title="Studio Visual Language" id="RFC 0017">
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

## Future: RTOM Integration

This Visual Language is a precursor to a full RTOM (Rich Text Object Model) styling system. Eventually, these Svelte components will be driven by declarative styling language defined in `exosuit.toml` or similar.

Implementing them as strict Svelte components first allows us to "discover" the necessary scope of that future language through actual usage.

## Drawbacks

- **Refactoring Cost**: Migrating existing editors requires significant effort.
- **Learning Curve**: Contributors must learn the new component vocabulary.
- **Premature Abstraction**: Risk of creating primitives that don't fit future needs.

## Alternatives

- **Keep as is**: Continue with fragmented, per-editor implementations. (Rejected: maintenance burden too high).
- **Full RTOM First**: Build the declarative styling system immediately. (Rejected: we don't yet know what primitives we need).
- **CSS Framework**: Use a third-party framework like Tailwind. (Rejected: doesn't address component fragmentation).

## Success Metrics

- **Consistency**: All Studio pages use the same components for common elements.
- **Maintainability**: Adding a global feature requires changing only 1-2 files.
- **Performance**: No degradation in rendering speed or responsiveness.
- **User Feedback**: Improved visual polish as reported in user testing.

