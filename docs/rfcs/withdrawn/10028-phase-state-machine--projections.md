<!-- exo:10028 ulid:01kmzxeffwzt9nmw26vz176x8w -->


# RFC 10028: Phase State Machine & Projections

## Summary

This RFC defines the canonical workflow model for Exosuit projects as a **phase-centric state machine** with explicit, deterministic steering for “where are we at?”. It also defines the **canonical vs projection** boundary: projection artifacts (notably `task-list.toml` and `walkthrough.toml`) are workflow-inconsistent and must trigger an **upgrade gate**.

The goal is to make the “main flow” inevitable:

- If you are in an active phase, focus on executing that phase’s lifecycle.
- If you finished a phase, focus on preparing the next phase and starting it.
- If you finished an epoch, focus on preparing the next epoch.
- Tangents are handled via a single-depth **surgical strike** overlay that cannot derail the phase lifecycle.

## Motivation

Today, the user (and the agent) frequently asks “where are we at?” and ends up thrashing:

- trying multiple commands to infer state,
- consulting projection files as if they were canonical,
- accidentally mutating the active phase when intending to schedule future work,
- and encountering mismatches caused by multiple sources-of-truth.

This RFC codifies a deterministic, machine-readable state model so tooling can answer “where are we at?” as a pure function of repo state.

## Core Axioms (Normative)

### Axiom A: Workflow locus

Most action lives inside the lifecycle of a phase.

- If a phase is active, the primary intent is **execute** (advance the active phase).
- If no phase is active, the primary intent is **prepare** (ready the next phase/epoch).

### Axiom B: Holistic steering (steering-by-shape)

Steering is multi-faceted:

- messages and next-call suggestions,
- **command parameters** that encode intent/confirmation,
- and schema constraints that prevent ambiguous or workflow-inconsistent actions.

Example: starting a phase is an approval event (below), so the `phase start` operation should carry an explicit approval assertion.

### Axiom C: Canonical vs projection

There is a small set of canonical “law” artifacts; everything else is a projection.

- Canonical artifacts drive decision-making.
- Projection artifacts must not be consulted for decision-making.

### Axiom D: Projections are inconsistency alarms

The existence of deprecated projection files means the project is in a workflow-inconsistent state and must be addressed immediately.

If a deprecated projection exists, the system must enter **NeedsUpgrade** and refuse normal execution mutations until upgrade completes.

## Glossary

- **Phase**: a unit of work with an explicit lifecycle.
- **Epoch**: an ordered collection of phases.
- **Active phase**: the phase currently being executed.
- **Execution artifacts**: canonical state updated during a phase.
- **Scheduling**: modifying a future phase without touching active-phase execution artifacts.
- **Acceptance criteria**: explicit, in-phase definition-of-done constraints.
- **Projection**: a rendered/materialized view for a UI (not canonical truth).
- **Surgical strike**: a single-depth tangent mode that temporarily supersedes the phase locus.

## Canonical Artifacts (Target Model)

This RFC does not fully enumerate “the law” set, but it establishes the key boundary:

- The canonical phase execution state is stored in the **phase execution artifact** (currently `docs/agent-context/current/implementation-plan.toml`).

Open question: rename `implementation-plan.toml` → `phase.toml`.

### Deprecated projection artifacts (must be removed)

The following are projections and must not be treated as canonical truth:

- `docs/agent-context/current/task-list.toml`
- `docs/agent-context/current/walkthrough.toml`

When present, they indicate an inconsistent workflow state.

## Approval Semantics

Starting a phase is an explicit approval event.

- A phase is considered **approved by definition once it is started**.
- Therefore, `phase start` must be understood as: “the user approved this phase enough to begin”.

In the near term, an AI asserting approval is sufficient. In the long term, a UI may capture explicit user approval.

## The Phase State Machine (Normative)

### State variables (observable)

The resolver must determine state from canonical artifacts and repo status:

