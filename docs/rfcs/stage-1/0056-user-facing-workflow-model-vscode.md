<!-- exo:56 ulid:01kmzxey0f8qfee98vyzbr6vty -->

# RFC 56: User-Facing Workflow Model for Exosuit (VS Code)


# RFC 0056: User-Facing Workflow Model for Exosuit (VS Code)

## Summary

This RFC captures the agreed program for a systematic, VS Code–first review of Exosuit’s user-facing workflow.

The work is intentionally scoped to:

- The VS Code extension UI surface (commands, views, menus, custom editors, notebooks, chat participants, and related entrypoints)
- Onboarding touchpoints surfaced via VS Code and the CLI (`exo init`, `exo update`) insofar as they affect the VS Code journey

The goal is to make the “solo journey” coherent: users should encounter a predictable progression of capabilities, and the UI must not present partial or placeholder features as if they are ready.

## Motivation

The VS Code extension presents many entrypoints. Without an explicit user-facing model, it is easy to:

- expose “debug” or “internal” affordances as core workflow,
- ship partial features that look complete,
- and accumulate UI surfaces that don’t correspond to an intentional journey.

This RFC defines a program of work that produces:

1) An inventory of the current surface
2) A solo-first journey model
3) A classification/mapping of surface items onto that journey
4) A rationalization pass that consolidates, hides, or removes surfaces with guardrails

## Artifacts

### Inventory (ground truth)

The current, maintained inventory of the VS Code UI surface lives in:

- [docs/manual/meta/vscode-ui-surface-inventory.md](docs/manual/meta/vscode-ui-surface-inventory.md)

This inventory is sourced from the extension manifest and is intended to be updated as the UI surface changes.

## Program (Phases)

This RFC corresponds to the program work tracked in the plan:

- Phase 66: VS Code Surface Inventory
- Phase 67: Solo Journey Model
- Phase 68: Surface Rationalization

Each phase produces a concrete outcome that should be reviewable in-tree.

### Phase 66: Surface inventory

Produce a complete enumeration of the user-facing surface:

- Commands, views, menus, chat participants
- Custom editors and notebooks
- Activation events and key entrypoints

Output: the inventory document (see above).

### Phase 67: Solo journey model

Define the solo user journey stages and goals, then map every surface item to a stage.

Classification guidance for each surface item:

- **full**: ready and coherent
- **partial**: exists but incomplete; must be guarded
- **placeholder**: present but not meaningfully functional
- **deprecate-candidate**: should likely be hidden/removed

### Phase 68: Surface rationalization

For each classified surface item, decide:

- consolidate (merge overlapping affordances)
- hide (remove from primary UI; keep only for internal/debug use)
- remove (delete surface)

Guardrail:

- Do not present partial/placeholder features as ready in the primary user-facing UI.

## Relationship to workflow model RFCs

This RFC is about the VS Code *presentation* of Exosuit’s workflow and the user journey over that surface.

The canonical workflow model and the “canonical vs projection” boundary are defined elsewhere (for example the phase state machine / projections RFC). This RFC does not redefine the canonical workflow state; it defines how VS Code surfaces that workflow intentionally.


