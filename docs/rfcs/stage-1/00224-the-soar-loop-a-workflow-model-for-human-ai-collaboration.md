<!-- exo:224 ulid:01kmzxey0hy7z6mk7b0j9aahe0 -->

# RFC 224: The SOAR Loop: A Workflow Model for Human-AI Collaboration


# RFC 00224: The SOAR Loop: A Workflow Model for Human-AI Collaboration

## Summary

SOAR (Status → Orient → Act → Review) is a tactical workflow loop for human-AI collaboration, adapted from Boyd's OODA loop. It provides a structured approach to executing work within the Exosuit system, complementing the strategic planning layer (epochs, phases, RFCs).

## Motivation

Agents and humans working together need a shared mental model for:

1. **Knowing where they are** - Current state relative to plan
2. **Deciding what to do** - Synthesizing options from context
3. **Executing work** - Taking concrete actions
4. **Verifying results** - Confirming work meets expectations

Without this structure, sessions drift, context is lost, and work becomes reactive rather than intentional.

### SOAR vs OODA

SOAR is inspired by Boyd's OODA loop but adapted for our context:

| OODA       | SOAR       | Adaptation Rationale                                                |
| ---------- | ---------- | ------------------------------------------------------------------- |
| Observe    | **Status** | We check drift from _plan_, not raw environmental sensing           |
| Orient     | **Orient** | Unchanged—synthesize context, update mental model, generate options |
| Decide→Act | **Act**    | Human decides (explicitly or by delegation); AI acts                |
| (implicit) | **Review** | Explicit verification phase (OODA leaves this in next Observe)      |

Key insight: OODA assumes a single actor. SOAR assumes a human-AI pair where the human retains decision authority.

## Detailed Design

### The Loop

```
    ┌──────────────────────────────────────────────┐
    │                                              │
    ▼                                              │
┌────────┐    ┌────────┐    ┌─────┐    ┌────────┐  │
│ STATUS │ ─▶ │ ORIENT │ ─▶ │ ACT │ ─▶ │ REVIEW │──┘
└────────┘    └────────┘    └─────┘    └────────┘
```

### Phase 1: Status

**Question**: "Where am I? What's the delta from plan?"

**Purpose**: Detect current state and drift from the planned trajectory.

**Tools**:

- `exo-status` - Quick snapshot: phase, tasks, git state
- `exo-phase` - Detailed phase breakdown
- `exo-list-tasks` - Task-level view

**Outputs**:

- Current phase/epoch context
- Completed vs pending work
- Git dirty state
- Blockers or anomalies

### Phase 2: Orient

**Question**: "What are my options? What should I do next?"

**Purpose**: Synthesize context, update mental model, generate actionable options.

**Tools**:

- `exo-steering` - Confidence-scored next actions
- `exo-context` - Full context dump for recovery
- `exo-goal-list` - Goals with RFC linkage

**Outputs**:

- Ranked action options with rationale
- Repair paths if state is inconsistent
- Pending intents requiring attention

### Phase 3: Act

**Question**: "Execute the chosen action."

**Purpose**: Perform concrete work—code, tests, documentation.

**Tools**:

- `exo-task-*` - Task mutation (add, complete, update)
- `exo-tdd-*` - Test-driven development cycle
- `exo-impl-*` - Implementation steps
- `exo-goal-*` - Goal management

**Constraint**: Human decides (explicitly or by delegation), AI executes.

### Phase 4: Review

**Question**: "Did it work? What did we learn?"

**Purpose**: Verify work meets expectations, capture learnings.

**Tools**:

- `exo-verify` - Run verification checks (proposed, not yet implemented)
- Human judgment - Visual inspection, testing
- PR review - Code review process

**Outputs**:

- Verification status (pass/fail)
- Learnings to capture
- Next iteration trigger

### Plan Tools (Orthogonal)

Strategic planning tools operate _across_ SOAR cycles—they're not part of the tactical loop:

- `exo-rfc-*` - RFC lifecycle management
- `exo-epoch-*` - Epoch transitions
- `exo-plan` - Roadmap and big picture

**When to use Plan tools**:

- Starting/ending epochs
- Creating/promoting RFCs
- Strategic reviews
- Roadmap adjustments

### The Loop in Practice

**Session Start**:

```
Status → Orient
```

"Where am I? What's next?"

**During Work**:

```
Act → Review → Status (tight loop)
```

Execute, verify, check state.

**At Decision Points**:

```
Orient
```

"What are my options?"

**Strategic Moments**:

```
Plan tools
```

RFC work, epoch transitions.

### SOAR → GitHub Alignment

The SOAR loop maps onto GitHub artifacts:

