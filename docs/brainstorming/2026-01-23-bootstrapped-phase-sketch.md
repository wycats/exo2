# Sketch: The Fully Bootstrapped Workflow

> **Status**: Design sketch for discussion
> **Date**: 2026-01-23
> **Purpose**: Map the full workflow from project plan through phases, how pieces interlock, and where synergies unlock
> **Slogan**: A system for building high-quality software with AI

---

## Key Differentiators to Embody

As we build, every slice should advance these differentiators:

1. **New Workflow Concepts for AI**: Not just adapting issues/PRs/docs — new primitives (phases, epochs, RFC pipeline, walkthroughs) rethought for AI collaboration
2. **Bending the Curve**: Structured data + coherent projections across CLI/UI/LM tools. No "which file should I update?"
3. **Anti-One-Shot**: Repeatable high-quality work across phases. Every phase should feel as good as the first one-shot.
4. **Strict Engineering Paradox**: Validation optimized for agent feedback. Rigor as enabler, not obstacle.
5. **VSCode Native**: UI is first-class, not afterthought. Each slice brings UI closer to concepts.
6. **Git Flows as Carrier**: Phase = PR. Lean into git, don't fight it.
7. **Sticky Artifacts**: Axioms, Manual, Codebase grow together coherently.
8. **Contextual Steering**: Thread of guidance from bootstrap → steering → completion → next phase.

**See**: [key-differentiators.md](2026-01-23-workflow-revamp/key-differentiators.md) for full detail.

---

## The Big Picture: Nested Loops

The workflow operates at four scales, each containing the next:

```
┌────────────────────────────────────────────────────────────────────────────┐
│                            PROJECT                                         │
│  The overall vision, axioms, and roadmap                                   │
│  Timescale: Months to years                                                │
│  Artifacts: plan.toml, axioms.*.toml, Manual                               │
│                                                                            │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                          EPOCH                                       │  │
│  │  A thematic milestone grouping related phases                        │  │
│  │  Timescale: Days to weeks                                            │  │
│  │  Artifacts: Epoch plan (goals, RFCs targeted, phases)                │  │
│  │                                                                      │  │
│  │  ┌────────────────────────────────────────────────────────────────┐  │  │
│  │  │                        PHASE                                   │  │  │
│  │  │  A focused deliverable with clear acceptance criteria          │  │  │
│  │  │  Timescale: Hours to a day                                     │  │  │
│  │  │  Artifacts: implementation-plan.toml, walkthrough, PR          │  │  │
│  │  │                                                                │  │  │
│  │  │  ┌──────────────────────────────────────────────────────────┐  │  │  │
│  │  │  │                      TASK                                │  │  │  │
│  │  │  │  Atomic work unit with TDD loop                          │  │  │  │
│  │  │  │  Timescale: Minutes to an hour                           │  │  │  │
│  │  │  │  Artifacts: Test, implementation, log entry              │  │  │  │
│  │  │  └──────────────────────────────────────────────────────────┘  │  │  │
│  │  └────────────────────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────┘
```

---

## The RFC Pipeline (Design → Implementation → Law)

RFCs are the **spine** of the planning process. They flow through stages, and phases are the mechanism that advances them:

```
                            THE RFC PIPELINE

  ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐
  │ Stage 0 │──▶│ Stage 1 │──▶│ Stage 2 │──▶│ Stage 3 │──▶│ Stage 4 │
  │  Idea   │   │Proposal │   │  Draft  │   │Candidate│   │ Stable  │
  └─────────┘   └─────────┘   └─────────┘   └─────────┘   └─────────┘
       │             │             │             │             │
       ▼             ▼             ▼             ▼             ▼
   Captured     Committed      Specified    Implemented    Codified
   in ideas     to explore     in detail    and tested     in Manual
       │             │             │             │             │
       └──────────────────────────────────────────────────────┘
                              │
                        PHASE WORK
                    (advances RFCs through stages)
```

### RFC Stage Transitions

| From  | To                             | Trigger                               | What Happens |
| ----- | ------------------------------ | ------------------------------------- | ------------ |
| 0 → 1 | User approves idea             | Create RFC file, move to stage-1/     |
| 1 → 2 | User approves direction        | Write detailed spec, move to stage-2/ |
| 2 → 3 | Phase completes implementation | Tests pass, move to stage-3/          |
| 3 → 4 | User confirms stability        | Update Manual, move to stage-4/       |

