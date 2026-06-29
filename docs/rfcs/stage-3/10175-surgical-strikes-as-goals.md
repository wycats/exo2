<!-- exo:10175 ulid:01kmzxbcy22vxd1e534ng2tef4 -->


# RFC 10175: Surgical Strikes as Goals

- **Supersedes**: RFC 0048

## Summary

A surgical strike is a goal with `kind = "strike"` — an urgent, reactive piece of work that becomes the active focus without requiring goal reordering. Strikes use the standard goal lifecycle (status, completion_log, history) with additional UI and steering affordances.

## Core Concept

**A strike IS a goal.** The only differences are:

- **Urgency**: Bypasses normal phase planning — you don't schedule strikes
- **Activation**: Creating a strike makes it "active" immediately
- **Visual distinction**: Marked separately so phase review isn't confusing
- **Steering priority**: Active strike = current focus for suggestions

Everything else — logging, completion, history preservation — is standard goal behavior.

## Schema

```toml
# canonical task state (under active phase)
[[epochs.phases.goals]]
id = "strike-1769812406"
label = "Fix critical auth bug"
status = "in-progress"
kind = "strike"
started_at = "2026-01-30T22:33:26Z"
description = "Restore service functionality"
```

The `kind = "strike"` field is the only schema difference from regular goals.

## Commands

| Command                                        | Behavior                                                      |
| ---------------------------------------------- | ------------------------------------------------------------- |
| `exo strike start "Name" --goal "Description"` | Creates goal with `kind = "strike"`, `status = "in-progress"` |
| `exo strike finish`                            | Sets `status = "completed"`, prompts for completion_log       |
| `exo strike abort`                             | Sets `status = "aborted"`                                     |

These are convenience commands, not aliases. They enforce strike-specific semantics:

1. **Singleton constraint**: Only one active strike at a time
2. **Immediate activation**: No reordering needed — the strike is "active" by existing
3. **Bypass upgrade gate**: Strikes skip the upgrade check (RFC 0084)

## Singleton Constraint

**Only one strike may be active at a time.** `exo strike start` errors if a goal with `kind = "strike"` AND `status = "in-progress"` already exists.

**Rationale**: Strikes represent "drop everything" urgency. Multiple concurrent strikes dilute that urgency. If you need multiple urgent items, they should be tasks within the single active strike.

Completed strikes remain in the phase's goal history — the constraint only applies to active strikes.

## UI Affordances

### Active Strike

When a strike is active, the Phase Details pane shows:

- **Visual distinction**: Lightning bolt icon with warning color
- **Pinned position**: Active strike appears at top, separated from other goals
- **Time context**: "Started: 2 hours ago"

```
┌─────────────────────────────────────────────────────────┐
│ ⚡ ACTIVE STRIKE                                        │
│   Fix critical auth bug                                 │
│   Started: 2 hours ago                                  │
├─────────────────────────────────────────────────────────┤
│ ▼ Goals (4)                                             │
│   ✓ Implement feature A                                 │
│   ○ Implement feature B                                 │
└─────────────────────────────────────────────────────────┘
```

### Completed Strike

Once completed/aborted:

- Moves to regular goals list (no longer pinned)
- Retains lightning bolt icon with completion color
- Part of phase history, just like any other completed goal

## Steering

While a strike is active, steering prioritizes strike-related actions. This is the key behavioral difference — the strike becomes the agent's focus until resolved.

## Implementation Status

- ✅ Schema: `kind` field on goals
- ✅ Commands: `strike start/finish/abort`
- ✅ Migration: `[[surgical_strikes]]` → goals with `kind = "strike"`
- ✅ UI: Visual distinction in Phase Details

## References

- RFC 00177: Goals and Tasks - Unified Work Item Model
- RFC 0048: Surgical Strike Workflow (superseded)
- RFC 0084: Pluggable Upgrade System