- `active_phase_id?`
- `active_epoch_id?`
- `active_strike_id?` (single-depth)
- `needs_upgrade?` (presence of deprecated projection files)
- `phase_prepared?` (placeholder vs meaningful execution plan)
- `phase_gates_satisfied?` (acceptance criteria + verification requirements)

### Primary states

The system has one primary locus state at a time:

1. **NoActivePhase**
2. **ActivePhase:NeedsUpgrade**
3. **ActivePhase:Unprepared**
4. **ActivePhase:Executing**
5. **ActivePhase:ReadyToFinish**
6. **PreparingNextPhase**
7. **PreparingNextEpoch**

### Surgical strike overlay (single-depth)

Orthogonal to the primary locus, there may be at most one active strike.

- If `active_strike_id?` is set, the primary intent becomes **execute strike**.
- No nested strikes: starting a strike while one is active is forbidden.

### Deterministic resolver: “Where are we at?”

Given repo state, resolve the primary state and yield 1–3 next actions.

Pseudo-ordering:

1. If `active_strike_id?` → **StrikeActive** (overlay) and steer to strike completion.
2. Else if `needs_upgrade?` and there is an active phase → **ActivePhase:NeedsUpgrade**.
3. Else if `active_phase_id?` and not `phase_prepared?` → **ActivePhase:Unprepared**.
4. Else if `active_phase_id?` and not `phase_gates_satisfied?` → **ActivePhase:Executing**.
5. Else if `active_phase_id?` and `phase_gates_satisfied?` → **ActivePhase:ReadyToFinish**.
6. Else if next phase exists but underspecified → **PreparingNextPhase**.
7. Else → **PreparingNextEpoch**.

The resolver must not “thrash” by trying multiple commands; it must use canonical state.

## Allowed Operations by State

### ActivePhase:NeedsUpgrade (hard gate)

The project is workflow-inconsistent. The only allowed mutations are those that complete the upgrade.

Allowed:

- status/orientation commands (e.g. “where are we at?”)
- upgrade commands (migration + completion)

Forbidden:

- normal phase execution mutations
- plan mutations unrelated to upgrade

Every allowed command in this mode must include steering pressure:

> You are mid-upgrade; complete upgrade before proceeding.

### ActivePhase:Unprepared

Allowed:

- define execution plan for this phase (steps/changes/tasks)
- add acceptance criteria
- in-phase backlog: “sometime during this phase” items
- begin a surgical strike (if needed)

Forbidden:

- finishing the phase

### ActivePhase:Executing

Allowed:

- execute work, update execution plan statuses
- add acceptance criteria (tighten definition-of-done)
- add in-phase backlog items (“sometime this phase”)
- schedule future phase work (plan-only mutation)
- start/finish a surgical strike

### ActivePhase:ReadyToFinish

Allowed:

- finish phase
- schedule future phase work
- (optionally) start a surgical strike, but only if it is explicitly justified (avoid derailing)

### PreparingNextPhase

Allowed:

- mutate the next phase plan with details (plan-only)
- start the next phase (approval-by-start)

### PreparingNextEpoch

Allowed:

- define the next epoch and its phases
- transition into PreparingNextPhase for the first phase

## Scheduling vs In-Phase Backlog vs Acceptance Criteria

To avoid confusion, “add work” must be classified:

1. **Schedule future phase work**: plan-only mutation targeting a future phase.
2. **In-phase backlog** (“do this sometime in this phase”): add to the current phase’s scope without disrupting execution artifacts.
3. **Acceptance criteria**: add or refine definition-of-done constraints that affect readiness to finish.

The resolver/steering must not treat in-phase backlog as “scheduling”.

## Upgrade Mode (Projection Removal)

### Trigger

If any deprecated projection artifact exists, the project enters NeedsUpgrade.

### Upgrade intent

Upgrade means:

- migrate projection contents into canonical artifacts,
- remove the deprecated projection files,
- and perform any finalization step needed to ensure the repo is consistent.

### Completion operation (abstract)

There must exist an upgrade completion operation (name TBD).

- Option: `exo upgrade finish-upgrade`
- This RFC leaves naming as an open question for Stage 1.

