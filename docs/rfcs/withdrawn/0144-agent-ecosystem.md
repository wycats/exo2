<!-- exo:144 ulid:01kg5kp2j4twqrd1bpncsys2ec -->

# RFC 144: Agent Ecosystem


# RFC 0144: Agent Ecosystem

## Summary

Formalize and integrate the custom agent system (`.github/agents/`) into Exosuit's workflow, establishing a role-based agent ecosystem that aligns with the phase-based collaboration model.

## Motivation

### The Problem

Exosuit's shipping strategy emphasizes **repeatable, phase-gated work** and **human-AI division of labor**. Currently:

1. The chat participant handles all interactions uniformly
2. There's no formal separation between exploration, planning, execution, and review
3. Model selection is implicit rather than task-appropriate
4. Workflow discipline depends on user memory rather than agent specialization

### The Opportunity

We've prototyped a **role-based agent ecosystem**:

| Agent            | Model       | Role                              | Writes Code? |
|------------------|-------------|-----------------------------------|--------------|
| **Recon**        | Opus        | Explore and map the codebase      | No           |
| **Recon-Worker** | Codex       | Gather raw data for Recon         | No           |
| **Prepare**      | Opus        | Audit plan ↔ codebase alignment   | No           |
| **Execute**      | Codex       | Perform the planned work          | Yes          |
| **Review**       | Opus        | Evaluate completed work           | No           |

This maps directly to the core loop: **Recon → Prepare → Execute → Review → (iterate)**

### Why Now

The shipping focus documents identify key disconnects:
- **Workflow vs. Practice**: Rituals exist but aren't enforced
- **Visibility**: No dashboard showing workflow state
- **Idea/RFC Integration**: Loose coupling to structured artifacts

Specialized agents can **encode workflow discipline** at the behavioral level, ensuring the right actions happen at the right time.

## Detailed Design

### Terminology

- **Agent**: A specialized prompt + model + tool configuration in `.github/agents/`
- **Coordinator Agent**: An agent with the `agent` tool that can spawn other agents
- **Worker Agent**: An agent without the `agent` tool; returns data to its caller
- **Agent Ecosystem**: The set of agents and their handoff relationships

### User Experience (UX)

#### Invoking Agents

Users invoke agents via:
```
@workspace /agent:recon How does the notification system work?
@workspace /agent:prepare Is the current phase ready for execution?
@workspace /agent:execute Implement the tasks in the current phase
@workspace /agent:review Review the changes in this PR
```

Or via natural language:
```
Have a recon agent explore the auth module
```

#### Workflow Prompt

The `phase_transition` prompt (in `prompts.toml`) orchestrates the handoff:
```
1. Review: Have a `review` agent verify the completed work
2. Commit: If approved, have an `execute` agent commit
3. Prepare: Have a `prepare` agent audit the next phase
```

#### Visibility (Future)

A dashboard could show:
- Current agent (if any)
- Agent recommendations based on phase state
- Recent agent invocations and their outputs

### Architecture

#### File Structure
```
.github/agents/
├── recon.md          # Opus coordinator
├── recon-worker.md   # Codex gatherer
├── prepare.md        # Opus auditor
├── execute.md        # Codex implementer
└── review.md         # Opus evaluator
```

#### Agent Definition Schema
```yaml
---
description: "Brief description for agent selection"
model: Claude Opus 4 (copilot)  # or GPT-5.2-Codex, etc.
tools: [...]                     # Tool allowlist
---

[Prompt content with Agent Ecosystem table, guidelines, output templates]
```

#### Coordinator/Worker Pattern

```
┌─────────────────────────────────┐
│  Coordinator (Opus)             │
│  - Receive question             │
│  - Plan exploration             │
│  - Dispatch workers             │
│  - Synthesize report            │
└─────────────────────────────────┘
         │         │         │
         ▼         ▼         ▼
    ┌────────┐ ┌────────┐ ┌────────┐
    │Worker  │ │Worker  │ │Worker  │
    │(Codex) │ │(Codex) │ │(Codex) │
    └────────┘ └────────┘ └────────┘
```