### Visualizing the Pipeline

At any moment, you should be able to see:

```
RFC Pipeline Status:
────────────────────────────────────────────────────────────────────
Stage 0 (Ideas):      [15 RFCs] ░░░░░░░░░░░░░░░
Stage 1 (Proposals):  [4 RFCs]  ████
Stage 2 (Drafts):     [2 RFCs]  ██  ← Next phases draw from here
Stage 3 (Candidates): [1 RFC]   █   ← Being implemented now
Stage 4 (Stable):     [11 RFCs] ███████████
────────────────────────────────────────────────────────────────────

Active Phase: "Dashboard Polish"
  Implementing: RFC 0136 (Stage 2→3), RFC 10102 (Stage 2→3)

Next Phase (planned): "CLI Overhaul"
  Will implement: RFC 0041 (Stage 2)
```

---

## Epochs: Organizing Multiple Phases

An **epoch** is a thematic milestone that groups related phases toward a coherent goal.

### Epoch Structure

```toml
# In plan.toml (or a dedicated epoch plan file)

[[epochs]]
id = "epoch-workflow-revival"
title = "Workflow Revival"
status = "active"
goal = "Restore the full bootstrapped workflow with RFC pipeline integration"

# RFCs this epoch aims to advance
target_rfcs = ["10090", "10102", "0041", "10047"]

# Phases in this epoch (some may be planned, not started)
[[epochs.phases]]
id = "phase-rfc-linkage"
title = "Phase-RFC Linkage"
status = "planned"
rfcs = ["10090"]
summary = "Link phases to RFCs, enable promotion on phase finish"

[[epochs.phases]]
id = "phase-task-discipline"
title = "Task-by-Task Discipline"
status = "planned"
rfcs = ["10102"]
summary = "TDD steering, walkthrough logging, sequential task execution"

[[epochs.phases]]
id = "phase-epoch-planning"
title = "Epoch Planning Tools"
status = "planned"
rfcs = ["0041"]
summary = "Tools for sketching epochs and staging future phase plans"
```

### Epoch Lifecycle

```
┌─────────────────────────────────────────────────────────────────────┐
│                         EPOCH START                                 │
│                                                                     │
│  1. Goal: What milestone does this epoch represent?                 │
│  2. Target RFCs: Which RFCs should advance during this epoch?       │
│  3. Phase Sketch: Outline phases needed (can be rough)              │
│  4. Success Criteria: How do we know the epoch is done?             │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         PHASE LOOP                                  │
│                                                                     │
│  For each phase in the epoch:                                       │
│    - Plan ahead: Stage implementation plan before starting          │
│    - Execute: Full phase lifecycle (start → tasks → finish)         │
│    - Reflect: Did this advance the epoch goal? Adjust if needed.    │
│                                                                     │
│  Key: Phases can be reordered, added, or removed as understanding   │
│       evolves (hermeneutic circle in action)                        │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         EPOCH FINISH                                │
│                                                                     │
│  1. RFC Review: Did target RFCs advance as planned?                 │
│  2. Manual Sync: Are Stage 4 RFCs reflected in Manual?              │
│  3. Retrospective: What worked? What to improve?                    │
│  4. Next Epoch: What's the next milestone?                          │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Planning Ahead: Staging Future Work

### The Problem with "Single Implementation Plan"

Currently, we only have `current/implementation-plan.toml`. This means:

- No way to sketch future phases before starting them
- Each phase start reconstructs from first principles
- "What's next?" requires digging through ideas, RFCs, plan.toml

### The Solution: Staged Implementation Plans

```
docs/agent-context/
  plan.toml                          # Project plan with epochs/phases
  current/
    implementation-plan.toml         # The ACTIVE phase
  staged/                            # FUTURE phases (sketched, not started)
    phase-rfc-linkage.toml           # Staged plan for next phase
    phase-task-discipline.toml       # Staged plan for phase after
    phase-epoch-planning.toml        # Rougher sketch, further out
```

### Staged Plan Structure

A staged plan is a _partial_ implementation plan — enough to know what we're doing, not fully fleshed out:

```toml
# staged/phase-rfc-linkage.toml

[phase]
id = "phase-rfc-linkage"
title = "Phase-RFC Linkage"
status = "staged"  # Not yet active
rfcs = ["10090"]
goal = "Link phases to RFCs and enable promotion on phase finish"
epoch = "epoch-workflow-revival"

