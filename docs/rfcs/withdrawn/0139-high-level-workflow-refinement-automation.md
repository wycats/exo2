<!-- exo:139 ulid:01kg5kp2hwx2zaygdmbjaqwv5e -->

# RFC 139: High-Level Workflow Refinement & Automation

- **Status**: Withdrawn
- **Stage**: 2
- **Reason**: Withdrawn by RFC 10180 storage disposition: file-backed phase context, docs/agent-context/current artifacts, and docs/agent-context/archive phase snapshots are retired.

# RFC 0139: High-Level Workflow Refinement & Automation

## Summary

This RFC proposes a comprehensive overhaul of the high-level agent workflows (Phase Lifecycle, Planning, Coherence) to improve reliability, reduce "prompt friction," and better integrate with the UI. It specifically targets the ambiguity between "starting a new chat" vs. "continuing in the current chat" and proposes moving complex logic from manual LLM execution into robust Rust-based tooling.

## Motivation

The current agent workflows (`phase-start`, `phase-transition`, etc.) rely heavily on the LLM manually following a list of steps in a prompt. This leads to several critical issues:

1.  **Fragility**: The LLM might skip a step, hallucinate a file path, or fail to update a specific documentation file.
2.  **Context Ambiguity**: The transition between phases often leaves the agent in an undefined state. It's unclear if the agent should clear its context for a new chat or retain it.
3.  **Manual Labor**: The LLM is forced to perform "shell script" logic (reading files, extracting lines, running commands) which is error-prone and wasteful of tokens.
4.  **Lack of Rigor**: "Coherence checks" and "Axiom checks" are currently aspirational instructions rather than enforced processes.
5.  **Governance Drift ("YOLO Mode")**: Without strict tooling, agents may unilaterally promote RFCs or skip approval steps. The workflow must enforce that stage transitions are _checkpoints_ requiring explicit human sign-off.

### Strategic Alignment (RFC 012)

This RFC serves as the **tactical implementation** of the workflow vision outlined in **RFC 012 (The Grand Unification)**. While RFC 012 sets the strategic roadmap (Backfill, Manual Sync, Legacy Purge), RFC 0030 defines the _mechanisms_ and _state machines_ required to execute that roadmap reliably.

Specifically, we need to shift from "LLM as a manual operator" to "LLM as a conductor of robust tools" to enable:

1.  **The Great Backfill (RFC 012, Phase 29)**: We need robust "Zero to One" verification to ensure backfilled RFCs match reality.
2.  **The Manual Sync (RFC 012, Phase 30)**: We need automated "Coherence Checks" to ensure the Manual stays in sync with Stage 3+ RFCs.

## User Journeys

This section illustrates how the user interacts with the system to trigger workflows, what happens behind the scenes, and how the UI reflects these states.

### Scenario 1: Starting a New Phase (The "Go" Button)

- **Pre-condition**: The phase has already been _prepared_ (RFCs exist, `implementation-plan.toml` is ready).
- **User**: "Let's start Phase 2." (or clicks "Start Phase" in UI).
- **LLM**: Calls `exo phase start 2`.
- **Tool (`exo`)**:
  - Validates that Phase 2 is in a "Ready" state (plan exists, RFCs are staged).
  - _Error Path_: If not ready, returns: "Phase 2 is not prepared. Run `exo phase prepare 2` first."
  - _Success Path_: Sets Phase 2 as "Active" in `plan.toml`. Hydrates the context.
- **UI**:
  - Sidebar updates to "Phase 2: Active".
  - **RFC List**: Shows active RFCs and their target transitions (e.g., "RFC 0030: Stage 0 -> 1").
  - **Chat**: "Phase 2 Started. Context loaded."

### Scenario 2: The Daily Standup (Phase Status)

- **User**: "Where are we?" or "Status".
- **LLM**: Calls `exo phase status`.
- **UI**: Renders a detailed summary table:
  - **Active RFCs**: Current Stage -> Target Stage.
  - **Tasks**: Completed / Total (from `task-list.md`).
  - **Drift Warning**: If `implementation-plan.toml` diverges from `task-list.md`.
- **Sidebar**: Highlights the current active task.

### Scenario 3: Phase Completion & Handoff