### Steering contract

While NeedsUpgrade:

- all commands must include steering that points to the next upgrade step,
- and non-upgrade mutations must be refused.

## Migration Retcon: Tripwires + Policies (Normative)

### Goal

When migrating from older Exosuit setups, the system should not “convert files”; it should **replay history using modern `exo` commands** to produce a canonical, linearized retcon that satisfies the phase state machine.

Legacy artifacts may be copied temporarily for reference during migration, but are not intended to persist as part of the new steady state.

### Migration invariants

The migration retcon MUST satisfy:

- Epochs are strictly ordered and linear.
- Epoch/phase IDs (numbers) are allocated by modern `exo`.
- There is exactly one **current** phase.
- All **pending** phases are in the future (after the current phase).

### Decision policy

Migration decisions are classified into three buckets:

1. **Autopilot (agent decides, always reports)**
   - mechanical normalization (schema drift, formatting, renumbering via `exo`)
   - moving all pending phases into the future (no pending interleaved before current)
   - ordering when there is a dominant, consistent signal

2. **Soft stop (confirm once, then apply globally)**
   - choose a global ordering heuristic (e.g. timestamp-first vs ID-first)
   - choose a policy for missing verification evidence on “completed” phases
   - choose a policy for orphan/unattributed events (attach vs preserve as unmapped)

3. **Hard stop (must ask the user)**
   - any ambiguity that changes phase/epoch meaning, ordering, or done-ness

### Hard-stop tripwires

The migrator MUST pause for user confirmation when any of the following occur:

- Multiple plausible current-phase candidates.
- Ambiguous epoch boundary (reassigning phases across epochs).
- Irreconcilable phase ordering conflict (different signals imply different orders).
- “Pending but actually happened” (strong evidence a pending phase was executed/completed).
- Discarding meaningful events due to lack of mapping (unknown task/phase/RFC references).
- Any operation that would flip done-ness semantics without explicit approval (pending→completed, completed→pending).

### Confidence gate

The migrator SHOULD compute a confidence score for each inference and MUST hard-stop when below a threshold.

Minimum recommended hard-stop conditions:

- current phase candidate set size > 1
- ordering confidence < 0.7
- any done-ness flip would occur

### Stop prompt contract

When a hard-stop occurs, tooling MUST present (and then wait):

- **Decision needed** (1 sentence)
- **Options** (2–3, with a default)
- **Evidence** (3 concrete bullets)
- **Consequence** (how the retcon will change the state machine: current phase, pending-in-future, etc.)

## Surgical Strikes (Single-Depth Overlay)

Surgical strikes are first-class tangents that do not derail phases.

Rules:

- Only one active strike at a time.
- Finishing a strike must produce a canonical “walkthrough/log” record inside the phase execution artifact (since `walkthrough.toml` is removed).

Open question: the exact canonical schema for the strike-completion log entry.

## Renderer Obligations (CLI + VS Code)

Renderers must implement the same resolver and the same canonical-vs-projection rules.

- Renderers must not consult deprecated projection files to infer state.
- “Where are we at?” must return the primary state and next actions.
- Scheduling operations must target future phases without touching active execution artifacts.

## Bug Report Mapping (Informative)

This RFC consolidates and resolves the failure modes described in:

- `docs/bug-reports/task-completion-mismatch.md` (multiple sources of truth)
- `docs/bug-reports/task-inconsistency.md` (renderers disagree on task listing)
- `docs/bug-reports/phase-issues.md` and `docs/bug-reports/phase-transition.md` (phase modeling + safe transitions)
- `docs/bug-reports/mass-confusion.md` (discoverability + steering)

## Open Questions (Stage 1)

1. Rename `implementation-plan.toml` → `phase.toml`?
2. Define the canonical schema for acceptance criteria and in-phase backlog.
3. Define the canonical schema for strike completion logs (replacing walkthrough).
4. Name and exact UX of the upgrade completion operation.
5. Explicit capability discovery endpoints (for tools) and how they relate to the state machine.

