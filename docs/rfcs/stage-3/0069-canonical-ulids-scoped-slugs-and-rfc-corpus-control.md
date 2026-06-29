<!-- exo:69 ulid:01kg5kp2eczp52qt7c6sw4h0fa -->

# RFC 69: Canonical ULIDs, Scoped Slugs, and RFC Corpus Control


# RFC 0069: Canonical ULIDs, Scoped Slugs, and RFC Corpus Control

## Summary

Adopt **ULIDs as the canonical stored identity** for Exosuit artifacts, while keeping **optional human slugs** for UX.

This draft builds on the direction of RFC 0057 while tightening the rules around scoped uniqueness, collision handling (retry rather than auto-suffix), and corpus control.

Companion RFC: RFC 0068 (Position Protocol for Ordered Lists).

Key properties:

- **Canonical storage** uses stable `id = "<ulid>"` fields in TOML.
- **Slugs are optional**, scoped, and primarily for human input/recall.
- **CLI accepts either** canonical IDs or slugs, but MUST always echo the canonical ID in both human and machine output.
- **Slug collisions** within a scope are handled by prompting the agent/LLM to try again (no auto-suffix by default).

This RFC also proposes tightening RFC workflow:

- Stage 0 RFCs are treated as _attached proposals_ (tied to a phase/idea) rather than an unbounded global backlog.
- RFC “numbers” become a Stage 1+ governance handle; stable identity is still ULID.

## Motivation

We currently mix:

- Order-derived or numeric identifiers (e.g. `phase-74`).
- Semantic string identifiers (e.g. `migrate-tests`).
- RFC numbers that act like IDs (and sometimes appear prematurely in Stage 0).

This causes:

- Reordering and renumbering risks.
- Tool ambiguity (“does `74` mean order, identity, filename stem?”).
- Agent confusion when an input resolves differently depending on scope.

We want a model that is:

- Stable under reordering and refactoring.
- Friendly for both humans and agents.
- Deterministic in the face of ambiguity.

## Detailed Design

### Definitions

- **Stable ID**: immutable ULID stored in TOML.
- **Slug**: optional human-friendly name, scoped (see below), mutable (via explicit rename), and used for convenience.
- **Canonical reference**: a typed ID reference that tools emit and accept (e.g. `task@<ulid>`).

### Canonical storage (normative)

Artifacts that participate in linking/referencing MUST have a stable `id`.

- `id` MUST be a ULID.
- `id` MUST never change once created.

Artifacts MAY also have:

- `slug` (string)
- `label` (string, human display)

The slug is not identity.

### Scoped slug uniqueness (normative)

Slugs MUST be unique within a narrow, natural scope:

- Epoch slug unique among epochs.
- Phase slug unique within its epoch.
- Task slug unique within its phase.
- Idea slug unique within the global ideas list.

Artifacts that do _not_ need human-friendly handles SHOULD not have slugs:

- Walkthrough entries: ULID only.
- Feedback threads: ULID only.

### Slug collisions (normative)

When a create/rename request specifies a slug that collides within the relevant scope:

- The tool MUST fail.
- The tool MUST provide steering that instructs the agent/LLM to pick a different slug.

We explicitly prefer this over auto-suffixing because:

- LLM workflows can retry quickly.
- Humans typically want intentionally distinct names.

### Resolution rules and “echo canonical”

User inputs MAY refer to artifacts by:

- Canonical reference `type@<ulid>` (preferred)
- Scoped slug (contextual)
- Legacy alias (during migration)

Resolution rules:

1. Resolve **within the command’s natural scope** first (e.g. for task operations: active phase).
2. If there is exactly one match, proceed.
3. If zero matches, fail with steering.
4. If multiple matches, fail with steering and list candidates.

On success, tools MUST echo:

- Human output: include the canonical reference.
- Machine output (`--format json`): include the canonical reference as the primary identifier.

### Legacy IDs and migration policy

We treat existing identifiers as:

- **Numeric-ish identifiers** (e.g. `phase-74`): treated as legacy aliases only. They should be removed from canonical storage and replaced by a ULID + a derived slug.
- **Semantic identifiers** (e.g. `migrate-tests`): generally become the slug (subject to scoped uniqueness).

During migration:

- Keep `aliases = ["phase-74", ...]` for compatibility.
- Prefer steering toward canonical references.

### RFCs: stable ULID + Stage 1 numbering

RFCs are special:

- They already have a governance handle (RFC number like `0057`).
- They also want stable identity for toolability and linking.

Proposal:

- Every RFC has a ULID identity.
- RFC numbers are assigned at Stage 1 promotion (and remain stable thereafter).
- Stage 0 RFCs should be attached to a phase or idea.

Implications:

- Stage 0 should not be an unbounded blob of disconnected proposals.
- Promoting to Stage 1 is the point at which we commit to a number.

### Corpus control (tightening Stage 0)

Introduce a verifier (eventually) that:

- Flags Stage 0 RFCs that are not attached to any phase/idea.
- Encourages “small curated inbox” rather than accumulating unowned drafts.

This is explicitly aligned with the goal: “get the RFC corpus under control after position.”

## Minimal migration slice (recommended)

We should land this work in a narrow, high-leverage slice that supports the Position protocol first, and defer wider identity rewrites until we have confidence.

### Slice 1: Plan artifacts only (tasks/phases)

1. Add stable ULIDs to:

   - Epochs
   - Phases
   - Tasks

2. Introduce scoped slugs (optional) for:

   - Epochs
   - Phases
   - Tasks

3. Implement resolver behavior:
   - Accept canonical refs (future `type@<ulid>`), slugs (scoped), and legacy IDs as aliases.
   - On success, always echo canonical in both human and JSON output.
   - On ambiguity, fail with deterministic steering.
   - On slug collision within scope, fail with “pick a different slug” steering.

This slice directly unlocks reliable `before/after/prepend/append` operations across the plan.

### Slice 2: Ideas / feedback / walkthrough entries

Adopt per-artifact policies:

- Ideas: ULID + optional slug (unique within global ideas scope)
- Feedback: ULID only
- Walkthrough: ULID only

### Defer: RFC ULID identity

RFCs already have a stable governance handle (number) and have additional complexity due to directory stage projections.

We should defer “RFC ULID as canonical identity” until after:

- Position protocol is implemented and in active use, and
- we are ready to do a dedicated RFC corpus coherence pass.

## Drawbacks

- Requires migrations for existing plan/task IDs.
- Adds a small conceptual split between slug and identity.

## Alternatives

- Slugs-only: breaks stable links under rename.
- Integers-only: reintroduces coordination/renumbering problems.
- Auto-suffix collisions: convenient, but yields low-quality, confusing names in LLM workflows.

## Unresolved Questions

- Exact canonical reference syntax: do we standardize on `type@<ulid>` everywhere?
- How long do we keep legacy aliases?
- Do we want `label` separate from `slug` everywhere or only for some artifact types?

