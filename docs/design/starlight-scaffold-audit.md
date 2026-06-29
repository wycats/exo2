# Starlight Scaffold Audit

Source task: `create-an-astro-starlight-documentation-site-scaffold::audit-existing-docs-content-and-starlight-requirements`

## Outcome

The documentation site should be scaffolded as a new workspace package under `packages/`, using Astro Starlight as the documentation shell and existing repo docs as source content. The scaffold should stay Starlight-native, use Exosuit brand semantics through CSS tokens, and avoid treating generated `docs/agent-context/` SQL projections as source content.

The mockups should remain a first-class input for the scaffold. Do not reduce the visual direction to prose-only requirements. The scaffold should include an early implemented style-guide page that extracts reusable visual cues from the mockups into durable Astro/Starlight code: semantic tokens, compact panel rhythm, rails, status strips, signal colors, typography hierarchy, and product-pattern examples. This gives the project a concrete visual reference before the images leave working context.

## Existing Content Inventory

### Primary source content

Use these as first-pass site source material:

- `docs/vision.md` — product philosophy, collaborative cockpit framing, phased workflow, and North Star user journey.
- `docs/vision-exo-everywhere.md` — broader product vision for Exosuit beyond this repository.
- `docs/design/starlight-style-guide-requirements.md` — style-guide translation, visual semantics, content structure, and Starlight theme requirements.
- `docs/design/agent-context-ownership.md` — source-of-truth boundaries for operational state, generated projections, durable design notes, research, and specs.
- `docs/design/display-titles.md` — command display language, conversational handles, and user-facing microcopy rules.
- `docs/design/github-profile-sidecar-discovery.md` — sidecar discovery design details for reference/architecture pages.

### Specification content

Use these as Architecture and Reference source material:

- `docs/specs/architecture.md` — architecture overview.
- `docs/specs/system_requirements.md` — runtime and system constraints.
- `docs/specs/context-markdown.md` — context document format and semantics.
- `docs/specs/rsl-spec.md` — RSL reference material.
- `docs/specs/sidecar-status-pane-view-model.md` — concrete product surface model for status panes.
- `docs/specs/rtd/` — RTD syntax, streaming, model, roadmap, and RTML references.
- `docs/specs/algebras/` — reactive filesystem, SQLite, collections, resources, and reactivity algebra.
- `docs/specs/vcom/` — VCOM spec and explainer.

### Research and diagnostics content

Use these as supporting material, not top-level navigation defaults:

- `docs/research/sidecar-status-pane-sources.md`
- `docs/research/github-profile-sidecar-discovery-inventory.md`
- `docs/research/migration-aware-upgrade-guidance-inventory.md`
- `docs/research/e2e-fixture-modernization-audit.md`
- `docs/research/sqlite-migration-inventory.md`
- `docs/research/sqlite-reactive-vtable-sketch.md`
- `docs/research/git-friendly-database-comparison.md`
- `docs/bug-reports/`

### Template material

Use template docs as examples of bootstrap/adoption content, not as canonical website pages:

- `src/templates/docs/design/axioms.md`
- `src/templates/docs/design/context-persistence.md`
- `src/templates/docs/design/modes.md`

### Excluded source content

Do not use these as human-authored site pages:

- `docs/agent-context/*.sql`
- Generated repo-policy projections under `docs/agent-context/`
- Sidecar/shadow generated state projections

This follows the ownership rule in `docs/design/agent-context-ownership.md`: generated projections are infrastructure, not durable prose.

## Recommended Site Package Shape

Create a new package:

- `packages/exosuit-docs/`

Expected baseline files:

- `package.json`
- `astro.config.mjs`
- `tsconfig.json`
- `src/content.config.ts`
- `src/content/docs/`
- `src/styles/exosuit.css`
- `public/`

The root `pnpm-workspace.yaml` already includes `packages/*`, so a new package under `packages/exosuit-docs` will be picked up automatically.

## Dependency Requirements

Add Starlight dependencies to the new package only:

- `astro`
- `@astrojs/starlight`
- `sharp`

The root package should not become the Astro app. Keep the docs site isolated as a workspace package so root build/test scripts remain monorepo orchestration entry points.

## Initial Information Architecture

Visual fidelity is an early scaffold concern, not a later polish pass. The first scaffold should include a mockup-derived style guide page before broad content migration so that the site has a durable visual baseline.

### Start Here

Purpose: explain what Exosuit is and establish the cockpit mental model.

Initial pages:

- `start-here/index.mdx` — Exosuit in one page.
- `start-here/philosophy.mdx` — adapted from `docs/vision.md`.
- `start-here/north-star.mdx` — adapted from the North Star user journey.

### Concepts

Purpose: explain the workflow vocabulary and mental model.

Initial pages:

- `concepts/phases-goals-tasks.mdx`
- `concepts/rfcs.mdx`
- `concepts/steering-and-perception.mdx`
- `concepts/local-first-state.mdx`

### Guides

Purpose: describe concrete workflows.

Initial pages:

- `guides/run-the-loop.mdx`
- `guides/start-a-phase.mdx`
- `guides/plan-execute-review.mdx`
- `guides/verify-and-close.mdx`

### Reference

Purpose: provide operational lookup material.

Initial pages:

- `reference/cli.mdx`
- `reference/vscode-extension.mdx`
- `reference/hooks.mdx`
- `reference/sidecars.mdx`
- `reference/state-locations.mdx`

### Design System

Purpose: preserve the mockup-derived style guide and product-pattern requirements.

Initial pages:

- `design-system/index.mdx`
- `design-system/voice.mdx`
- `design-system/visual-semantics.mdx`
- `design-system/product-patterns.mdx`

### Architecture

Purpose: explain system design and source-of-truth boundaries.

Initial pages:

- `architecture/index.mdx`
- `architecture/validation-based-reactivity.mdx`
- `architecture/storage-and-projections.mdx`
- `architecture/sidecars.mdx`

## Theme Requirements

The first scaffold should include a custom CSS file that defines Exosuit semantic tokens and Starlight overrides.

Required token groups:

- Ink, surface, and line colors.
- Signal blue, green, yellow, and red.
- Compact spacing scale for technical docs.
- Mono/status token styling.
- Panel/card styles for workflow examples.
- Callout variants for phase, steering, perception, verification, warning, and error.

Implementation rule: use Starlight and Astro conventions first, then add branded accents. The site should look like a disciplined technical documentation surface, not a custom marketing app shell.

## Scaffold Acceptance Criteria

The scaffold is complete when:

1. `packages/exosuit-docs` exists as a workspace package.
2. Starlight builds successfully from the package.
3. The initial sidebar matches the information architecture above.
4. The homepage communicates Exosuit as augmentation, not automation.
5. The theme includes semantic signal tokens and light/dark support.
6. The scaffold includes at least one page per top-level section.
7. Generated `docs/agent-context/` projections are not copied as page source.
8. Root monorepo scripts remain intact.
9. The Design System section includes an implemented, mockup-derived style guide page with concrete visual examples for signal tokens, rails, panels, status strips, task lists, and microcopy.

## Implementation Task Breakdown

Recommended next tasks:

1. Scaffold `packages/exosuit-docs` with Astro Starlight and workspace package scripts.
2. Add Exosuit Starlight theme tokens and custom CSS.
3. Implement a mockup-derived style guide page in the Design System section while the images are still fresh in working context.
4. Create the initial navigation/content skeleton from the audited source map.
5. Adapt the first Start Here page from existing docs.
6. Validate package build and workspace integration.
