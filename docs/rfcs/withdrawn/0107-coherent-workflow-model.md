<!-- exo:107 ulid:01kg5m2xzq7yap6rgg077qfat1 -->

# RFC 107: Coherent Workflow Model


# RFC 0107: Coherent Workflow Model

| Field          | Value                   |
| -------------- | ----------------------- |
| **ID**         | 10109                   |
| **Title**      | Coherent Workflow Model |
| **Stage**      | 1 (Proposal)            |
| **Status**     | In Progress             |
| **Created**    | 2026-01-23              |
| **Updated**    | 2026-01-24              |
| **Author**     | Exosuit Team            |
| **Supersedes** | —                       |
| **Related**    | RFC 0106 (Cleanup)     |

---

## Summary

This RFC defines the **target state** for Exosuit's workflow model — the coherent system we're cleaning up _toward_. It synthesizes insights from the shipping documents and establishes the minimal set of concepts that power the core workflows.

## Philosophy

These principles from the shipping documents guide the design:

1. **Anti-One-Shot**: The ecosystem optimizes for one-shot. Exosuit is explicitly anti-one-shot — every phase should feel as good as the first one-shot, across days, weeks, or months.

2. **Coherent Projections**: CLI, VS Code UI, and LM tools all project the same structured data. No "which file should I update?" confusion. TOML is source of truth; Markdown is generated for human consumption.

3. **Phase = PR**: Git flows are the carrier. Phase start creates a branch. Phase finish opens/merges the PR. This leans into existing git muscle memory.

4. **Strict Engineering as Enabler**: The more types capture error conditions with good explanations, the more the AI stays within the four corners of high-quality code. TDD steering is invoked automatically, not remembered.

5. **Sticky Artifacts**: Long AI projects typically degrade in coherence. Exosuit is designed to accumulate coherence — Axioms constrain, Manual grows, Codebase reflects.

6. **The Double Helix**: Two intertwined pipelines with different rhythms:

   **RFC Pipeline** (decisions): Ideas enter, get triaged into proposals, mature into specs, become stable truth.

   **Phase Pipeline** (execution): Work is scheduled into phases and epochs, completed as an assembly line.

   The key insight is **when** you engage with each:

   | Context            | RFC Pipeline                                | Phase Pipeline                     |
   | ------------------ | ------------------------------------------- | ---------------------------------- |
   | **During phases**  | Background. Only _add_ new ideas for later. | Foreground. Execute the work.      |
   | **Between phases** | Triage ideas, schedule work, connect RFCs.  | Review what's next in the epoch.   |
   | **Between epochs** | Reassess priorities, promote/archive RFCs.  | Plan the next strategic objective. |

   This temporal separation keeps focus sharp: during execution, you're not distracted by triage; during transitions, you're not distracted by implementation details.

## Motivation

### The Current State

The workspace has accumulated many concepts:

- Ideas, Inbox, Feedback, RFCs, Tasks, Axioms, Modes, Council, Manual, Walkthroughs, Plan, Decisions...

These concepts overlap, some are dormant, some are broken, and the connections between them are incomplete. The result: sophisticated machinery that doesn't feel coherent.

### The Insight

From the shipping documents:

> "The ideas were good, but the tooling implementation prioritized other things over parity with the original manual workflows."

The fix isn't to delete concepts, but to **map concepts to the workflows they power** and ensure the wiring is complete.

## The Coherent Model

### Workflows First

Everything exists to power these 7 workflows:

| Workflow       | When                        | What It Needs                                             |
| -------------- | --------------------------- | --------------------------------------------------------- |
| **Orient**     | Session start, context loss | Big picture, current state, pending user input            |
| **Plan**       | Phase start, new work       | Principles to guide, ideas to evaluate, decisions to make |
| **Execute**    | Active implementation       | Clear next step, quality guardrails, bounded scope        |
| **Verify**     | Work complete               | Acceptance check, narrative for review, coherence         |
| **Transition** | Phase boundary              | What we built, what's next, RFC promotions                |
| **Capture**    | User has idea mid-flow      | Quick capture that doesn't interrupt, surfaces later      |
| **Decide**     | Architectural choice        | Principle alignment, structured deliberation              |

### Three Core Artifacts

These are the **actively maintained** files that power day-to-day work:

| Artifact    | Purpose                                 | Format    | Powers                |
| ----------- | --------------------------------------- | --------- | --------------------- |
| **Plan**    | Where we are, where we're going         | TOML      | Orient, Transition    |
| **Current** | Active phase: tasks, steps, walkthrough | TOML + MD | Execute, Verify       |
| **Queue**   | Things needing attention                | TOML      | Capture, Orient, Plan |

#### Plan (`docs/agent-context/plan.toml`)