- **User**: "We're done. Wrap it up." (or clicks "Finish Phase").
- **LLM**: Calls `exo phase finish`.
- **Tool (`exo`)**:
  - **Validation**: Checks all tasks in `task-list.md` are marked complete.
  - **Mini-Coherence**: Runs a targeted check (lints, broken links, uncommitted changes).
  - **Changelog**: Prompts the user/agent to update the `changelog` field in the headers of modified RFCs.
  - **Commit**: Updates `plan.toml` (Phase 2 -> Completed).
- **Output**: "Phase 2 Complete. Ready to prepare Phase 3?"
- **User**: "Yes, prepare Phase 3."
- **LLM**: Calls `exo phase prepare 3`.
  - Scaffolds `docs/agent-context/phase-3/`.
  - Helps user select/create RFCs for the new phase.
  - Generates `implementation-plan.toml`.

### Scenario 4: Sidebar Reflection

- The sidebar should always reflect the _persisted_ state (`plan.toml`), not just the chat state.
- When `exo phase finish` commits, the sidebar updates to show the phase as "Completed" and the next as "Pending" (or "Active" if immediately started).

## Detailed Design

### 1. Workflow Definitions

We will formalize the following high-level workflows:

#### A. New Axiom: Steering-First Tooling (Axiom 10)

We propose adding the following to `docs/manual/architecture/axioms.md`:

> **10. Steering-First Tooling**
>
> **Principle**: Tools are not passive utilities; they are active participants that steer the agent.
> **Why**: Agents operate in an "infinite context" and easily lose the thread. They need clear, immediate, and context-aware instructions on "what to do next" to stay on the Happy Path.
> **Implication**:
>
> - **Chatty Interfaces**: Every CLI command output must include a "Next Step" or "Steering Instruction" (even in JSON mode).
> - **State Awareness**: Tools must know the global state (Phase, Task) and guide the agent accordingly.
> - **Error Recovery**: Error messages must not just say "Failed"; they must say "Failed. Try X or Y."

#### B. Axiom and Manual Check

- **Goal**: Ensure new plans and implementations align with `AGENTS.md` and `docs/manual/`.
- **Process**:
  - Before finalizing a `plan-outline.md` or `implementation-plan.md`, the agent must run a specific tool (e.g., `exo check axioms --plan <file>`) that performs a semantic search or heuristic check against the axioms.
  - **Automation**: The tool could extract key assertions from the plan and cross-reference them with the axioms.

#### B. Zero to One Workflow (The "Grand Simulation")

- **Goal**: Validate that a user can go from "Zero" (no code) to "One" (a working feature/system) by following the documentation _exactly_.
- **Concept**: This is a specialized **Fresh Eyes** mode (Persona: "The New User").
- **Process**:
  1.  **Persona Selection**: The agent adopts a specific persona (e.g., "The New User", "The Skeptic", "The Power User").
  2.  **Documentation-Driven Execution**: The agent reads the _actual_ documentation (e.g., `docs/manual/features/new-feature.md`) and executes the steps _verbatim_.
  3.  **Friction Logging**: Any deviation between the docs and reality (e.g., "Command not found", "Output differs") is logged as a high-priority issue.
  4.  **Outcome**: A "Friction Log" and a verified "Zero to One" path.
- **Automation**: A prompt template (like the provided example) that instructs the agent to create a sandbox environment and strictly follow the guide.

#### C. Coherence Checks

We distinguish between three levels of coherence checking to avoid "check fatigue" while maintaining rigor.

1.  **Continuous Coherence (The "Drift" Check)**

    - **Goal**: Ensure `docs/` reflects the reality of `src/`.
    - **Trigger**: Runs on `exo phase status` or periodically.
    - **Checks**:
      - **Drift**: Code changed but docs/RFCs are stale (via git timestamps).
      - **Task Alignment**: `task-list.md` matches `implementation-plan.toml`.
      - **Manual Sync (RFC 012)**: Warns if a Stage 3+ RFC exists without a corresponding entry in `docs/manual/`.

2.  **End-of-Phase Coherence (The "Gatekeeper")**

    - **Goal**: Ensure the phase is clean before closing.
    - **Trigger**: Runs on `exo phase finish`.
    - **Checks**:
      - **Lints/Tests**: `cargo check`, `npm test`.
      - **Links**: No broken internal links in docs.
      - **Git**: No uncommitted changes (unless explicitly allowed).
      - **Feedback**: All feedback tagged for this phase is resolved.

