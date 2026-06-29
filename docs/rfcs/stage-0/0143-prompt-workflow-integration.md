<!-- exo:143 ulid:01kg5kp2j33ngn7f8m9y4w18zm -->

# RFC 143: Prompt Workflow Integration


# RFC 0143: Prompt Workflow Integration

## Summary

Wire lifecycle events (phase transitions, task completion, mutations) to contextual prompt invocation, so the agent's behavior adapts to the work phase automatically.

## Current State (as of 2026-03)

The project has several prompt/workflow mechanisms that operate independently:

- **`.github/prompts/`** — VS Code prompt files for specific workflows (session-handoff, RFC consolidation, phase-start, etc.). These are manually invoked by the user.
- **`AGENTS.md`** — Defines the SOAR loop (Status → Orient → Act → Review) and agent roles (recon, prepare, execute, review).
- **`copilot-instructions.md`** — Axioms and workflow guidance injected into every agent session.
- **Steering system** — `exo status` and `exo steering` provide contextual guidance based on project state.

What's missing: **automatic invocation**. The user must know which prompt to use and when. The system doesn't trigger prompts based on lifecycle events.

## Motivation

The gap is between _definition_ and _practice_:

1. **Prompts exist but aren't invoked**: Workflow prompts like session-handoff and phase-transition are defined but not systematically triggered by lifecycle events.
2. **Axioms are listed but not enforced**: Axiom files exist but aren't checked by prompts at gate points (e.g., Green-to-Green before phase finish).
3. **Personas were never integrated**: The Fresh Eyes review concept (evaluating from different user perspectives) was a valid idea that got conflated with "modes" and abandoned. It should be revisited as a prompt workflow, not a mode system.

## The Opportunity

Lifecycle events already exist in the system — phase start, phase finish, task completion, goal completion, RFC promotion. Prompt files already exist in `.github/prompts/`. The missing piece is the wiring: the system should trigger the right prompt at the right time.

| Lifecycle Event | Prompt That Should Fire                       | Currently           |
| --------------- | --------------------------------------------- | ------------------- |
| Phase start     | `.github/prompts/phase-start.prompt.md`       | Manual              |
| Phase finish    | `.github/prompts/phase-transition.prompt.md`  | Manual              |
| Session end     | `.github/prompts/session-handoff.prompt.md`   | Manual              |
| RFC work        | `.github/prompts/rfc-consolidation.prompt.md` | Manual              |
| Task completion | Axiom check (Green-to-Green)                  | Steering hints only |

## Design Direction

### Gate Prompts

A **gate prompt** fires automatically at a lifecycle transition. The system checks whether a prompt is registered for the event and invokes it.

Gates can be:

- **Required**: Blocks the action until the prompt completes (e.g., Green-to-Green check before phase finish)
- **Advisory**: Runs the prompt but doesn't block (e.g., session handoff reminder)

### Prompt Context Injection

When a gate fires, the prompt receives context from the current project state (via `exo status`, `exo goal list`, etc.). This replaces the earlier idea of token interpolation — the SOAR loop already provides the context-gathering mechanism.

### Personas as Prompt Workflows

The Fresh Eyes review concept (evaluating from different user perspectives) was a valid idea from the persona system that got lost when personas were conflated with "modes." This should be revisited as a prompt workflow — a structured review that asks the agent to evaluate from specific perspectives, not a persistent behavioral mode.

## Open Questions

1. **Where do gate registrations live?** In `exosuit.toml`? In the prompt files themselves (frontmatter)?
2. **How does the agent invoke a gate?** Does the CLI trigger it, or does the LM tool surface?
3. **Should gates be skippable?** (`--skip-gates` flag)
4. **How do personas compose with the SOAR loop?** Is Fresh Eyes a Review-phase activity?

## Superseded Concepts

- **`prompts.toml`**: Deleted. Was a passive catalog that nothing read. Prompt files in `.github/prompts/` are the current mechanism.
- **`modes.toml`**: Deleted. The Thinking Partner / Chief of Staff / Maker taxonomy is superseded by the SOAR loop and agent roles (recon, prepare, execute, review).
- **RFC 0150 (Modes and Persona Unification)**: Withdrawn. The modes concept is superseded; the persona concept is captured here as a prompt workflow direction.
- **RFC 0084 (Pluggable Upgrade System)**: Withdrawn. TOML schema versioning is superseded by SQLite migrations.