# Rough acceptance criteria (refined when phase starts)
[[acceptance_criteria]]
description = "phase start prompts for RFC links"
status = "pending"

[[acceptance_criteria]]
description = "phase finish checks RFC promotion"
status = "pending"

# Rough task outline (refined when phase starts)
[[plan.sketch]]
task = "Add rfcs field to phase schema"
notes = "Update implementation-plan.toml schema"

[[plan.sketch]]
task = "Modify exo phase start to prompt for RFCs"
notes = "Interactive prompt or CLI flag"

[[plan.sketch]]
task = "Add RFC promotion check to phase finish"
notes = "Check if linked RFCs should advance stage"

# Not yet filled in (populated when phase starts):
# [plan.goals] - full implementation plan
# [walkthrough] - execution log
```

### The `exo phase prepare` Command

```
> exo phase prepare "Phase-RFC Linkage"

This creates a staged implementation plan. You can flesh it out before starting.

Linked RFCs: 10090
Goal: Link phases to RFCs and enable promotion on phase finish

Sketch tasks now, or leave rough? [sketch/rough]
> sketch

Task 1: Add rfcs field to phase schema
Task 2: Modify exo phase start to prompt for RFCs
Task 3: Add RFC promotion check to phase finish
Task 4: (blank to finish)
>

Staged plan created: docs/agent-context/staged/phase-rfc-linkage.toml
When ready, run: exo phase start phase-rfc-linkage
```

### Viewing What's Planned

```
> exo plan overview

PROJECT: Exosuit
═══════════════════════════════════════════════════════════════════

ACTIVE EPOCH: Workflow Revival
  Goal: Restore the full bootstrapped workflow with RFC pipeline integration
  Target RFCs: 10090, 10102, 0041, 10047

  ┌─ Phases ────────────────────────────────────────────────────────┐
  │ ✓ Phase 4: CI Integration                    [completed]        │
  │ ► Phase: RFC Linkage (staged)                [ready to start]   │
  │   Phase: Task Discipline (staged)            [sketched]         │
  │   Phase: Epoch Planning (staged)             [rough]            │
  └─────────────────────────────────────────────────────────────────┘

NEXT EPOCH: Dashboard & UX (planned)
  Goal: Improve human visibility into workflow state
  Target RFCs: (not yet assigned)
  Phases: (not yet sketched)

RFC PIPELINE:
  Stage 2 (ready to implement): 10090, 10102, 0041
  Stage 3 (in progress): —
  Stage 4 (awaiting manual sync): —
```

---

## Sticky Artifacts: Axioms and Manual

### The Axiom System

**Axioms** are core values that are **assumed sticky** — they persist across sessions and inform all decisions unless explicitly changed.

```
┌─────────────────────────────────────────────────────────────────────┐
│                         AXIOMS                                      │
│                                                                     │
│  Core values that constrain all other decisions:                    │
│                                                                     │
│  • Context is King: Workspace files are the source of truth         │
│  • Phased Execution: Plan → Implement → Verify → Transition         │
│  • Tooling Independence: Workspace works without the extension      │
│  • Laws vs. Code: RFCs are history, Manual is current reality       │
│  • User in the Loop: Human approves at phase boundaries             │
│                                                                     │
│  Axioms are NOT:                                                    │
│  • Design decisions (those go in RFCs)                              │
│  • Implementation details (those go in code)                        │
│  • Preferences (those go in settings)                               │
│                                                                     │
│  Axioms ARE:                                                        │
│  • Fundamental values that other decisions derive from              │
│  • Stable unless explicitly reconsidered                            │
│  • Referenced when evaluating RFCs and ideas                        │
└─────────────────────────────────────────────────────────────────────┘
```

### How Axioms Fit the Workflow

| Workflow Moment         | Axiom Check                                     |
| ----------------------- | ----------------------------------------------- |
| New idea captured       | "Does this align with our axioms?"              |
| RFC Stage 0→1 promotion | "Is this idea consistent with our core values?" |
| RFC Stage 2 review      | "Does the design honor our axioms?"             |
| Phase retrospective     | "Did we stay true to our principles?"           |

### The Manual: Compiled Law

The **Manual** is the codified reality of the system — what the agent should treat as ground truth:

```
┌─────────────────────────────────────────────────────────────────────┐
│                         MANUAL                                      │
│                                                                     │
│  docs/manual/                                                       │
│    ├── core-loop.md           # How the workflow operates           │
│    ├── architecture/          # System structure                    │
│    │     ├── file-structure.md                                      │
│    │     └── context-files.md                                       │
│    ├── features/              # What exists and how it works        │
│    │     ├── cli.md                                                 │
│    │     ├── steering.md                                            │
│    │     └── rfc-process.md                                         │
│    └── governance/            # How decisions are made              │
│          ├── axioms.md                                              │
│          └── rfc-stages.md                                          │
│                                                                     │
│  Updated when: RFC reaches Stage 4 (Stable)                         │
│  Read by agent: As source of truth for current system               │
│  Relationship to RFCs: Manual = compiled laws, RFCs = history       │
└─────────────────────────────────────────────────────────────────────┘
```

### How Axioms and Manual Build Up

```
                         TIME →