- Epochs and phases (big picture)
- Future phase sketch (what's next)
- RFC attachments (which RFCs drive which phases)

#### Current (`docs/agent-context/current/`)

- `implementation-plan.toml` — Tasks and steps (machine-readable)
- `walkthrough.md` — What we built (human-readable narrative)

#### Queue (`docs/agent-context/inbox.toml` extended)

- User intents (corrections, guidance, questions)
- Ideas awaiting triage
- Pending approvals (phase finish, RFC promotion)

### Two Reference Materials

These are **consulted during specific workflows**, not actively maintained every session:

| Material   | Purpose                             | Format   | Powers          |
| ---------- | ----------------------------------- | -------- | --------------- |
| **Axioms** | Principles that constrain decisions | TOML     | Plan, Decide    |
| **Manual** | Source of truth for how things work | Markdown | Execute, Verify |

#### Axioms (`docs/agent-context/axioms.*.toml`)

- Core principles of the project
- Consulted when evaluating ideas, reviewing RFCs, making decisions
- Scoped: workflow, system, design

#### Manual (`docs/manual/`)

- Compiled truth from Stage 4 RFCs
- "How things work" reference
- Updated when RFCs reach Stage 4

### One State Machine: Multi-Level Steering

Steering computes "what next" based on:

1. **Current Level** — Are we between phases, between epochs, or revising the roadmap?
2. **Current State** — Where are we in the phase/epoch lifecycle?
3. **Queue Items** — Filtered by what's appropriate to surface now
4. **Recent Context** — What just happened that might change the plan?

#### The Three Modes

| Mode                 | Focus              | When Active                | Surfaces                    |
| -------------------- | ------------------ | -------------------------- | --------------------------- |
| **Thinking Partner** | Why, exploration   | Planning, design reviews   | Ideas, RFCs, axiom checks   |
| **Maker**            | How, execution     | Implementation, testing    | Tasks, steps, TDD steering  |
| **Chief of Staff**   | What, organization | Transitions, status checks | Queue, coherence, approvals |

#### Mode ↔ ProgressMode Mapping

The current `steering.rs` uses `ProgressMode` with 4 values. These map to the 3 modes:

| ProgressMode (current) | WorkMode (proposed) | Phase State               |
| ---------------------- | ------------------- | ------------------------- |
| Discovery              | Thinking Partner    | No phase, or Planning     |
| Execution              | Maker               | Executing (tasks pending) |
| Verification           | Maker               | Verifying (tests red)     |
| Review                 | Chief of Staff      | Transitioning (all done)  |

#### Epoch-Level Planning

The original state machine only modeled **phase transitions**. But steering must also handle transitions **between phases within an epoch** and **between epochs**. Different situations require different guidance:

| Situation                 | What Steering Should Surface                                |
| ------------------------- | ----------------------------------------------------------- |
| **Mid-epoch, phase done** | "Continue with phase X, or pause to reflect?"               |
| **Epoch complete**        | "Review epoch accomplishments, plan next epoch"             |
| **Between epochs**        | "What's the next strategic objective? Consider the roadmap" |
| **Roadmap feels stale**   | "Recent work (RFCs, decisions) may warrant plan revision"   |
| **RFC defines new work**  | "RFC 0106 defines Cleanup Epoch — create it in plan?"      |

The key insight: **Discovery mode is not "find the next phase to start."** Discovery follows a sequence:

1. **Orient**: Where are we? What just happened? What's the current state?
2. **Reflect**: Is the current plan still right? Did we learn something that changes things?
3. **Plan**: What should come next? (Only now do we consider starting a phase)

Jumping straight to "Start phase X" skips reflection, leading to stale roadmaps.

#### The Multi-Level State Machine

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ROADMAP REVISION                                  │
│                        (Thinking Partner mode)                              │
│                                                                             │
│  When: RFCs define new epochs, major pivot, roadmap staleness detected      │
│  Surfaces: Current roadmap, recent RFCs, epoch health, future sketch        │
│  Actions: Create epoch from RFC, defer epoch, revise priorities             │
│  Exits: → BETWEEN EPOCHS (when roadmap updated)                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ [Roadmap Updated]
┌─────────────────────────────────────────────────────────────────────────────┐
│                            BETWEEN EPOCHS                                   │
│                         (Chief of Staff mode)                               │
│                                                                             │
│  When: No active epoch, or current epoch complete                           │
│  Surfaces: Epoch options, strategic objectives, RFC-defined work            │
│  Actions: Start epoch, review completed epoch, revise roadmap               │
│  Exits: → BETWEEN PHASES (epoch started), → ROADMAP REVISION (if needed)    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ [Start Epoch]
┌─────────────────────────────────────────────────────────────────────────────┐
│                            BETWEEN PHASES                                   │
│                         (Chief of Staff mode)                               │
│                                                                             │
│  When: In an epoch, but no active phase                                     │
│  Surfaces: Phase options within epoch, what's next, pending ideas           │
│  Actions: Start phase, reflect on progress, revise epoch scope              │
│  Exits: → PLANNING (phase started), → BETWEEN EPOCHS (epoch done)           │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ [Start Phase]
┌─────────────────────────────────────────────────────────────────────────────┐
│                              PLANNING                                       │
│                        (Thinking Partner mode)                              │
│                                                                             │
│  Surfaces: Axioms, RFCs, idea triage, implementation plan draft             │
│  Actions: Add tasks, define scope, get approval                             │
│  Exits: → EXECUTING (plan approved)                                         │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ [Plan Approved — human gate]
┌─────────────────────────────────────────────────────────────────────────────┐
│                              EXECUTING                                      │
│                            (Maker mode)                                     │
│                                                                             │
│  Surfaces: Current task, TDD steering, bounded context                      │
│  Actions: Implement, test, complete steps                                   │
│  Exits: → VERIFYING (tests red), → TRANSITIONING (all tasks done)           │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ [Tests Red]
┌─────────────────────────────────────────────────────────────────────────────┐
│                              VERIFYING                                      │
│                            (Maker mode)                                     │
│                                                                             │
│  Surfaces: Failing tests, fix suggestions, recent changes                   │
│  Actions: Debug, fix, re-run                                                │
│  Exits: → EXECUTING (tests green, tasks remain), → TRANSITIONING (all done) │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼ [All Green + All Tasks Done]
┌─────────────────────────────────────────────────────────────────────────────┐
│                             TRANSITIONING                                   │
│                         (Chief of Staff mode)                               │
│                                                                             │
│  Surfaces: Walkthrough, coherence check, RFC promotions                     │
│  Actions: Review work, finish phase, sketch next                            │
│  Exits: → BETWEEN PHASES (phase finished)                                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### Discovery Mode Refinement

The current `ProgressMode::Discovery` conflates multiple states. In the refined model:

| Old Discovery Behavior          | New State        | New Behavior                                       |
| ------------------------------- | ---------------- | -------------------------------------------------- |
| "No phase → start next phase"   | BETWEEN PHASES   | Orient first, then offer phase options             |
| "Epoch done → start next phase" | BETWEEN EPOCHS   | Review epoch, consider roadmap, then epoch options |
| "Stale roadmap → ignore it"     | ROADMAP REVISION | Surface RFCs that define work, offer plan updates  |

The key change: **always Orient before suggesting actions**. Steering should first explain the current situation, then offer appropriate actions — not jump to "Start phase X."

This implements the **Double Helix** principle (see Philosophy): the time between phases and epochs is when you engage with the RFC pipeline — triaging ideas, scheduling work, connecting RFCs to phases. During execution, the RFC pipeline fades to background; you can add new ideas but you're focused on the work.

### RFCs as Process, Not Pile

RFCs are a **process for making decisions**, not a permanent artifact category:

| Stage | Purpose                 | Duration                |
| ----- | ----------------------- | ----------------------- |
| 0     | Idea captured           | Until triaged           |
| 1     | Proposal for discussion | Until approved/rejected |
| 2     | Draft specification     | Until implemented       |
| 3     | Implemented, validating | Until stable            |
| 4     | Stable                  | Permanent (→ Manual)    |

When an RFC reaches Stage 4:

- Its content is **codified into the Manual**
- The RFC file becomes **historical record** (how we decided)
- The Manual is **current truth** (what actually is)

#### RFC → Plan Commands

RFCs define _what we decided_; the plan defines _when we'll do it_. These commands bridge the gap:

| Command                                                    | What It Does                                     |
| ---------------------------------------------------------- | ------------------------------------------------ |
| `exo rfc create-epoch <rfc-id> --target-stage <N>`         | Creates an epoch targeting RFC stage N promotion |
| `exo rfc create-phase <rfc-id> <epoch> --target-stage <N>` | Creates a phase targeting RFC stage N promotion  |
| `exo rfc attach <rfc-id> <target> --target-stage <N>`      | Links an RFC to phase/epoch with stage target    |
| `exo rfc detach <rfc-id> <target>`                         | Removes an RFC link from a phase or epoch        |

**The `--target-stage` parameter is required.** It answers: "When this phase/epoch completes, what RFC stage gate does it satisfy?"

Examples:

```bash
# This epoch implements RFC 0107 and gates its promotion to Stage 2
exo rfc create-epoch 10109 --target-stage 2

# This phase implements part of RFC 0107, targeting Stage 3
exo rfc create-phase 10109 wiring-epoch --target-stage 3

# Attach RFC 0106 to existing cleanup epoch, targeting Stage 3
exo rfc attach 10108 cleanup-epoch --target-stage 3
```

**Why stage targets matter:**

The plan becomes an **RFC pipeline visualization**, not just a phase schedule:

| Epoch   | Phases                        | RFC       | Target Stage |
| ------- | ----------------------------- | --------- | ------------ |
| Cleanup | Triage, Archive, Renumber     | RFC 0106 | 2→3          |
| Wiring  | Steering, Queue, Walkthrough  | RFC 0107 | 2→3          |
| Polish  | Manual, Dashboard, Fresh Eyes | RFC 0107 | 3→4          |

This surfaces the RFC pipeline in the roadmap view and answers: "How does completing this phase advance our RFCs?"

**Why RFC-centric?** The RFC is the _source of the decision_. The mental model is:

> "This RFC defines work → schedule it in the plan → completing it advances the RFC"

Not:

> "Create an epoch → hope someone remembers which RFC drove it"

When steering detects an RFC that defines unscheduled work (e.g., RFC 0106 defines a Cleanup Epoch that doesn't exist in plan.toml), it surfaces this in ROADMAP REVISION state:

> "RFC 0106 (Stage 1) defines 'Cleanup Epoch' with 6 phases. Create it? `exo rfc create-epoch 10108 --target-stage 2`"

This closes the loop between decisions and execution, making the RFC pipeline visible in the plan.

### Queue Item Lifecycle

Items in the queue have lifecycle awareness:

```rust
pub enum SurfaceWhen {
    Always,              // Immediate intents, blockers
    PhaseTransition,     // Ideas, coherence checks, approvals
    ModeMatch(Mode),     // Only when in specific mode
    ScopeMatch(Scope),   // Only when touching relevant files
}
```

This ensures: "there's an idea in the queue" surfaces during planning, not during verification.

### Git Integration: Phase = PR

Each phase is represented as a Pull Request:

- **Phase Start** → Creates a feature branch
- **Phase Work** → Commits accumulate on the branch
- **Phase Finish** → Opens/merges the PR

The walkthrough serves as PR description. This leans into existing git muscle memory and attaches validation (tests, lints) automatically via CI.

### Walkthrough: The Human-Readable Narrative

The walkthrough is a **separate artifact** (`current/walkthrough.md`), not merged into task logs:

| Aspect                   | Specification                                                    |
| ------------------------ | ---------------------------------------------------------------- |
| **Format**               | Markdown (human-readable)                                        |
| **Source**               | Authored by agent, linked to TOML tasks                          |
| **Updates**              | On task completion, step completion, agent observation           |
| **Purpose**              | User review at transition, PR description                        |
| **Relationship to TOML** | TOML tracks structure (tasks/steps); walkthrough tells the story |

The walkthrough answers: "What did we build and why?" — not just "What tasks were checked off?"

**Clarification**: The walkthrough is an **authored artifact**, not a pure projection. The agent writes narrative prose to `walkthrough.md`. This is intentional: the story of what we built is richer than a list of completed checkboxes. TOML provides structure and state; Markdown provides meaning.

### Approval Checkpoints

Human gates at phase boundaries:

| Transition               | Gate           | Mechanism                                          |
| ------------------------ | -------------- | -------------------------------------------------- |
| Planning → Executing     | Plan Approved  | User acknowledgment in chat, or `exo plan approve` |
| Transitioning → No Phase | Phase Finished | User confirmation of walkthrough review            |

The approval is a **human gate**, not an automated check. The system presents the plan/walkthrough; the human approves.

### Coherence Rituals

| Ritual              | When Invoked               | What It Does                               |
| ------------------- | -------------------------- | ------------------------------------------ |
| **Coherence Check** | Before phase transition    | Automated (exohook lane) + prompted review |
| **Fresh Eyes**      | End of epoch, user request | Mode-based review for friction points      |
| **Axiom Check**     | Idea add, RFC Stage 1→2    | Verify alignment with core principles      |

### UI Projections (Studio Pages)

CLI, LM tools, and Studio Pages all project the same underlying data. This section specifies what the UI projections look like — not just what they display, but how they **guide workflow**.

#### Design Principles

**1. Steer, Don't Just Report**

The UI should answer "what should I do next?" not just "what's happening now?" Every view includes a **Next Action** prompt based on current state and mode.

**2. Dashboards with Depth (Progressive Disclosure)**

Studio Pages are dashboards that expand into detail views. The overview shows state and next action; clicking through reveals full context. This resolves the tension between "quick glance" and "deep work."

**3. Mutations Flow Through Tools**

The UI is read-only. All changes flow through LM tools or CLI, keeping TOML as source of truth. The UI may provide "action buttons" that invoke tools, but never edits files directly.

**4. State-Aware, Not Mode-Aware**

The UI adapts to the 4 `ProgressMode` states (Discovery, Execution, Verification, Review), not the 3 conceptual modes. This provides finer-grained steering:

| ProgressMode     | Primary View            | Emphasis                                  |
| ---------------- | ----------------------- | ----------------------------------------- |
| **Discovery**    | Roadmap + Queue         | Big picture, what to plan next            |
| **Execution**    | Current Phase (Execute) | Current task, TDD stage, bounded context  |
| **Verification** | Current Phase (Verify)  | Failing tests, debugging guidance         |
| **Review**       | Walkthrough + Queue     | What we built, coherence check, approvals |

#### View Catalog

##### 1. Current Phase View

The primary workspace. Adapts its layout based on `ProgressMode`.

**Header (Always Visible)**:

| Element            | Data Source                | What It Shows                              |
| ------------------ | -------------------------- | ------------------------------------------ |
| Phase Title        | `plan.toml` active phase   | Phase name, epoch context                  |
| Git Status         | Git integration            | Branch name, PR status (draft/open/merged) |
| Progress Indicator | `implementation-plan.toml` | Task completion %, health indicator        |
| Next Action        | Steering computation       | "Write test for X" / "Review walkthrough"  |

**Execution Panel** (prominent in Execution mode):

| Section      | Data Source                | What It Shows                                |
| ------------ | -------------------------- | -------------------------------------------- |
| Current Task | Active task from impl-plan | Task title, description, acceptance criteria |
| Current Step | Active step within task    | What we're doing right now                   |
| TDD Stage    | Test runner state          | 🔴 Red / 🟢 Green / 🔄 Refactor indicator    |
| Next Test    | TDD steering               | "Write test: should handle empty input"      |
| File Context | Scope from task            | Files relevant to current work               |

**Verification Panel** (prominent in Verification mode):

| Section        | Data Source        | What It Shows                        |
| -------------- | ------------------ | ------------------------------------ |
| Failing Tests  | Test runner output | List with file:line, error message   |
| Suggested Fix  | TDD steering       | AI-generated fix hint (if available) |
| Recent Changes | Git diff           | What changed since tests passed      |
| Re-run Action  | Tool invocation    | Button to re-run tests               |
| Coverage Delta | Coverage diff      | What coverage changed this phase     |

**Plan Panel** (prominent in Discovery mode):

| Section         | Data Source                | What It Shows                       |
| --------------- | -------------------------- | ----------------------------------- |
| Plan Summary    | `implementation-plan.toml` | Narrative: what we're building, why |
| Scope           | Task list overview         | All tasks with status indicators    |
| Approval Status | Human gate state           | "Awaiting approval" / "Approved"    |
| Axiom Alignment | Linked axioms              | Which principles this phase serves  |

**Transition Panel** (prominent in Review mode):

| Section           | Data Source              | What It Shows                          |
| ----------------- | ------------------------ | -------------------------------------- |
| Walkthrough       | `current/walkthrough.md` | Full narrative (not preview)           |
| PR Preview        | Git integration          | How this will look as PR description   |
| Coherence Check   | Automated checks         | Exohook results, manual review prompts |
| Pending Approvals | Queue                    | What needs human sign-off              |

##### 2. Walkthrough View

Dedicated view for the phase narrative. Elevated from "collapsible preview" to first-class view.

| Section          | Data Source                    | What It Shows                              |
| ---------------- | ------------------------------ | ------------------------------------------ |
| Phase Story      | `current/walkthrough.md`       | Full Markdown narrative                    |
| Task Annotations | Linked from impl-plan          | Which task produced each section           |
| Timeline         | Git commits + task completions | When each part was built                   |
| PR Actions       | Git integration                | "Open PR" / "Update PR" / "View on GitHub" |

**Walkthrough Editability**: The walkthrough is an **authored artifact**, not a pure projection. The agent writes to `walkthrough.md` directly. TOML tracks tasks/steps; Markdown captures the narrative. This is intentional: the story is more than the sum of its checkboxes.

##### 3. Roadmap View

Big-picture planning and orientation. The roadmap is a **dual view** that projects the same data through two lenses:

**Phase Pipeline** (execution-focused): What are we doing next?
**RFC Pipeline** (decision-focused): How are our decisions progressing?

These are different coordinating devices that connect together:

| Aspect          | Phases                                      | RFCs                            |
| --------------- | ------------------------------------------- | ------------------------------- |
| **About**       | Execution (bounded work)                    | Decisions (what and why)        |
| **Duration**    | Days to weeks                               | Weeks to months                 |
| **Output**      | Code, tests, artifacts                      | Specifications, rationale       |
| **Stages**      | Planning → Executing → Done                 | Idea → Proposal → Spec → Stable |
| **Git**         | Branch + PR                                 | Markdown file in rfcs/          |
| **Cardinality** | Many phases per RFC, or many RFCs per phase | N:M relationship                |

The `--target-stage` parameter bridges them: "When this phase completes, RFC X can advance to Stage Y."

**Phase Pipeline Section**:

| Element         | Data Source                | What It Shows                            |
| --------------- | -------------------------- | ---------------------------------------- |
| Epoch Timeline  | `plan.toml` epochs         | Visual timeline, current position marked |
| Phase Cards     | `plan.toml` phases         | Title, status, target stage, health      |
| RFC Attachments | Phase ↔ RFC links          | Which RFCs this phase advances           |
| Future Sketch   | `plan.toml` future section | Rough ideas for what's next (editable)   |
| Next Action     | Steering                   | "Start phase X" / "Plan next epoch"      |

**RFC Pipeline Section**:

| Stage   | What It Shows                                                  |
| ------- | -------------------------------------------------------------- |
| Stage 0 | Ideas captured, awaiting triage                                |
| Stage 1 | Proposals under discussion (linked phases show how to advance) |
| Stage 2 | Draft specs being implemented (linked phases in progress)      |
| Stage 3 | Implemented, validating (awaiting stability confirmation)      |
| Stage 4 | Stable → links to Manual sections where content is codified    |
| Blocked | RFCs waiting on dependencies, decisions, or unscheduled phases |

**RFC Card Detail** (drill-down):

| Field         | What It Shows                                            |
| ------------- | -------------------------------------------------------- |
| RFC Title     | What the decision is about                               |
| Current Stage | Where it is in the pipeline                              |
| Linked Phases | Which phases implement this RFC (with target stages)     |
| Next Gate     | What's needed to advance (e.g., "Complete Wiring Epoch") |
| Manual Link   | For Stage 4: where it's codified in the Manual           |
| Dependencies  | Other RFCs this one depends on                           |

This dual view answers two questions at once:

- "What work is coming up?" → Phase Pipeline
- "What decisions are we making progress on?" → RFC Pipeline

##### 4. Queue View

Unified attention manager. Everything needing human attention, filtered by relevance.

| Section           | Data Source               | What It Shows                                  |
| ----------------- | ------------------------- | ---------------------------------------------- |
| Immediate         | `SurfaceWhen::Always`     | Blockers, urgent corrections, errors           |
| Current Mode      | `SurfaceWhen::ModeMatch`  | Items relevant to current ProgressMode         |
| Pending Approvals | Human gates               | Plan approvals, phase finishes, RFC promotions |
| Ideas             | Ideas awaiting triage     | Title, tags, axiom alignment indicator         |
| Scope-Triggered   | `SurfaceWhen::ScopeMatch` | Items relevant to files being touched          |

**Queue Item Detail** (drill-down):

| Field            | What It Shows                                      |
| ---------------- | -------------------------------------------------- |
| Title            | What needs attention                               |
| Source           | User intent / captured idea / system alert         |
| Axiom Alignment  | Which axioms this serves or conflicts with         |
| Suggested Action | "Triage to RFC" / "Add to current phase" / "Defer" |
| Age              | How long it's been waiting                         |

##### 5. Axioms View

Active reference, not passive listing. Shows axioms in context of current work.

| Section         | Data Source                    | What It Shows                          |
| --------------- | ------------------------------ | -------------------------------------- |
| Active Axioms   | Axioms linked to current phase | Principles guiding this work           |
| Alignment Check | Ideas/RFCs vs axioms           | What serves each axiom, what conflicts |
| Workflow Axioms | `axioms.workflow.toml`         | How we work (process principles)       |
| System Axioms   | `axioms.system.toml`           | Technical constraints                  |
| Design Axioms   | `axioms.design.toml`           | Design principles                      |

**Axiom Detail** (drill-down):

| Field      | What It Shows                               |
| ---------- | ------------------------------------------- |
| Axiom Text | The principle itself                        |
| Serves     | RFCs, phases, ideas that align              |
| Tensions   | Work that may conflict (flagged for review) |
| History    | When added, why, related decisions          |

##### 6. TDD Steering Panel

Embedded in Current Phase View but deserves specification. The quality-without-memory differentiator.

| Element        | Data Source                | What It Shows                                               |
| -------------- | -------------------------- | ----------------------------------------------------------- |
| Current Stage  | Test runner + heuristics   | 🔴 Red (write code) / 🟢 Green (refactor?) / 🔄 Refactoring |
| Stage Guidance | TDD steering logic         | "Make this test pass" / "Consider extracting X"             |
| Next Test      | Coverage gaps + task scope | Suggested next test to write                                |
| Coverage Trend | Coverage history           | Sparkline showing coverage over phase                       |
| Quality Gates  | Exohook checks             | Lint, type check, coverage threshold                        |

#### View ↔ State Matrix

Which views are primary/secondary in each state (using the refined multi-level states):

| View              | Roadmap Revision | Between Epochs | Between Phases | Planning | Executing | Verifying | Transitioning |
| ----------------- | ---------------- | -------------- | -------------- | -------- | --------- | --------- | ------------- |
| **Current Phase** | ○                | ○              | ○              | ●        | ●         | ●         | ●             |
| **Walkthrough**   | ○                | ○              | ○              | ○        | ○         | ○         | ●             |
| **Roadmap**       | ●                | ●              | ●              | ○        | ○         | ○         | ○             |
| **Queue**         | ●                | ●              | ●              | ●        | ○         | ○         | ●             |
| **Axioms**        | ●                | ○              | ○              | ●        | ○         | ○         | ○             |
| **TDD Panel**     | ○                | ○              | ○              | ○        | ●         | ●         | ○             |

● = Primary (auto-opened, prominent)  
○ = Secondary (available, collapsed or linked)

**Note**: The old 4-value `ProgressMode` (Discovery, Execution, Verification, Review) maps to these states:

- Discovery → Roadmap Revision, Between Epochs, Between Phases, or Planning (context-dependent)
- Execution → Executing
- Verification → Verifying
- Review → Transitioning

#### View ↔ Workflow Matrix

Which views power which workflows:

| View              | Orient | Plan | Execute | Verify | Transition | Capture | Decide |
| ----------------- | ------ | ---- | ------- | ------ | ---------- | ------- | ------ |
| **Current Phase** | ●      | ●    | ●       | ●      | ○          | ○       | ○      |
| **Walkthrough**   | ○      | ○    | ○       | ○      | ●          | ○       | ○      |
| **Roadmap**       | ●      | ●    | ○       | ○      | ●          | ○       | ●      |
| **Queue**         | ●      | ●    | ○       | ○      | ●          | ●       | ●      |
| **Axioms**        | ○      | ●    | ○       | ○      | ○          | ●       | ●      |
| **TDD Panel**     | ○      | ○    | ●       | ●      | ○          | ○       | ○      |

#### Git Integration

"Phase = PR" requires visible git state throughout:

| Element          | Where Shown          | What It Shows                            |
| ---------------- | -------------------- | ---------------------------------------- |
| Branch Indicator | Current Phase header | `feat/phase-name` with copy button       |
| PR Status        | Current Phase header | Draft / Open / Merged / None             |
| Commit Count     | Current Phase header | "12 commits this phase"                  |
| PR Link          | Walkthrough view     | Direct link to PR on hosting platform    |
| Diff Preview     | Verification panel   | Changes since last green                 |
| PR Actions       | Transition panel     | "Open PR" / "Update description" buttons |

#### Steering Integration

Every view includes steering guidance. The guidance changes based on state:

| State                | Steering Shows                                                             |
| -------------------- | -------------------------------------------------------------------------- |
| **Roadmap Revision** | "RFC 0106 defines Cleanup Epoch. Create it? Review roadmap priorities?"   |
| **Between Epochs**   | "Epoch X complete. Review accomplishments? Start next epoch? Revise plan?" |
| **Between Phases**   | "Phase Y done. Continue with phase Z? Pause to reflect? Update epoch?"     |
| **Planning**         | "Define tasks. Get plan approval when ready."                              |
| **Executing**        | "Current task: [X]. TDD: [Red/Green/Refactor]. Next: [specific action]"    |
| **Verifying**        | "Fix: [specific test] at [file:line]. Recent changes: [diff summary]"      |
| **Transitioning**    | "Review walkthrough. Finish phase? Any RFC promotions pending?"            |

**Key principle**: Steering always **orients first**, then suggests actions. Compare:

| ❌ Old (jump to action)           | ✅ New (orient then act)                                                                                        |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| "Start phase map-phase-8-landing" | "No active phase. Last: queue-prototype. Roadmap may need update (RFC 0106 defines new work). Options: [list]" |
| "No active phase detected"        | "Between phases in Epoch X. 3 of 5 phases complete. Next scheduled: Y. Or: reflect, revise scope"               |

## What This Replaces

| Current              | In Coherent Model             | Change                                 |
| -------------------- | ----------------------------- | -------------------------------------- |
| ideas.toml           | Queue (ideas awaiting triage) | Merge into unified queue               |
| inbox.toml           | Queue (intents)               | Extend with more categories            |
| feedback.toml        | Queue                         | Remove (broken, superseded)            |
| tasks in impl-plan   | Current (tasks/steps)         | Keep, clarify purpose                  |
| walkthrough (merged) | Current (walkthrough.md)      | Restore as separate artifact           |
| axioms.\*.toml       | Axioms (reference)            | Keep, wire into workflows              |
| modes.toml           | Steering (mode awareness)     | Merge into steering logic              |
| council.toml         | —                             | Archive (not workflow-critical)        |
| decisions.toml       | —                             | Archive (useful content → Manual/RFCs) |
| prompts.toml         | —                             | Evaluate necessity                     |

## Implementation Path

This RFC defines the target. RFC 0106 defines the cleanup process to get there.

### Implementation Progress

> **Status as of 2026-01-24**: Cleanup Epoch complete. Ready for Wiring Epoch.

#### Completed

- ✅ **Multi-Level State Machine** (PR #52, merged 2026-01-24)
  - 7-state machine implemented in `steering.rs`: ROADMAP_REVISION, BETWEEN_EPOCHS, BETWEEN_PHASES, PLANNING, EXECUTING, VERIFYING, TRANSITIONING
  - Epoch boundary detection in `WorldState`
  - Orient-first steering behavior (explains situation before suggesting actions)
  - `exo-status` LM tool outputs new state names

- ✅ **Cleanup Epoch** (RFC 0106)
  - Multi-level steering phase completed
  - Epoch marked complete in plan.toml

#### Remaining Work

**Stage 2 Gate** (next milestone):

- [x] User approves direction for Wiring Epoch
- [x] Wiring Epoch created in plan.toml

### Wiring Epoch Phases

The Wiring Epoch implements the coherent model defined in this RFC. Each phase maps to a major system component:

| Phase                                 | ID                         | Work Items                                                                                            |
| ------------------------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Phase 1: Double Helix Integration** | `double-helix-integration` | RFC → Plan commands (`exo rfc create-epoch`, `exo rfc attach`, etc.), temporal separation enforcement |
| **Phase 2: Axiom Wiring**             | `axiom-wiring`             | Wire axioms into idea/RFC evaluation, principle-based steering                                        |
| **Phase 3: Mode-Aware Behavior**      | `mode-wiring`              | Thinking Partner / Maker / Chief of Staff mode switching, mode-based tool selection                   |
| **Phase 4: Coherent Projections**     | `coherent-projections`     | Queue `SurfaceWhen` logic, Studio views (3+ of 6 functional)                                          |
| **Phase 5: TDD Steering Integration** | `tdd-steering`             | Automatic TDD invocation, test-aware steering                                                         |

**Note**: Walkthrough restoration is in RFC 0106 Phase 5 (cleanup), not here.

**Wiring Epoch Gate** (implements Stage 2→3):

- [ ] All 5 phases complete
- [ ] Multi-level steering validated in real use
- [ ] At least 3 of 6 UI views functional

**Polish Epoch** (implements Stage 3→4):

- [ ] Queue unification complete
- [ ] Update Manual with workflow documentation
- [ ] Fresh Eyes review
- [ ] One full epoch completed using the coherent model

### RFC Stage Progression

| Stage | Gate                                                   | What Changes                              |
| ----- | ------------------------------------------------------ | ----------------------------------------- |
| 1→2   | User approves direction, cleanup epoch started         | Lock scope, begin detailed implementation |
| 2→3   | Multi-level steering implemented, UI projections built | Validation in real use                    |
| 3→4   | One full epoch completed using the coherent model      | Codify into Manual                        |

**Stage 2 Criteria** (Draft Specification):

- RFC 0106 cleanup epoch created in plan.toml
- First cleanup phase started
- No major design objections from user

**Stage 3 Criteria** (Implemented):

- Multi-level state machine in steering.rs
- RFC → Plan commands implemented
- At least 3 of 6 UI views functional
- Queue unification complete

**Stage 4 Criteria** (Stable):

- Full epoch completed using this workflow model
- Fresh Eyes review confirms coherence
- Manual updated with workflow documentation

### Work Epochs

1. **Cleanup Epoch**: Execute RFC 0106
   - Triage RFCs
   - Archive dormant files
   - Clean orphaned directories
   - Renumber RFCs

2. **Wiring Epoch**: Connect the coherent model
   - Wire axioms into idea/RFC evaluation
   - Make steering mode-aware
   - Extend queue with SurfaceWhen logic
   - Restore walkthrough artifact

3. **Polish Epoch**: Refine and document
   - Update Manual with Stage 4 RFC content
   - Dashboard reflects coherent model
   - Fresh Eyes review for coherence

## Success Criteria

The coherent model is achieved when:

1. **Workflows feel natural**: Each of the 7 workflows has clear tooling support
2. **Artifacts are minimal**: Only 3 active artifacts (Plan, Current, Queue)
3. **References are consulted**: Axioms and Manual are checked at appropriate moments
4. **Steering is mode-aware**: Different behavior in Planning vs Executing vs Transitioning
5. **Queue is unified**: All attention-needing items in one place with lifecycle awareness
6. **RFCs flow to Manual**: Stage 4 means documented in Manual
7. **Phase = PR**: Git integration feels natural, not forced

## Dependencies

This RFC builds on:

| RFC                             | What It Provides                                 |
| ------------------------------- | ------------------------------------------------ |
| RFC 0050 (Async Intent Channel) | Inbox design, intent categories, surfacing logic |
| RFC 0064 (Phase State Machine) | Phase lifecycle states and transitions           |
| RFC 0107 (Plan Health)         | Health-based steering, confidence adjustment     |
| RFC 0106 (Cleanup)             | Process to get from current state to this target |

## References

- [Workflow Disconnects](docs/brainstorming/2026-01-23-workflow-revamp/shipping/workflow-disconnects.md)
- [Key Differentiators](docs/brainstorming/2026-01-23-workflow-revamp/shipping/key-differentiators.md)
- [Repo History Analysis](docs/brainstorming/2026-01-23-workflow-revamp/shipping/repo-history-analysis.md)
- [Vision: The Exosuit Philosophy](docs/vision.md)
