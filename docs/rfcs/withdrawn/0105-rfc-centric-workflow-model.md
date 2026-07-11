<!-- exo:105 ulid:01kg5kp2g7c2pzzjs2y64chpxn -->

# RFC 105: RFC-Centric Workflow Model

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

# RFC 0105: RFC-Centric Workflow Model

## Summary

Reframe the Exosuit workflow model so that **RFCs are the primary unit of intent** and **phases are tactical containers for advancing groups of RFCs through their lifecycle**. This resolves a fundamental disconnect between how the user thinks about planning ("which design decisions need to advance?") and how the current system asks questions ("what tasks are in the current phase?").

## Motivation

The current workflow model treats phases as primary with tasks inside them, and RFCs as a parallel track that gets "implemented" at some point. This creates cognitive friction:

| Current Model                   | User's Mental Model                      |
| ------------------------------- | ---------------------------------------- |
| "What tasks are in this phase?" | "Which design decisions am I advancing?" |
| Phase contains tasks            | Phase advances RFCs through stages       |
| RFCs are documentation          | RFCs are the work itself                 |

The RFC lifecycle (Stage 0 → 1 → 2 → 3 → 4) is actually the **real workflow**. Phases become:

> "This phase will advance RFC-X from Stage 1→2 and RFC-Y from Stage 2→3"

## Detailed Design

### The RFC Lifecycle as the True Workflow

| RFC Stage   | Meaning             | Phase Mapping              |
| ----------- | ------------------- | -------------------------- |
| **Stage 0** | Idea captured       | Idea triage work           |
| **Stage 1** | Approved proposal   | Phase: draft specification |
| **Stage 2** | Draft specification | Phase: implement RFC       |
| **Stage 3** | Implemented         | Phase: stabilize, document |
| **Stage 4** | Stable              | No work needed             |

### Phases as RFC Batches

A phase definition shifts from:

```toml
# Current: task-centric
[[phase.tasks]]
name = "Implement feature X"
status = "pending"
```

To:

```toml
# Proposed: RFC-centric
[[phase.rfcs]]
id = "0050"
target_stage = 2  # Advance from 1 → 2
```

The tasks within a phase become **derived from the RFC stage transitions**:

- Advancing 0→1: Triage the idea, write problem statement
- Advancing 1→2: Draft the specification
- Advancing 2→3: Implement the feature, update Manual
- Advancing 3→4: Stabilize, get user feedback

### The Unified Queue

Everything becomes a queue item:

| Item Type      | Current                 | Proposed                                  |
| -------------- | ----------------------- | ----------------------------------------- |
| User intents   | `inbox.toml`            | Unified queue                             |
| Ideas          | `ideas.toml` (separate) | Queue item: `category = "idea-triage"`    |
| RFC promotions | Manual                  | Queue item: `category = "rfc-promotion"`  |
| Phase starts   | Manual                  | Queue item: `category = "phase-approval"` |

The `IntentCategory` enum expands:

```rust
pub enum IntentCategory {
    Correction,   // Existing
    Guidance,     // Existing
    Question,     // Existing
    Priority,     // Existing
    IdeaTriage,   // NEW: Idea needs review
    RfcPromotion, // NEW: RFC stage transition
    PhaseApproval,// NEW: Phase ready to start
}
```

### Dashboard Integration

The dashboard becomes RFC-flow oriented:

1. **RFC Stage Distribution**: Visual chart of RFCs by stage
2. **Unified Queue**: All actionable items in one place
3. **Phase as RFC Batch**: "This phase advances: RFC-50 (1→2), RFC-28 (2→3)"
4. **"What's Next" derived from queue**: Top queue item = next action

## Implementation Strategy

### Phase 1: Queue Unification (Minimal Viable)

1. Extend `IntentCategory` with 3 new variants
2. Wire `exo idea add` to create inbox item
3. Update dashboard to show unified queue
4. Add `exo queue next` command

### Phase 2: RFC-Centric Phase Definition

1. Add `[[phase.rfcs]]` to plan.toml schema
2. Derive tasks from RFC stage transitions
3. Dashboard shows "advancing RFCs" not "tasks"

### Phase 3: Full Integration

1. Phase completion triggers RFC stage updates
2. RFC promotion creates queue items
3. Dashboard visualizes RFC flow

## Relationship to Existing RFCs

- **RFC 0050 (Async Intent Channel)**: Foundation for queue
- **RFC 0064 (Phase State Machine)**: Extends with RFC-centric states
- **RFC 0126 (Dashboard V2)**: UI for RFC flow visualization
- **RFC 0094 (Sidebar-First UI)**: Queue appears in sidebar

## Alternatives Considered

### Keep Task-Centric Model

Continue with phases containing tasks, RFCs as parallel track. **Rejected**: Creates the cognitive disconnect this RFC addresses.

### Pure RFC Model (No Phases)

Work directly on RFCs without phase containers. **Rejected**: Phases provide valuable batching, momentum, and review checkpoints.

## Open Questions

1. How do we handle phases that span multiple RFC promotions of different types?
2. Should `exo phase start` auto-derive tasks from target RFC promotions?
3. How do we migrate existing phase definitions?