Session 1    Session 2    Session 3    Session 4
    │            │            │            │
    ▼            ▼            ▼            ▼
┌────────────────────────────────────────────────────────┐
│                    AXIOMS                              │
│  Stable foundation, rarely changed                     │
│  ─────────────────────────────────────────────────     │
│  [Axiom 1] ████████████████████████████████████████    │
│  [Axiom 2] ████████████████████████████████████████    │
│  [Axiom 3] ████████████████████░░░░░░░ (refined S3)    │
│  [Axiom 4]         ████████████████████████████████    │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│                    MANUAL                              │
│  Grows as RFCs reach Stage 4                           │
│  ─────────────────────────────────────────────────     │
│  [Section A] ██████████████████████████████████████    │
│  [Section B]     ██████████████████████████████████    │
│  [Section C]               ████████████████████████    │
│  [Section D]                           ████████████    │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│                    CODEBASE                            │
│  Evolves through phases                                │
│  ─────────────────────────────────────────────────     │
│  [Feature A] ██████████████████████████████████████    │
│  [Feature B]     ██████████████████████████████████    │
│  [Feature C]               ████████████████████████    │
│  [Feature D]                           ████████████    │
└────────────────────────────────────────────────────────┘

The three grow together, staying coherent:
  - Axioms constrain what we build
  - RFCs capture decisions, phases implement them
  - Manual documents what was built
  - Codebase embodies the implementation
```

---

## The Goal

When a phase starts, **the entire downstream workflow should activate**. The agent shouldn't need to remember TDD, walkthroughs, RFC links, or validation — these should be bootstrapped by the act of starting the phase.

---

## Phase Lifecycle (Fully Bootstrapped)

```
┌─────────────────────────────────────────────────────────────────────┐
│                         PHASE START                                 │
│                                                                     │
│  1. Agreement: What are we building? Which RFCs drive this?         │
│  2. Scaffold: Implementation plan with linked RFCs, acceptance      │
│     criteria, and empty walkthrough structure                       │
│  3. Branch: Create git branch for this phase                        │
│  4. Activate: TDD steering, task-by-task discipline, logging        │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         TASK LOOP                                   │
│                                                                     │
│  For each task (one at a time):                                     │
│    1. TDD Steering: "Write the test first"                          │
│    2. Implement: Make the test pass                                 │
│    3. Log: Add walkthrough entry (what was done, decisions made)    │
│    4. Complete: Mark task done, move to next                        │
│                                                                     │
│  Key: Tasks are worked sequentially, not all at once                │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         PHASE FINISH                                │
│                                                                     │
│  1. Walkthrough Review: Human reviews accumulated walkthrough       │
│  2. Feedback: Structured feedback on walkthrough (if any)           │
│  3. Validation: exohooks run (tests, lints, coherence)              │
│  4. PR: Submit/merge the phase PR                                   │
│  5. RFC Promotion: Check if linked RFCs should advance stage        │
│  6. Manual Update: If RFC→Stage 4, update Manual                    │
└─────────────────────────────────────────────────────────────────────┘
```

---

## What `exo phase start` Should Do

### 1. Agreement Phase

Before scaffolding, establish what we're doing:

```
> exo phase start "Dashboard Polish"

Which RFCs drive this phase? (comma-separated, or 'none')
> 10090, 10102

Phase goal (one sentence):
> Improve dashboard visibility and reduce cruft per user feedback

