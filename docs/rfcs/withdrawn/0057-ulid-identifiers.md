<!-- exo:57 ulid:01kmzxey1m7wafn2x72v90b2cy -->

# RFC 57: ULID-like Identifiers, Ordering Projections, and Human Slugs

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**: Later partial duplicate of RFC 0130; RFC 0130 is the implemented Stage 3 authority.

- **Superseded by**: RFC 0130



# RFC 0057: ULID-like Identifiers, Ordering Projections, and Human Slugs

## Summary

This RFC proposes a consistent model for **identity**, **ordering**, and **human-readable names** across Exosuit artifacts:

- **Stable identity** is an immutable, globally unique identifier (ULID-like).
- **Ordering** is a projection of storage shape (e.g. array order), not identity.
- **Human slugs** exist alongside stable IDs and have low-friction uniqueness rules.

The goal is to eliminate drift between “what we call something”, “where it appears”, and “what it *is*”, while keeping UX friendly.

## Motivation

Exosuit increasingly treats documents and derived views as projections. Today, several artifacts implicitly treat numeric order as identity (e.g. “Phase 44”), which creates problems:

- **Renumbering becomes a breaking change**: identity shifts when ordering changes.
- **UI/CLI ambiguity**: “44” might refer to order, filename stem, or internal identity.
- **Merges are fragile**: two people can independently create “the next number”.
- **Human intent is lost**: the thing humans remember is often a short name, not a number.

We want:

- Stable references that never change.
- A clear separation between identity and ordering.
- Human-friendly handles that are easy to create and resolve.

## Detailed Design

### Terminology

- **ID**: Stable identifier (ULID-like). Immutable. Unique.
- **Slug**: Human-friendly name (e.g. `polish-rfc-stage-indicator`). Mutable (within rules). Unique within a scope.
- **Order**: A projection determined by the container/document representation (e.g. array order). Not a stable identity.

### 1) Stable IDs (ULID-like)

Every artifact that participates in linking/referencing SHOULD have a stable ID.

Properties:

- **Uniqueness**: no collisions within a workspace.
- **Immutability**: IDs never change once assigned.
- **Opaque**: IDs are not user-facing by default.

Why ULID-like:

- Lexicographically sortable by time-of-creation is useful for debugging and “recent first”.
- Unlike incrementing integers, IDs do not require global coordination.

Non-goals:

- ULID ordering is not a substitute for explicit ordering. It is at best a default view.

### 2) Ordering is a projection

Ordering MUST be treated as a property of the *container*, not the *contained item*.

Examples:

- A task list’s order is the order of the array entries in `task-list.toml`.
- A list of phases shown in the UI is a projection (filter + sort) over a set of phase records.

Implications:

- Reordering does not change identity.
- “Phase N” can exist as a UI label, but it must be derived from the current projection.
- Any stable links MUST resolve via IDs (and optionally slugs).

### 3) Human slugs (low-friction uniqueness)

Humans need something better than ULIDs.

We introduce optional **slugs** for artifacts:

- Slugs SHOULD be lowercase kebab-case.
- Slugs SHOULD be unique within a well-defined scope (e.g. within an Epoch, within `docs/rfcs/`, within a task list).
- Slugs MAY change, but changes should be visible (e.g. via steering or a rename command).

Uniqueness rules (low-friction):

- On creation, if the desired slug is free: use it.
- If it’s taken: automatically suffix with `-2`, `-3`, … (or another deterministic suffix strategy).

Resolution rules:

- User inputs may refer to artifacts by **ID** or **slug**.
- If an input is ambiguous (same slug in multiple scopes), the system MUST require disambiguation and provide steering.

### 4) Mapping existing “numbered identity”

Some existing artifacts currently use numeric IDs (e.g. RFC number, Phase number).

This RFC does not require immediate migration, but it establishes direction:

- Numeric labels MAY remain as a **projection** (display order, sequence number).
- The underlying identity SHOULD move to a stable ID.
- Tools SHOULD provide steering toward stable references when ambiguity appears.

Practical rule:

- If a thing is referenced across files, across time, or via commands, it should have a stable ID.

### 5) Interaction with RFC numbering

RFC numbers (e.g. `0057`) are already stable enough for many workflows, but:

- They are globally coordinated and can be expensive to manage.
- They encourage “number-as-identity” thinking.

This RFC does not propose removing RFC numbers, but it does propose using them *as a handle*, not as “the model”.

## Drawbacks

- Adds conceptual overhead: ID vs slug vs order.
- Requires careful steering to avoid confusing UX.
- Implies migrations for legacy numeric-only artifacts.

## Alternatives

- **Use integers everywhere**: simple, but coordination and renumbering problems persist.
- **Use UUIDs only**: stable but unfriendly; encourages copy/paste of opaque values.
- **Use slugs only**: friendly, but renames and collisions make long-lived references fragile.

## Unresolved Questions

- Which artifacts must have stable IDs first (tasks, phases, walkthrough entries, RFCs, ideas)?
- What are the exact scopes for slug uniqueness (workspace-wide vs per-file vs per-artifact-type)?
- What is the canonical UX for disambiguation (prompting vs steering suggestions)?

## Future Possibilities

- A unified resolver: `exo resolve <id-or-slug>`.
- A rename command that preserves back-references.
- A consistent “display label” system: e.g. `Phase 44: polish-stage-indicator` where `44` is projection and the slug is human memory.
