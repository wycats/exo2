<!-- exo:10166 ulid:01kmzxey0s6263erxdnjkrar9t -->


# RFC 10166: Architect Agent Mode (Two-Agent Workflow)

## Summary

Introduce a "two agent" workflow where a primary "Implementor" agent focuses on code generation and task execution, while a secondary "Architect" agent focuses on high-level design, RFC compliance, vetting implementation strategies, and **drafting new RFCs**. The user acts as the bridge and arbiter between these two agents.

## Motivation

As the project grows, the context required to understand the full architectural vision (all RFCs, axioms, and long-term goals) becomes too large for a single agent session focused on implementation details.

Currently, the user performs this role ad-hoc, sometimes simulating an "Architect" voice or manually checking RFCs. This proposal aims to formalize this pattern, allowing the user to:

1.  Keep the Implementor focused on the "how".
2.  Consult the Architect on the "why" and "what", ensuring alignment with the "Laws" (RFCs).
3.  **Leverage the Architect to draft new RFCs (Stage 0/1) by synthesizing user intent with existing axioms.**
4.  Streamline the communication between these roles in the VS Code UI.

## Detailed Design

### The Roles

1.  **The Implementor**:

    - **Context**: Current task, active source files, immediate implementation plan.
    - **Goal**: Write code, pass tests, complete tasks.
    - **Personality**: Focused, tactical, detail-oriented.

2.  **The Architect**:
    - **Context**: `docs/rfcs/`, `docs/agent-context/axioms.workflow.toml`, `docs/agent-context/axioms.system.toml`, `docs/design/axioms.design.toml`, `docs/agent-context/decisions.toml`, high-level specs.
    - **Goal**: Vet ideas, ensure consistency, interpret "The Law", **draft new RFCs**.
    - **Personality**: Strategic, critical, vision-oriented.

### User Experience (UX)

The user remains the central node. The user decides _when_ to consult the Architect.

**Proposed Workflow 1: Vetting**

1.  User is working with Implementor on a task.
2.  Implementor proposes a complex change.
3.  User invokes "Ask Architect" (via command or UI).
4.  User (or system) frames the question: "The Implementor suggests X. Does this align with RFC Y?"
5.  Architect responds with critique or approval.
6.  User feeds this feedback back to the Implementor.

**Proposed Workflow 2: RFC Drafting**

1.  User has a new idea but needs to flesh it out.
2.  User invokes "Architect: Draft RFC".
3.  Architect loads existing RFCs and Axioms to ensure coherence.
4.  Architect interviews the user to gather requirements.
5.  Architect generates a Stage 0 RFC.

**UI Ideas:**

- **Explicit Agent Switching**: A dropdown or command to switch the active chat context between "Implementor" and "Architect".
- **Multi-Turn Conversation UI**: Visual distinction (e.g., different icons or colors) for messages from different agents if they appear in the same stream (though separate streams might be cleaner).
- **Ad-hoc Syntax Support**: Formalize the user's existing `<from agent="implementor">` syntax to help the LLM understand which "hat" it is wearing or who it is listening to, if we are simulating this within a single session.

### Architecture

- **Chat Participant (`@architect`)**:
  - The Architect will be implemented as a dedicated VS Code Chat Participant.
  - **Benefit**: Allows inline consultation within the chat stream without polluting the main "Implementor" context window.
  - **Context Strategy**: The Architect will automatically load the "Laws" (RFCs and axioms) and the current project state through Exo commands/API surfaces to stay aligned with the active phase.
- **Context Management**: The system needs to be able to swap "System Prompts" and "Context Files" quickly.
  - _Implementor Context_: source files, tests, and active task/phase state.
  - _Architect Context_: RFCs, axioms, durable design docs, and current phase state.
- **VS Code Extension**:
  - Register `@architect` participant in `package.json`.
  - Implement a handler that sets the system prompt to the "Architect Persona" and retrieves relevant RFCs.

## Drawbacks

- **Complexity**: Managing two context states is more complex than one.
- **Latency**: Switching contexts might incur overhead if not optimized.
- **User Friction**: If the UI is clunky, the user won't use it.

## Alternatives

- **Single "Super" Agent**: Try to stuff everything into context (hits token limits, confuses focus).
- **Manual Context Swapping**: User manually adds/removes files (tedious).

## Unresolved Questions

- How do we efficiently pass the "state" of the implementation to the Architect without overloading its context? (Maybe just the diff or the specific question?)
- Should this be two separate chat sessions or one session with role-switching? (Decision: Use `@architect` participant for inline role-switching).
