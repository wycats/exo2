<!-- exo:226 ulid:01kmzxey15dhzy30bxjp1v1h22 -->

# RFC 226: Lightweight RFC Path: Minimal Ceremony for Simple Decisions


# RFC 00226: Lightweight RFC Path: Minimal Ceremony for Simple Decisions

## Summary

Defines a "lightweight path" through the RFC process for simple decisions that don't require full ceremony at each stage. Extends RFC 0108 (Refined Staged RFC Process) without contradicting it.

## Motivation

### The Ceremony Objection

A common objection to Exosuit: "This looks like too much ceremony for my project." The current RFC process (RFC 0108) is optimized for major architectural decisions—the kind that need careful review at each stage.

But not every decision is architectural:

- Naming conventions
- Small feature additions
- Tool configuration choices
- Documentation structure decisions

These decisions benefit from being recorded (searchable, permanent) but don't need 3 pages of spec and formal review at each stage.

### The Goal

Enable **minimal artifacts per stage** while preserving the RFC structure. A 3-paragraph RFC can still go through all stages—just with lighter artifacts at each step.

## Detailed Design

### What Makes an RFC "Lightweight"?

An RFC qualifies for lightweight treatment when:

| Criterion              | Description                                              |
| ---------------------- | -------------------------------------------------------- |
| **Bounded scope**      | Affects single file, single feature, or single subsystem |
| **High reversibility** | Easy to change later without cascading effects           |
| **Local impact**       | Doesn't affect other systems or create dependencies      |
| **Low controversy**    | Unlikely to generate significant debate                  |

### Lightweight Frontmatter

Add `lightweight: true` to RFC frontmatter:

```yaml
---
title: My Simple Decision
stage: 1
feature: Tooling
lightweight: true
---
```

### Artifact Requirements by Stage

| Stage             | Full Process                    | Lightweight Path                   |
| ----------------- | ------------------------------- | ---------------------------------- |
| **0 (Idea)**      | Vision + motivation             | 1 paragraph summary                |
| **1 (Proposal)**  | Detailed rationale              | Brief rationale (3 paragraphs max) |
| **2 (Draft)**     | Full spec + implementation plan | Inline implementation notes        |
| **3 (Candidate)** | Implementation + Manual PR      | Implementation only                |
| **4 (Stable)**    | Manual sync required            | Manual sync batched                |

### Stage Transitions

Lightweight RFCs still go through all stages, but transitions can be **combined**:

- **0→1 + 1→2**: "Is this worth doing AND is the shape right?" (single review)
- **2→3**: Can be immediate if implementation is trivial
- **3→4**: Batched with other lightweight RFCs

### Manual Update Cadence

**Full RFCs**: Manual sync required at Stage 4 (current behavior)

**Lightweight RFCs**: Manual sync batched:

- Monthly batch updates, OR
- At epoch boundaries, OR
- When 5+ lightweight RFCs accumulate

Track pending updates in `docs/manual/pending-sync.md` (new file).

### CLI Support

```bash
# Create lightweight RFC
exo rfc create --title "..." --lightweight

# List pending Manual syncs
exo rfc pending-sync

# Batch promote lightweight RFCs
exo rfc batch-promote --stage 4
```

## Examples

### Lightweight RFC Example

```markdown
---
title: Use kebab-case for CLI command names
stage: 2
feature: CLI
lightweight: true
---

# RFC 00XXX: Use kebab-case for CLI command names

## Summary

All CLI commands should use kebab-case (e.g., `exo phase-start`) not camelCase or snake_case.

## Motivation

Consistency with Unix conventions. Most CLI tools use kebab-case.

## Design

Apply to all new commands. Migrate existing commands at next major version.
```

Total: ~50 words. Still an RFC. Still permanent. Still searchable.

### When NOT to Use Lightweight

- Architectural decisions (data model changes, new subsystems)
- Breaking changes
- Security-sensitive decisions
- Decisions that affect multiple teams/systems

## Relationship to RFC 0108

This RFC **extends** RFC 0108, not replaces it:

- RFC 0108 defines the stage model (0→1→2→3→4)
- RFC 0108 defines steering events at each transition
- This RFC adds a "lightweight" modifier that reduces artifact requirements
- Stage semantics remain unchanged

## Drawbacks

1. **Two-tier system**: Risk of "everything is lightweight" abuse
2. **Batched Manual updates**: Manual may lag behind lightweight decisions
3. **Judgment required**: Deciding "is this lightweight?" adds cognitive load

## Alternatives Considered

### 1. Use ideas.toml for simple decisions

**Rejected**: Ideas don't have stages, aren't promoted, don't become permanent record.

### 2. Amend RFC 0108 directly

**Rejected**: RFC 0108 is already complex. Better to extend than modify.

### 3. Skip stages entirely for lightweight RFCs

**Rejected**: Stages provide value (tracking, review points) even with minimal artifacts.

## Implementation Plan

| Phase | Work                                  | Dependencies |
| ----- | ------------------------------------- | ------------ |
| 1     | Add `lightweight` frontmatter support | None         |
| 2     | Update `exo rfc create --lightweight` | Phase 1      |
| 3     | Create `pending-sync.md` tracking     | None         |
| 4     | Add `exo rfc pending-sync` command    | Phase 3      |
| 5     | Update RFC governance docs            | Phases 1-4   |

**Complexity**: Low-medium. Mostly CLI and documentation changes.

## Unresolved Questions

1. Should lightweight RFCs skip Stage 1 entirely (0→2 direct)?
2. What's the right batch cadence for Manual updates?
3. Should there be a "micro" level below lightweight?