Acceptance criteria (one per line, blank to finish):
> Dashboard shows phase, current task, and next step at a glance
> Unused panes are removed or hidden
> Steering suggests relevant actions based on current state
>
```

### 2. Scaffold Implementation Plan

```toml
# Generated implementation-plan.toml

[phase]
id = "phase-dashboard-polish"
title = "Dashboard Polish"
rfcs = ["10090", "10102"]
goal = "Improve dashboard visibility and reduce cruft per user feedback"
branch = "phase/dashboard-polish"

[[acceptance_criteria]]
description = "Dashboard shows phase, current task, and next step at a glance"
status = "pending"

[[acceptance_criteria]]
description = "Unused panes are removed or hidden"
status = "pending"

[[acceptance_criteria]]
description = "Steering suggests relevant actions based on current state"
status = "pending"

[plan]
# Empty until planning step

[walkthrough]
# Populated as tasks are completed
entries = []

[verification]
automated = ["exo verify"]
manual = ["Review walkthrough with user"]
```

### 3. Create Git Branch

```bash
git checkout -b phase/dashboard-polish
```

### 4. Activate Steering

The phase-active state triggers contextual steering:

- **On task start**: "TDD: Write the test first. What behavior should this task produce?"
- **On task complete**: "Log this for the walkthrough. What did you do? Key decisions?"
- **On code edit without active task**: "No task is in-progress. Start a task first with `exo task start`"
- **On long time without commit**: "Consider committing your progress"

---

## What `exo task start <id>` Should Do

1. Mark task as `in-progress`
2. Output TDD steering:

   ```
   Task: Implement status display component

   TDD Reminder:
   1. What test would prove this task is done?
   2. Write that test (it should fail)
   3. Implement until it passes
   4. Then mark complete with `exo task complete`

   Related RFC: 10090 (section 3.2)
   ```

3. Set active task context (for steering and logging)

---

## What `exo task complete <id>` Should Do

1. Prompt for walkthrough entry:

   ```
   Task complete: Implement status display component

   Walkthrough log (what did you do? key decisions?):
   > Created StatusDisplay component that reads from exo-status output.
   > Decision: Used reactive binding rather than polling for freshness.
   > Tests: Added status-display.test.ts covering all display states.
   ```

2. Append to walkthrough in implementation-plan.toml
3. Mark task complete
4. Suggest next task or indicate "all tasks done"

---

## What `exo phase finish` Should Do

### 1. Walkthrough Review

```
Phase: Dashboard Polish
All tasks complete. Ready for walkthrough review.

Walkthrough Summary:
─────────────────────────────────────────────────────
Task 1: Implement status display component
  - Created StatusDisplay using reactive bindings
  - Decision: Reactive > polling for freshness
  - Tests: status-display.test.ts

Task 2: Remove unused panes
  - Removed: Ideas pane (rarely used), Debug pane (dev-only)
  - Kept: Status, Phase, Steering
  - Decision: Can restore via settings if needed

Task 3: Contextual steering suggestions
  - Steering now outputs "next step" based on phase state
  - Added action suggestions to steering output
─────────────────────────────────────────────────────

Review this walkthrough with user? [yes/no]
```

### 2. Feedback Integration

If user has feedback on walkthrough, capture it:

```
> exo feedback "The reactive binding approach is good, but consider
> adding a manual refresh button for edge cases"
```

### 3. Validation

```
Running validation...
  ✓ Tests pass (542 tests)
  ✓ Clippy clean
  ✓ Acceptance criteria: 3/3 met
  ✓ No uncommitted changes

Ready to submit PR.
```

### 4. PR Submission

```
Creating PR for phase: Dashboard Polish

Title: Phase: Dashboard Polish
Body: [Generated from walkthrough + acceptance criteria]

Submit PR? [yes/no]
> yes

PR #47 created: https://github.com/wycats/exo2/pull/47
```

### 5. RFC Promotion Check

```
This phase implemented RFCs: 10090, 10102

RFC 0136 is at Stage 2 (Draft).
  Phase completed successfully. Promote to Stage 3 (Candidate)? [yes/no]
  > yes

RFC 10102 is at Stage 3 (Candidate).
  Feature appears stable. Promote to Stage 4 (Stable)? [yes/no]
  > yes

  Stage 4 promotion requires Manual update.
  Sections to update: docs/manual/features/testing.md
  Update now? [yes/no]