3.  **Deep Coherence (The "Architect")**
    - **Goal**: Ensure logical consistency and axiom alignment.
    - **Trigger**: On-demand (`exo check coherence --deep`) or before major RFC promotions.
    - **Checks**:
      - **Incoherence**: Do upcoming RFCs conflict with each other or the axioms?
      - **Relationship Integrity**: Are `related` links in RFCs bidirectional and meaningful?
      - **Stage Gate Validation**: Are "Unresolved Questions" in RFCs addressed?

#### D. Extraction Workflows (Axioms, Vision, Personas)

- **Goal**: Systematically harvest insights from the codebase and discussions to update the high-level guidance.
- **Process**:
  - **Axiom Extraction**: Review `docs/rfcs/*` and recent RFCs to identify recurring principles. Promote them to `AGENTS.md` or `docs/manual/architecture/axioms.md`.
  - **Vision Refinement**: Compare the current `vision.md` with the "on-the-ground" reality of the implementation. Update the vision to reflect new discoveries or pivots.
  - **Persona Development**: Analyze user feedback (or simulated feedback from "Zero to One") to refine the personas used in testing.

### 2. Phase Lifecycle Overhaul

The core lifecycle commands will be redesigned to handle the "New Chat" vs. "Same Chat" dichotomy explicitly and minimize context churn. We distinguish between **Preparing** a phase (planning) and **Starting** a phase (execution).

**Key Concept: Steering Injection**
Every `exo` command is an opportunity to inject "Steering Instructions" into the agent's context. Instead of just returning "OK" or a JSON blob, the tool returns a prompt-optimized message that tells the agent _exactly_ what to do next (e.g., "Plan valid. Run `exo phase start` to begin."). This reduces the chance of "wandering" and reinforces the state machine.

#### New Primitives

- `update-agent-context`: Saves the current in-memory state (decisions, progress) to disk (`docs/agent-context/`).
- `load-agent-context`: Reads the state from disk into the active context.
  - **Optimization**: Instead of waiting for the LLM to request files, this command should _emit_ the critical context (Plan, Active Tasks, Key Decisions) directly into the chat.
  - **Steering**: The output should explicitly guide the agent (e.g., "You are resuming Phase X. Here is your scratchpad. Your next step is Y.").

#### The Workflows

- **Phase Prepare (`exo phase prepare <phase-id>`)**:

  - **Goal**: Define the work for the _next_ phase.
  - **Transition**: Moves phase from `Future` -> `Preparing`.
  - **Process**:
    - **RFC Selection**: User and Agent agree on which RFCs to tackle.
    - **Scaffolding**: Creates `docs/agent-context/<phase-id>/`.
    - **Plan Generation**: Creates `implementation-plan.toml` (the machine-readable source of truth) and `task-list.md` (the human/agent view).
  - **Completion**: When the plan is finalized and validated, the state transitions to `Ready`.
  - **Steering**: "Phase <id> prepared and validated. Review `implementation-plan.toml`. If correct, run `exo phase start <id>` to begin execution."

- **Phase Start (`exo phase start <phase-id>`)**:

  - **Goal**: Begin execution of a prepared phase.
  - **Validation**: Relies on the **State Machine**. Checks `plan.toml` to ensure Phase `<phase-id>` is in the `Ready` state.
    - _Note_: We trust the state because the transition to `Ready` (during `prepare`) implies the plan was validated.
  - **Automated**: Transitions state from `Ready` -> `Active` in `plan.toml`.
  - **Context**: Loads the plan and relevant RFCs.
  - **Steering**: "Phase <id> is now Active. Context loaded. Your first task is: <Task 1>. Begin execution."

- **Phase Status (`exo phase status`)**:

  - **Goal**: Reorient the user and the agent.
  - **Automated**:
    - **RFC Context**: Tersely lists active RFCs for this phase, their current stage, and target stage (e.g., `RFC 0030: 0 -> 1`).
    - Summarizes the current state from `plan.toml` and `task-list.md`.
    - Checks for "drift" between the `implementation-plan.toml` and the completed tasks.
  - **Steering**: "You are 50% through Phase <id>. The next pending task is <Task N>. No drift detected."

