<!-- exo:52 ulid:01kg5kp2dg2mxab6zjcqb8gt1x -->

# RFC 52: The Agent Quality Loop

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0052: The Agent Quality Loop

## Summary

This RFC proposes a set of enhancements to the `exo` CLI to foster an "Organic Quality Loop" for AI agents. Instead of enforcing quality via paternalistic gates ("Did you write tests?"), we aim to **steer** the agent toward high-quality behavior by providing Just-In-Time (JIT) context, mission-oriented prompts, and narrative-driven verification.

## Motivation

Current agent workflows suffer from two extremes:

1.  **The Wild West**: Agents implement features without checking existing standards, leading to drift.
2.  **The Nanny State**: Agents are forced to fill out checklists ("I have written tests"), leading to performative compliance.

We need a third way: **The Paved Road**. The environment itself should make high-quality work the path of least resistance.

## The Steering Philosophy

We define three core axioms for Agent Steering:

### 1. The Principle of Paved Roads

**"The Tool is a Guide, not a Gatekeeper."**
We do not force the Agent to walk the path of quality; we pave that path so smoothly that walking off it feels like unnecessary effort.

- _Anti-Pattern_: Blocking the agent for missing a tag.
- _Pattern_: Suggesting relevant tags based on the current epoch.

### 2. The Principle of Contextual Resonance

**"Every Output is a Prompt."**
The CLI's stdout is the Agent's stdin. We use every command output to subtly prime the Agent's latent knowledge and persona.

- _Anti-Pattern_: `Task started.` (Zero context).
- _Pattern_: `[Mission] Task started. Remember: In this Epoch, we value "Glitch Freedom".`

### 3. The Principle of Professional Dignity

**"Optimize for Pride, not Compliance."**
We treat the Agent as a Senior Engineer. We ask for "Narratives of Victory," not "Proof of Compliance."

- _Anti-Pattern_: "Upload a screenshot to prove you did it."
- _Pattern_: "Show me what you built. If I were reviewing this PR, what would make me say 'Wow'?"

## Implementation Plan

### 1. The Law Library (`exo rfc`)

Enable the agent to "Consult the Oracle" effortlessly.

- **Commands**:
  - `exo rfc list --tag <tag>`: Find relevant standards.
  - `exo rfc search <query>`: Semantic/Keyword search.
  - `exo rfc show <id>`: Read the law.
- **Steering**: When an agent struggles or asks about "standards," the system prompts it to use `exo rfc`.

### 2. The Mission Briefing (`exo task start`)

Transform `exo task start` into a context-injection event.

- **Logic**:
  1.  Identify the Task's Epoch and Tags.
  2.  Retrieve relevant "Quality Axioms" (e.g., UI -> Glitch Freedom, Core -> Idempotency).
  3.  Output a "Mission Briefing" block that primes the agent with these values.

### 3. The Narrative Walkthrough (`exo walkthrough`)

Refactor `exo walkthrough` to encourage storytelling.

- **Prompting**: Instead of generic "Description", the CLI asks specific, pride-oriented questions based on the entry type.
  - _Feature_: "What is the 'North Star' user experience of this feature?"
  - _Refactor_: "How did you leave the code better than you found it?"
  - _Fix_: "How did you prove the bug is dead?"

## User Experience

**Starting a Task:**

```text
> exo task start "Implement Sidebar"
[System] Task "Implement Sidebar" started.
[Context] Epoch: "User Experience" | Tags: [UI, VSCode]
[Mission] In this Epoch, we value "Glitch Freedom". Ensure no stale state exists after updates.
[Tip] Run `exo rfc list --tag ui` to review UI standards.
```

**Adding a Walkthrough Entry:**

```text
> exo walkthrough add
[System] What kind of entry is this? [Feature/Fix/Refactor]
> Feature
[System] Great. Describe the "North Star" experience. What makes this feature shine?
> ...
```

