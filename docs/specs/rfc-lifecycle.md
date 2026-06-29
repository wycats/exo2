# RFC Lifecycle Specification

> Derived from RFC 00238 (Pipeline-Aware Self-Model) and existing docs/rfcs/README.md

## Stage 4: Living Canon

### Definition

Stage 4 ("Stable") is the home of the project's **canonical state** — the living documentation of how the system works right now.

### Entry Criteria

An RFC may be promoted to Stage 4 when:

1. **Implementation complete**: The feature is built and working
2. **Tests passing**: Verification demonstrates correctness
3. **Manual updated**: `docs/manual/` reflects the feature
4. **Stable in use**: The feature has been used without critical issues

### What Stage 4 Means

| Aspect          | Description                                                          |
| --------------- | -------------------------------------------------------------------- |
| **Authority**   | The RFC represents shipped, stable behavior                          |
| **Editability** | RFCs remain historical; manual is the living doc                     |
| **Coherence**   | Stage 4 RFCs should be mutually consistent                           |
| **Maintenance** | If reality changes, manual updates first, then RFC may be superseded |

### When Canon Status Ends

An RFC loses canon status when:

1. **Superseded**: A newer RFC replaces the feature
   - The old RFC stays in Stage 4 as historical record
   - `superseded_by:` frontmatter links to the replacement
   - Consider moving to `archive/` if it creates confusion

2. **Deprecated**: The feature is being phased out
   - Add `deprecated: true` to frontmatter
   - Manual should note deprecation

3. **Archived**: The feature was shipped but later removed/replaced
   - Move to `docs/rfcs/archive/`
   - Preserves history without polluting Stage 4

### Stage 4 vs Archive

| Location   | Meaning                                                         |
| ---------- | --------------------------------------------------------------- |
| `stage-4/` | Active canon — the system works this way today                  |
| `archive/` | Historical canon — the system _worked_ this way, now superseded |

The distinction: Stage 4 is the _current_ truth; archive is _past_ truth that's been replaced.

## Archive Directory

### Purpose

`docs/rfcs/archive/` holds RFCs that:

- Reached Stage 3 or 4 (were implemented)
- Are now superseded by newer work
- Should be preserved as historical record
- Should not pollute Stage 4 as active canon

### Entry Criteria

An RFC may be archived when:

1. **Was implemented**: Reached Stage 3+ (not just an idea)
2. **Is superseded**: A newer RFC has replaced it
3. **Would confuse**: Keeping it in stage-3/4 creates ambiguity about current truth

### What Archive Means

| Aspect           | Description                                           |
| ---------------- | ----------------------------------------------------- |
| **Historical**   | This was real — it shipped — but is no longer current |
| **Read-only**    | Don't edit archived RFCs                              |
| **Linked**       | `superseded_by:` points to the replacement            |
| **Discoverable** | Tools should still find archived RFCs when relevant   |

### Archive vs Withdrawn

| Directory    | Meaning                                                                  |
| ------------ | ------------------------------------------------------------------------ |
| `withdrawn/` | Never shipped — rejected, abandoned, or superseded before implementation |
| `archive/`   | Did ship — implemented but later replaced                                |

## Ephemeral RFC Retirement

### Problem

Not every RFC should reach Stage 4. Some RFCs are **project plans** — they organize a body of work, and when that work is done, they've served their purpose.

Examples:

- "Whiteboard Priorities" (RFC 00235) — sprint planning doc
- "Audit Cleanup" — phase organization RFC
- Feature flag RFCs — relevant only until flag is removed

### Options Considered

1. **Archive folder**: Move to `docs/rfcs/archive/`
   - Pro: Clean separation
   - Con: Mixes "shipped but superseded" with "ephemeral completed"

2. **PR association**: RFCs get linked to the PR(s) they guided
   - Pro: Lifecycle aligns with code lifecycle
   - Pro: "RFCs for this PR" is a useful query
   - Con: Requires tooling investment

3. **Withdrawn with reason**: Use `withdrawn/` with `reason: completed`
   - Pro: Uses existing infrastructure
   - Con: Conflates "rejected" with "completed"

### Recommendation

**Use archive with `kind: ephemeral`** in frontmatter:

```yaml
---
title: Whiteboard Priorities Sprint
kind: ephemeral
archived_reason: completed
completed_at: 2026-02-06
---
```

This distinguishes:

- `archive/` with no `kind`: Shipped feature, now superseded
- `archive/` with `kind: ephemeral`: Organizing RFC, served its purpose

## Frontmatter Schema

### Current Fields

| Field           | Type   | Purpose               |
| --------------- | ------ | --------------------- |
| `title`         | string | RFC title             |
| `feature`       | string | Feature category      |
| `ulid`          | string | Unique identifier     |
| `exo.tool`      | string | Tool that created it  |
| `exo.protocol`  | number | Protocol version      |
| `superseded_by` | string | ID of superseding RFC |

### Proposed Additions

| Field             | Type   | Purpose                                          |
| ----------------- | ------ | ------------------------------------------------ |
| `kind`            | enum   | `feature` (default), `ephemeral`, `architecture` |
| `archived_reason` | enum   | `superseded`, `completed`, `deprecated`          |
| `deprecated`      | bool   | Feature is being phased out                      |
| `depends_on`      | array  | RFCs this depends on                             |
| `target_stage`    | number | Where this RFC is headed (for pipeline viz)      |
| `role`            | enum   | RFC's role in current phase (from RFC 00239)     |

### Role Values (from RFC 00239)

| Role         | Symbol | Meaning                           |
| ------------ | ------ | --------------------------------- |
| `driver`     | `▸`    | The RFC driving this phase's work |
| `touched`    | `·`    | Moved forward during this phase   |
| `supporting` | `○`    | Referenced but not advanced       |
| `blocked`    | `⊗`    | Can't progress due to dependency  |

## Open Questions

1. **Archive subdirectories**: Should archive have `archive/superseded/` vs `archive/ephemeral/`?

2. **Stage 4 cleanup**: Should we audit current Stage 4 RFCs and archive the superseded ones?

3. **Manual sync verification**: How do we verify Stage 4 RFCs are reflected in the manual?

---

_Status: Draft specification. Needs validation and implementation._
