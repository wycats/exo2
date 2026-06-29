<!-- exo:240 ulid:01kmzxey242w7tv0v5j48vjapk -->

# RFC 240: Fractal SOAR & The Goal Loop


# RFC 00240: Fractal SOAR & The Goal Loop

## Summary

This RFC formalizes the "Fractal SOAR" architecture, unifying the high-level project workflow (Epochs/Phases) with the low-level agent execution loop (PER). It redefines the "PER" pattern (Prepare -> Execute -> Review) as the **Goal Loop**—a specific instance of the SOAR cycle applied at the granularity of a single goal.

## Motivation

We currently have two disconnected workflows:

1.  **SOAR (Status-Orient-Act-Review)**: RFC 00224 defined this for human-AI collaboration at the session/phase level.
2.  **PER (Prepare-Execute-Review)**: A growing pattern for agent delegation tasks.

These are not distinct concepts. They are the same loop applied at different scales. Recognizing this self-similarity allows us to:

- Standardize tooling across scales.
- Implement "Model Routing" (Reasoning models for Orient, Coding models for Act).
- Use `exo` to manage `.github/agents` artifacts to enforce this architecture.

## The Fractal Model

Work is self-similar at every scale.

| Scale  | Loop Name | Focus       | Governance Artifact                     |
| :----- | :-------- | :---------- | :-------------------------------------- |
| **L3** | **Epoch** | Strategic   | `plan.toml` (Roadmap)                   |
| **L2** | **Phase** | Tactical    | `implementation-plan.toml` (Phase Plan) |
| **L1** | **Goal**  | Operational | **The Goal Loop** (formerly PER)        |

## The Goal Loop (L1 SOAR)

The "PER" cycle is re-mapped to SOAR phases:

### 1. Status (Context Audit)

- **Agent:** Main Agent / Startup
- **Focus:** "What exists before we start?"
- **Output:** Focused context dump.

### 2. Orient (Prepare)

- **Agent:** `@prepare`
- **Model Profile:** Reasoning / Planning (e.g., o1, o3-mini)
- **Focus:** Audit the goal, check files, detect regressions, decompose steps.
- **Output:** `prepare-report.md` (Artifact).

### 3. Act (Execute)

- **Agent:** `@execute`
- **Model Profile:** Coding / Speed (e.g., Claude 3.5 Sonnet, GPT-4o)
- **Focus:** Execute the plan blindly and faithfully. TDD Red/Green.
- **Output:** Code changes.

### 4. Review (Review)

- **Agent:** `@review`
- **Model Profile:** Critique / Analysis
- **Focus:** Verify the Act against the Orientation.
- **Output:** `review-verdict.md` (Artifact).

## Implementation Strategy

We will implement this via a **Compiler Architecture**, where `exo` acts as the definitive build tool for the project's AI Context.

### 1. The Agent Context Compiler

The `.github/` folder (specifically `copilot-instructions.md` and `agents/*.json`) is no longer treated as "Source", but as a **Rendered View** of the `docs/agent-context` database.

#### The Pipeline

`docs/agent-context` (Source) → `exo sync` (Compiler) → `.github` (Artifact)

### 2. The Inputs (Source of Truth)

Users define the project's intent in `docs/agent-context/`:

| Component  | File Path              | Purpose                                                 |
| :--------- | :--------------------- | :------------------------------------------------------ |
| **Agents** | `agents.toml`          | Defines active agents, models, and toolsets.            |
| **Config** | `prompts.toml`         | Defines voice, system prompt rules, and user overrides. |
| **Axioms** | `axioms.*.toml`        | Immutable project laws (System, Workflow, etc).         |
| **State**  | `plan.toml`            | Dynamic context (Current Phase, Goals).                 |
| **Hooks**  | `instructions/user.md` | User-defined instructions to inject (optional).         |

### 3. The Outputs (Build Artifacts)

The `exo` tool compiles these inputs into standard Copilot-compatible formats:

1.  **`.github/copilot-instructions.md`**:
    - **Context-Aware**: Injects the _Active Phase_ and _Goals_ directly into the system prompt.
    - **Composable**: Merges Core Axioms + User Instructions + Project Context.
    - **Managed**: Includes a "DO NOT EDIT" header directing users to `docs/agent-context`.

2.  **`.github/agents/*.json`**:
    - **Optimized Routing**: Generated with the specific model ID defined in `agents.toml` (e.g., `o3-mini` for Prepare, `gpt-4o` for Execute).
    - **Shared Primitives**: System prompts recycle the same Axiom definitions as the main agent, ensuring consistency.

### 4. The Goal Loop (CLI)

The Goal Loop is reified in the CLI to support the Agent Compiler:

- `exo goal loop <id> <phase>`: Generates the context artifact for a specific phase of the goal loop.
  - Example: `exo goal loop 123 orient` -> records a structured orientation packet in Exo state.
  - The Agent (`@prepare`) is then invoked with that packet through the Exo command/API surface.

### 5. Unified Terminology

- **Retire "PER"**: The acronym is replaced by "Goal Loop" or "L1 SOAR".
- **Retire Manual Edits**: All changes to agent behavior must go through `docs/agent-context`.

## Related RFCs

- RFC 00224: The SOAR Loop — Parent RFC defining the SOAR model that this RFC extends fractally
- RFC 10170: Mutation Boundaries in Feedback Loops — Clarifies when steering can happen inside L1 loops; mutation boundaries define natural pause points for contextual injection