- **Phase Finish (`exo phase finish`)**:

  - **Goal**: Close the current phase and record history.
  - **Validation**: Verifies all tasks in `task-list.md` are complete.
  - **Coherence (Mini)**: Runs a targeted check (lints, broken links, uncommitted changes).
  - **Changelog**: Prompts to update the `changelog` frontmatter in relevant RFCs.
  - **Commit**: Updates `plan.toml` to mark the phase as "Completed".
  - **Steering**: "Phase <id> Complete. Changelog updated. Ready to prepare next phase? Run `exo phase prepare <next-id>`."

- **Phase Transition**:
  - This is the high-level flow of `finish` (current) -> `prepare` (next) -> `start` (next).
  - **Handoff**:
    - Asks the user: "New chat or continue?"
    - _New Chat_: Serializes the _new_ plan and context to disk, provides a resume token.

### 3. Rust Tooling Automation (`exo`)

We will expand the `exo` Rust tool to replace manual shell scripts and Python glue, following the philosophy of **RFC 0132**.

- **Philosophy**:
  - **Data-First**: Internal logic uses structured data (`plan.toml`, `implementation-plan.toml`).
  - **Steering-First (New Axiom)**: The CLI is not just a passive tool; it is an active participant that guides the agent through the state machine. Every command output should include a "next step" instruction.
  - **Dual Interface**:
    - **Porcelain**: Human-friendly TUI commands (e.g., `exo phase status` prints a nice table).
    - **Plumbing**: Agent-friendly JSON output (e.g., `exo phase status --json` returns structured state AND steering instructions).
- **`exo phase state`**:
  - Manages the **Lifecycle State Machine** stored in `plan.toml`.
  - **States**:
    - **Standard Flow**: `Future` -> `Preparing` -> `Ready` -> `Active` -> `Completed`.
    - **Exception States**:
      - `Deferred`: A phase that was in progress or ready but has been postponed. Can transition back to `Preparing` or `Ready`.
      - `Withdrawn`: A phase that has been cancelled. Terminal state (unless resurrected to `Future`).
  - **Invariants**:
    - Only one phase can be `Active` at a time.
    - **Mutation via Command Only**: The `state` field in `plan.toml` is strictly managed by `exo` commands. Agents must never edit it manually. This ensures that transitions (e.g., `Preparing` -> `Ready`) only happen after successful validation (e.g., "Plan is valid").
  - **Trust**: The agent relies on this state rather than ad-hoc file checks.
  - Used by the agent to determine valid next actions (e.g., can't `start` if `Active`).
- **`exo plan`**: Commands to manipulate `plan.toml` and `task-list.md` programmatically.
  - _Source of Truth_: `plan.toml` is the master record. `task-list.md` is a transient view for the agent.
  - _Agent Autonomy_: The agent can `add-task` or `modify-task` freely.
- **`exo context`**: Commands to manage the `docs/agent-context` state.
  - `exo context save`: Snapshots current state.
  - `exo context restore`: Restores state.
- **`exo check`**:
  - `exo check coherence`: Runs the doc/code drift detection.
  - `exo check axioms`: Runs the axiom validation.

### 4. UI Integration

These workflows should be exposed in the Exosuit UI (e.g., the Sidebar or a Command Palette).

- Buttons for "Start Phase", "Complete Task", "Transition".
- Visual indicators for "Coherence Drift" (e.g., a warning if docs are stale).

## Drawbacks

- **Tooling Overhead**: Requires writing significant Rust code to replace simple shell scripts.
- **Rigidity**: Hard-coded workflows in Rust might be less flexible than an LLM interpreting a prompt. We must ensure the tools are composable.
- **Verbosity (Axiom 10)**: "Steering-First" means tools produce more output. This consumes more tokens. However, the cost of tokens is negligible compared to the cost of an agent hallucinating or getting lost (which wastes entire turns).

## Alternatives

- **Better Prompts**: We could just write better prompts, but that doesn't solve the "manual labor" or "determinism" issues.
- **Python Scripts**: We could stick to Python, but Rust is the chosen infrastructure language and offers better type safety and performance for the "Exosuit" binary.

## Unresolved Questions

- How exactly do we implement "Axiom Checking" programmatically? Is it an LLM call wrapped in the tool?
- What is the exact schema for the `agent-context` serialization?