Workers gather raw data; coordinators synthesize.

#### Integration Points

| Component | Integration |
|-----------|-------------|
| **LM Tools** | Agents use `exo-status`, `exo-phase`, `exo-task-complete`, etc. |
| **prompts.toml** | `phase_transition` prompt references agents by name |
| **Phase Workflow** | Agents mediate phase transitions |
| **RFC Lifecycle** | Prepare agent could check RFC alignment (future) |

### Implementation Details

#### Model Selection Rationale

| Role | Model | Rationale |
|------|-------|-----------|
| Thinking (Recon, Prepare, Review) | Opus | Deep reasoning, synthesis, judgment |
| Doing (Execute, Recon-Worker) | Codex | Tool use, speed, instruction following |

#### Tool Allowlists

| Agent | Tools | Rationale |
|-------|-------|-----------|
| Recon | read, search, agent, exo-* | Exploration + delegation |
| Recon-Worker | read, search, terminal | Mechanical gathering only |
| Prepare | read, search, exo-* | Audit, no edits |
| Execute | edit, execute, terminal, todo, exo-* | Full implementation |
| Review | read, search, exo-* | Evaluation, no edits |

#### Output Templates

Each agent has a structured output template:
- **Recon**: Recon Report (Summary, Key Findings, Architecture, Unknowns)
- **Prepare**: Readiness Report (Status, Blockers, Caveats, Verified Assumptions)
- **Review**: Review (Verdict, Blockers, Suggestions, Questions, Praise)

## Implementation Plan (Stage 2)

- [ ] Finalize agent definitions in `.github/agents/`
- [ ] Add agent invocation documentation to manual
- [ ] Wire `phase_transition` prompt to phase lifecycle
- [ ] Add "When to Escalate" sections to all agents
- [ ] Test coordinator/worker pattern in real workflows
- [ ] Consider visibility dashboard (future RFC)

## Context Updates (Stage 3)

- [ ] Create `docs/manual/features/agents.md`
- [ ] Update `docs/manual/core-loop.md` with agent roles
- [ ] Add agents to shipping focus documents as differentiator
- [ ] Update `AGENTS.md` to reference `.github/agents/`

## Drawbacks

1. **Complexity**: Multiple agents vs. one chat participant adds cognitive overhead
2. **Handoff Friction**: Serializing context between agents costs tokens
3. **Maintenance**: Agent definitions must stay synchronized
4. **Discovery**: Users must learn which agent to invoke

## Alternatives

### A. Single Agent with Mode Switching
Keep one agent, switch behavior via modes. Rejected because:
- Model selection can't vary by mode
- Tool allowlists can't vary by mode
- Less explicit division of labor

### B. Fully Automated Orchestration
A meta-agent that automatically selects the right agent. Deferred because:
- Adds another layer of indirection
- User loses control over agent selection
- Harder to debug

### C. No Agents (Status Quo)
Rely on user discipline and prompts. Rejected because:
- Workflow discipline is inconsistent
- Model selection is suboptimal
- No clear separation of concerns

## Unresolved Questions

1. **Modes vs. Agents**: What's the relationship? Are they complementary or overlapping?
2. **Agent Selection UI**: Should there be a picker or recommendations?
3. **Worker Model**: Is Codex the best choice, or would Gemini/Sonnet be better?
4. **Nesting Depth**: How deep should coordinator/worker chains go?

## Future Possibilities

1. **Agent Recommendations**: Steering tool suggests which agent to invoke
2. **Agent Metrics**: Track agent usage, success rates, token efficiency
3. **Custom Agents**: Users define their own agents
4. **Agent Marketplace**: Share agent definitions across projects
5. **Automated Handoff**: Phase transitions auto-invoke appropriate agents