| SOAR Phase    | GitHub Artifact                     |
| ------------- | ----------------------------------- |
| Status        | Branch state, PR status, CI results |
| Orient        | PR description, review comments     |
| Act           | Commits, code changes               |
| Review        | PR review, CI verification          |
| (Loop closes) | PR merge → new Status on main       |

A complete SOAR cycle often culminates in a **PR**. The PR is a Review artifact—it captures what was done and invites verification. Merging closes the loop.

## Tool Categorization Audit

> **Audit Status**: ✅ Complete (2026-02-03)
>
> Audited 30 LM tools from `lm_tool_metadata.rs` and VS Code extension.

All exo-\* tools should be categorized into SOAR buckets. Tools that don't fit cleanly are candidates for:

1. Renaming to clarify intent
2. Refactoring to separate concerns
3. Deprecation if redundant

### Audit Summary

| SOAR Phase     | Tool Count | Coverage     | Notes                   |
| -------------- | ---------- | ------------ | ----------------------- |
| **Status**     | 5          | 🟢 Good      | Core tools present      |
| **Orient**     | 4          | 🟢 Good      | Steering is strong      |
| **Act**        | 15         | 🟢 Excellent | Most tools are Act      |
| **Review**     | 0          | 🔴 **Gap**   | Critical missing phase  |
| **Plan**       | 5          | 🟢 Good      | RFC/Epoch tools present |
| **Diagnostic** | 1          | 🟡 Adequate  | Only logs tool          |

### Status Tools (5)

_"Where am I? What's the delta from plan?"_

| Tool             | Confidence | Notes                                  |
| ---------------- | ---------- | -------------------------------------- |
| `exo-status`     | 🟢 High    | Core Status tool - quick snapshot      |
| `exo-phase`      | 🟢 High    | Detailed phase breakdown               |
| `exo-list-tasks` | 🟢 High    | Task-level view of current phase       |
| `exo-epoch-list` | 🟡 Medium  | Lists epochs - could be Status or Plan |
| `exo-rfc-list`   | 🟡 Medium  | Lists RFCs - could be Status or Plan   |

### Orient Tools (4)

_"What are my options? What should I do next?"_

| Tool            | Confidence | Notes                                             |
| --------------- | ---------- | ------------------------------------------------- |
| `exo-steering`  | 🟢 High    | Core Orient tool - confidence-scored actions      |
| `exo-context`   | 🟢 High    | Full context for recovery/handoff                 |
| `exo-goal-list` | 🟡 Medium  | Borderline Status/Orient - shows what needs doing |
| `exo-inbox`     | 🟡 Medium  | Cross-cutting - surfaces pending intents          |

### Act Tools (15)

_"Execute the chosen action."_

| Tool                     | Confidence | Notes                      |
| ------------------------ | ---------- | -------------------------- |
| `exo-add-task`           | 🟢 High    | Adds task to active phase  |
| `exo-task-complete`      | 🟢 High    | Marks task done            |
| `exo-task-remove`        | 🟢 High    | Removes task               |
| `exo-task-update`        | 🟢 High    | Updates task title         |
| `exo-task-reorder`       | 🟢 High    | Reorders task              |
| `exo-tdd-start`          | 🟢 High    | Starts TDD cycle           |
| `exo-tdd-red`            | 🟢 High    | Confirms failing test      |
| `exo-tdd-green`          | 🟢 High    | Confirms passing test      |
| `exo-impl-add-step`      | 🟢 High    | Adds implementation step   |
| `exo-impl-update-status` | 🟢 High    | Updates step status        |
| `exo-impl-remove-step`   | 🟢 High    | Removes step               |
| `exo-impl-clear-steps`   | 🟢 High    | Clears all steps           |
| `exo-idea`               | 🟢 High    | Captures idea to backlog   |
| `exo-phase-start`        | 🟡 Medium  | Lifecycle - starts phase   |
| `exo-phase-finish`       | 🟡 Medium  | Lifecycle - finishes phase |

### Review Tools (0) — 🔴 CRITICAL GAP

_"Did it work? What did we learn?"_

**No dedicated Review tools exist.** This is a critical gap in the SOAR loop.

Current workaround: Review is implicit—users manually inspect results or use `exo-phase` to see completion status.

**Missing tools that should exist:**

- `exo-verify` - Run verification checks
- `exo-criteria-list` - List acceptance criteria
- `exo-criteria-satisfy` - Mark criterion satisfied

### Plan Tools (5)

_Orthogonal—strategic tools that operate across SOAR cycles._

| Tool               | Confidence | Notes                         |
| ------------------ | ---------- | ----------------------------- |
| `exo-plan`         | 🟢 High    | Core Plan tool - roadmap view |
| `exo-rfc-create`   | 🟢 High    | Creates RFC                   |
| `exo-rfc-promote`  | 🟢 High    | Promotes RFC stage            |
| `exo-epoch-start`  | 🟢 High    | Starts epoch                  |
| `exo-epoch-finish` | 🟢 High    | Finishes epoch                |