```

---

## The Interlocking Pieces

This sketch reveals how the pieces depend on each other:

| Piece               | Depends On                | Enables                               |
| ------------------- | ------------------------- | ------------------------------------- |
| Agreement           | RFCs exist and are linked | Clear phase goal, acceptance criteria |
| TDD Steering        | Task-by-task discipline   | Tests written, quality maintained     |
| Walkthrough Logging | Task-by-task discipline   | Review artifact, feedback integration |
| Validation          | Tests exist (from TDD)    | PR quality, merge confidence          |
| PR as Phase         | Git branch per phase      | Validation attached to merge          |
| RFC Promotion       | Phase linked to RFCs      | Pipeline flow, Manual updates         |
| Manual Update       | RFC Stage 4 promotion     | Agent source of truth evolves         |

**The synergy**: Each piece makes the others work better. TDD produces tests that validation runs. Walkthrough logs create the review artifact. PR-as-phase attaches validation to git flows. RFC links drive promotion. Promotion updates the Manual.

---

## Hermeneutic Circle: What to Build First?

Given the interlocking nature, we can't just pick one. Instead, identify **vertical slices** that advance multiple fronts simultaneously.

### The Full Picture: What Needs to Exist

```
┌─────────────────────────────────────────────────────────────────────┐
│  LEVEL 1: PLANNING INFRASTRUCTURE                                   │
│  ────────────────────────────────────────────────────────────────── │
│  • Epoch planning (goals, target RFCs, phase sketches)              │
│  • Staged implementation plans for future phases                    │
│  • RFC pipeline visibility (what's at each stage, what's next)      │
│  • `exo phase prepare` for staging work before starting             │
│  • `exo plan overview` for seeing the full picture                  │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│  LEVEL 2: PHASE EXECUTION                                           │
│  ────────────────────────────────────────────────────────────────── │
│  • Phase-RFC linkage (phases know which RFCs they implement)        │
│  • Acceptance criteria (clear definition of done)                   │
│  • Phase-as-PR (git branch per phase, validation on merge)          │
│  • Task-by-task discipline (sequential, not parallel)               │
│  • TDD steering (write test first, implement, pass)                 │
│  • Walkthrough logging (narrative builds up during execution)       │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│  LEVEL 3: ACCUMULATION                                              │
│  ────────────────────────────────────────────────────────────────── │
│  • RFC promotion (phase completion advances linked RFCs)            │
│  • Manual sync (Stage 4 RFCs reflected in Manual)                   │
│  • Axiom coherence (decisions checked against core values)          │
│  • Project plan evolution (epochs complete, new ones start)         │
└─────────────────────────────────────────────────────────────────────┘
```

### Vertical Slices (Each Advances Multiple Levels)

**Critical principle**: Each slice must include **docs, tools, AND UI**. The hermeneutic circle means bringing all surfaces up to date together, so the reality of the system gets closer to the high-level concepts (which then helps us refine the high-level concepts).

```
Each Slice Includes:
┌─────────────────────────────────────────────────────────────────────┐
│  DOCS: Manual, RFCs, context files updated                          │
│  TOOLS: CLI commands, LM tools working                              │
│  UI: VSCode sidebars/webviews reflect the concepts                  │
└─────────────────────────────────────────────────────────────────────┘
```

#### Slice A: The Pipeline Skeleton

**What we build**:

- Epoch structure in plan.toml (goals, target RFCs)
- Staged implementation plans directory (`staged/`)
- `exo phase prepare` command
- `exo plan overview` command
- Phase-RFC linkage in implementation-plan.toml

**UI Component**:

- Plan overview webview showing epochs, phases, RFC pipeline
- Sidebar that shows current epoch/phase at a glance
- RFC pipeline visualization (what's at each stage)

**Why it matters**:

- Makes planning visible (Level 1)
- Establishes RFC → Phase → RFC promotion flow (Levels 1-3)
- Answers "what's next?" without digging through files

**Touches**:

- plan.toml schema (epochs gain structure)
- New staged/ directory and file format
- Phase start/finish (RFC linkage)
- New CLI commands
- **VSCode views for plan/pipeline**

#### Slice B: The Execution Muscles

**What we build**:

- `exo task start` / `exo task complete` with logging
- TDD steering on task start
- Walkthrough structure that accumulates
- Phase-as-PR (branch creation, PR submission)

**UI Component**:

- Task list showing current task, TDD status
- Walkthrough pane that shows accumulated entries
- Current phase status in status bar

**Why it matters**:

- Makes phases produce artifacts (walkthroughs, tests)
- Attaches validation to git flows (PR = validation trigger)
- Forces sequential task execution

**Touches**:

- Task commands (start, complete with logging)
- Steering (TDD reminders)
- Git integration (branch per phase)
- implementation-plan.toml (walkthrough entries)
- **VSCode views for tasks/walkthrough**

#### Slice C: The Accumulation Layer

**What we build**:

- RFC promotion checks on phase finish
- Manual sync requirements for Stage 4
- Axiom check integration (idea/RFC evaluation)
- Epoch completion flow

**UI Component**:

- RFC progress indicators (which RFCs advanced this epoch)
- Manual sync status (what needs updating)
- Epoch progress overview
- "Health" dashboard showing alignment between RFCs and Manual

**Why it matters**:

- Closes the loop (work → documented reality)
- Makes axioms active, not just documented
- Epochs become real planning units, not just labels

**Touches**:

- RFC promotion workflow
- Manual update enforcement
- Axiom consultation at key moments
- Epoch finish command
- **VSCode views for RFC/Manual sync status**

### The Hermeneutic Approach: How to Sequence

**Epoch 1: Workflow Revival** (current)

```
Phase 1: Pipeline Skeleton (Slice A - partial)
  - Staged implementation plans
  - Phase-RFC linkage
  - exo plan overview
  - UI: Basic plan overview webview

