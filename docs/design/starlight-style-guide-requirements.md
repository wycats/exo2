# Starlight Style Guide Requirements

Source: four Exosuit style-guide mockups shared during the `Scaffold Astro Starlight documentation site` phase.

## Purpose

The documentation site should translate Exosuit’s product identity into a Starlight-native surface: local-first, compact, inspectable, and developer-facing. The site should feel like a collaborative cockpit, not a marketing microsite or chatbot wrapper.

## Brand Premise

- Exosuit is augmentation, not automation.
- The product is a fitted control surface for developer judgment.
- The documentation should foreground shared context, steering, verification, and reach.
- Native-first presentation matters: the site can be branded, but it should still feel like technical documentation.

## Design Principles

1. **Augmenting, not autonomous** — Docs should frame Exosuit as strengthening developer judgment, not replacing it.
2. **Cockpit, not chatbot** — Navigation and page layouts should emphasize orientation, state, controls, and next steps.
3. **Dense, not cramped** — Pages should use compact information architecture with clear hierarchy and breathing room.
4. **Local and inspectable** — Content should explain where state lives, how it is verified, and what the user can inspect.
5. **Native, not over-branded** — Prefer Starlight conventions and host theme variables first; brand accents follow.
6. **Tactical, not militarized** — Use operational language such as phases, steering, rails, signals, progress, and verification; avoid combat imagery.

## Voice and Copy

The site voice should be calm, precise, and operational. Preferred copy pattern:

- Say what happened.
- Say what is next.
- Say who decides.

Example microcopy patterns:

- No active phase — start one to begin working.
- Outcome ready for review.
- User feedback waiting on this goal.
- Next: verify before closing.

Avoid:

- Magic AI sparkle language.
- Replace-the-developer rhetoric.
- Cyberpunk clichés.
- Robot mascot energy.
- Opaque automation claims.

## Visual System Requirements

### Color Roles

The theme should treat colors as semantic signals, not decoration.

| Role          | Usage                                                               |
| ------------- | ------------------------------------------------------------------- |
| Ink           | Primary text and high-emphasis content                              |
| Surface       | Page and component backgrounds                                      |
| Line          | Borders, dividers, rails, and subtle UI boundaries                  |
| Signal Blue   | Informational and neutral workflow signals, phase/progress emphasis |
| Signal Green  | Positive state and successful outcomes                              |
| Signal Yellow | Warnings and attention-needed states                                |
| Signal Red    | Errors, blocking states, and critical issues                        |

### Typography

- Use strong hierarchy for display, heading, body, caption, and mono text.
- Keep body text compact and readable.
- Use restrained monospace for technical emphasis, command snippets, IDs, and status tokens.
- Preserve editorial clarity over ornamental styling.

### Iconography

Icons should be simple, geometric, and compatible with codicons. Core semantic icon set:

- Phase
- Steering
- Perception
- Verification
- RFC
- Task

### Spacing and Rhythm

- Use generous outer margins for content breathing room.
- Use compact internal module spacing for information density.
- Use thin dividers and rails to organize without visual clutter.
- Prefer panel/card groupings that communicate workflow state at a glance.

### Motion

- Motion should be subtle and functional.
- Loading states should be subdued and indeterminate for short waits.
- Progress indicators should be linear, honest, and measured.
- Attention cues should be gentle signals, not surprise or celebration effects.

## Product Pattern Requirements

The Starlight site should support reusable documentation patterns inspired by the mockups:

### Phase Header

A compact hero or callout pattern that identifies the active unit of work and next action.

Required fields when applicable:

- Phase title
- Current status
- Epoch/context label
- Linked RFC/task references
- Next action

### RFC Rail

A horizontal or vertical progress rail for lifecycle documentation.

Stages represented in copy or UI examples:

- Draft
- Review
- Approved
- Implement
- Verify
- Closed

### Signal / Perception Event

A compact alert/callout pattern for detected drift, stale state, warnings, or notable workflow signals.

Should include:

- Signal label
- Human-readable summary
- Source
- Confidence or severity when meaningful
- Action affordance when applicable

### Status Bar Signals

Grouped status strips should show multiple state categories together without relying on color alone.

Examples:

- Steering active
- Inbox count
- Diagnostics/issues count

### Task Lists

Task lists should be glanceable and linguistically clear.

Fields:

- Task title
- Assignee/agent when relevant
- State
- Updated time or recency

States should be words first, color second.

## Starlight Structure Implications

The scaffold should begin with a content architecture that can grow into these sections:

1. **Start Here** — What Exosuit is, the brand premise, and the cockpit mental model.
2. **Concepts** — Phases, goals, tasks, RFCs, steering, perception, verification, and local-first state.
3. **Guides** — How to run the loop, start a phase, plan work, execute, review, and close.
4. **Reference** — CLI commands, VS Code extension surfaces, hooks, sidecar behavior, state locations.
5. **Design System** — Voice, visual semantics, product patterns, and UI examples.
6. **Architecture** — Validation-based reactivity, storage, sidecars, and generated projections.

## Theme Requirements for Scaffold

The first scaffold should include:

- A custom Starlight theme CSS file with Exosuit semantic tokens.
- Light/dark theme support using Starlight and Astro conventions.
- Compact content width and page rhythm tuned for technical documentation.
- Callout styles for phase, steering, perception, verification, warning, and error signals.
- Card/grid styles that can host compact product-pattern examples.
- Minimal custom JavaScript; prefer static Astro/Starlight primitives and CSS.

## Implementation Constraints

- Keep the site native to Astro Starlight rather than building a fully custom app shell.
- Do not store human-authored site requirements under `docs/agent-context/`.
- Reuse existing docs content from `docs/design/`, `docs/specs/`, `docs/research/`, and top-level docs.
- Generated SQL projections are not source content.
- Treat colors as semantic roles and ensure text labels carry status meaning without relying on color alone.