### Diagnostic Tools (1)

_Cross-cutting tools that don't fit SOAR phases._

| Tool       | Confidence | Notes                           |
| ---------- | ---------- | ------------------------------- |
| `exo-logs` | 🟡 Medium  | Debugging/diagnostic - not SOAR |

### Issues Found

#### Naming Inconsistencies

| Current Name   | Internal Name  | Issue                       |
| -------------- | -------------- | --------------------------- |
| `exo-steering` | `root.map`     | Internal name is "map"      |
| `exo-phase`    | `phase.status` | Could be `exo-phase-status` |
| `exo-plan`     | `plan.review`  | Could be `exo-plan-review`  |

#### Cross-Cutting Tools

These tools span multiple SOAR phases:

- **`exo-goal-list`**: Borderline Status/Orient—shows current state but also informs decisions
- **`exo-inbox`**: Surfaces in Orient (pending intents) but also provides Status (what's waiting)

#### Unexposed Namespaces

These CLI namespaces have operations but no LM tools:

- **`criteria`**: `add`, `list`, `remove`, `satisfy`, `unsatisfy` — would support Review
- **`commit`**: `create`, `status` — would support Act/Status

## Implementation Plan

### Phase 1: Documentation ✅

- [x] Formalize SOAR in this RFC
- [x] Update copilot-instructions.md to reference RFC
- [ ] Add SOAR section to manual

### Phase 2: Tool Audit ✅

- [x] Categorize all tools into SOAR buckets (30 tools audited)
- [x] Identify misfit tools (cross-cutting: goal-list, inbox)
- [x] Propose renames/refactors (see Issues Found)
- [x] **Key Finding**: Review phase has 0 tools (critical gap)

### Phase 3: Holistic Tool Review (Stage 1→2 work)

Before implementing changes, conduct a holistic review of all tools:

- [ ] Evaluate whether each tool earns its place in the SOAR model
- [ ] Identify tools to consolidate, rename, or deprecate
- [ ] Design Review phase tools (criteria.list/satisfy are candidates, not commitments)
- [ ] Adjust SOAR theory if practice reveals missing concepts
- [ ] Ensure tool surface coherently reflects SOAR (not just categorized into it)

**Goal**: The tool surface should _embody_ SOAR, not just be labeled with it.

### Phase 4: Tool Implementation

- [ ] Update LM tool descriptions with SOAR context
- [ ] Add `soar_phase` field to `LmToolOverride` struct
- [ ] Implement Review tools based on Phase 3 design
- [ ] Update RFC 0136 with SOAR alignment

### Phase 5: Axiom Integration

- [ ] Create formal axiom in axioms.workflow.toml
- [ ] Wire axiom into steering logic
- [ ] Add SOAR phase to steering output

## Alternatives Considered

### Keep OODA as-is

Rejected: OODA's implicit Review phase and single-actor assumption don't fit human-AI collaboration.

### Four-phase without Review

Rejected: Explicit verification is critical for AI work quality. Leaving it implicit leads to drift.

### Merge Status and Orient

Rejected: They serve different purposes—Status is factual ("what is"), Orient is analytical ("what should be").

## Prior Art

- **OODA Loop** (John Boyd) - Military decision-making cycle
- **PDCA** (Deming) - Plan-Do-Check-Act quality cycle
- **GTD** (David Allen) - Capture-Clarify-Organize-Reflect-Engage

SOAR combines OODA's tempo with PDCA's explicit verification, adapted for the unique dynamics of human-AI pair programming.

## Unresolved Questions

1. Should `exo-goal-list` be Status or Orient? (Currently borderline)
2. How should diagnostic tools (`exo-logs`) be categorized?
3. Should SOAR phase be visible in tool picker UI?
4. How does SOAR interact with the PER (Prepare→Execute→Review) protocol?
5. **Tool surface design**: Should we slim down the 30 tools to a smaller, more coherent set that embodies SOAR? Or does the current breadth serve real needs?
6. **Theory vs practice**: Does the audit reveal gaps in SOAR theory itself? (e.g., is "Diagnostic" a missing phase, or truly orthogonal?)

## Future Possibilities

- **SOAR-aware steering**: Steering could suggest tools based on detected SOAR phase
- **Session replay**: Reconstruct SOAR cycles from tool invocation history
- **Metrics**: Track time spent in each SOAR phase for workflow optimization

## Related RFCs

- RFC 00240: Fractal SOAR & The Goal Loop — Extends SOAR fractally to nested goal loops
- RFC 10170: Mutation Boundaries in Feedback Loops — ODM loop refines Act/Review transitions with explicit mutation boundaries and commit points