Phase 2: Execution Muscles (Slice B - partial)
  - Task start/complete with logging
  - TDD steering
  - Walkthrough accumulation
  - UI: Task list view, phase status bar

Phase 3: Accumulation Bootstrap (Slice C - partial)
  - RFC promotion on phase finish
  - Manual sync check (not enforcement yet)
  - UI: RFC stage indicators
```

**Epoch 2: Pipeline Maturity**

```
Phase 4: Full Epoch Planning
  - Epoch goals, target RFCs
  - exo epoch start/finish
  - Multi-phase staging
  - UI: Epoch overview in dashboard

Phase 5: Phase-as-PR
  - Branch creation on phase start
  - PR submission on phase finish
  - Validation glued to PR
  - UI: PR status in phase view

Phase 6: Axiom Integration
  - Axiom checks at idea capture
  - Axiom checks at RFC promotion
  - Axiom coherence in steering
  - UI: Axiom warnings/confirmations
```

**Epoch 3: Full Integration**

```
Phase 7: Manual Enforcement
  - Stage 4 blocked until Manual updated
  - Manual sections auto-detected from RFC
  - UI: Manual sync status dashboard

Phase 8: Pipeline Visualization
  - Dashboard shows RFC pipeline
  - Shows epochs, phases, progress
  - UI: Full pipeline visualization

Phase 9: Feedback Integration
  - Structured feedback on walkthroughs
  - Feedback drives refinement
  - UI: Feedback capture and display
```

### Why This Ordering?

1. **Slice A first** because you can't link phases to RFCs if you can't see the pipeline
2. **Slice B second** because execution produces the artifacts (tests, walkthrough) that Slice C consumes
3. **Slice C third** because accumulation only makes sense when there's something to accumulate
4. **Within each slice**: Partial implementation first, refinement in later epochs

This is the hermeneutic circle in action:

- Build enough of the skeleton to see the shape
- Flesh out the muscles to make it move
- Return to skeleton with new understanding
- Strengthen muscles with better structure
- Repeat

---

## Open Questions

1. **Branch strategy**: One branch per phase, or allow multiple phases per branch?
2. **PR timing**: PR on phase finish, or PR-per-task for larger phases?
3. **Walkthrough format**: Embedded in TOML, or separate projected file?
4. **Acceptance criteria**: Auto-checked by tooling, or human-verified?
5. **RFC Stage 3→4**: What counts as "stable enough" to promote?
6. **Epoch granularity**: How big should an epoch be? Days? Weeks?
7. **Staged plan detail**: How fleshed out should staged plans be?
8. **Axiom evolution**: How do we add/change axioms without destabilizing?

---

## Next Steps

- [ ] Review this sketch together
- [ ] Decide on first epoch scope (Workflow Revival)
- [ ] Stage implementation plans for first few phases
- [ ] Create/update RFCs for the work
- [ ] Start Phase 1 (use the execution to refine the model)
