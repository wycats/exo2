<!-- exo:167 ulid:01kg5kp2k8srcv593ayz36gtp1 -->

# RFC 167: Lightweight Checks / Cognitive Load

- **Stage**: 0
- **Reason**:

# RFC 0167: Lightweight Checks / Cognitive Load

**Status**: Withdrawn (Consolidated)
**Feature**: Agent Workflow

## Context

The AI agent often forgets to run "lightweight checks" (like `cargo check`, `cargo fmt`, or `npm run lint`) until the very end of a phase, leading to a "fix-commit" loop where the agent is blocked by pre-commit hooks. This breaks flow and increases cognitive load as the agent has to context-switch from "feature work" to "janitorial work".

Currently, the agent relies on the user or the `verify-phase.sh` script to catch these issues. However, `verify-phase.sh` is often run only at the end.

## Problem

1.  **Late Feedback**: Errors are caught too late (at commit time), causing frustration.
2.  **Cognitive Overload**: The agent has to "remember" to run these checks, which competes with the primary task for context window and attention.
3.  **Broken Flow**: The "fix-commit" loop is a distraction from the actual work.

## Proposal (Strawman)

We need a mechanism to encourage or automate these checks _during_ the workflow, without overwhelming the agent with constant prompts or slow feedback loops.

### Idea 1: `exo check` Command

A unified command that runs the relevant lightweight checks for the current context (Rust, TS, etc.) and reports _only_ new errors or high-priority issues.

- **Pros**: Simple to invoke.
- **Cons**: Agent still has to remember to run it.

### Idea 2: "Steering" Injection

The `exo` CLI (or the system prompt) could inject "steering" instructions that remind the agent to run checks after modifying files.

- **Example**: "You just edited a Rust file. Run `cargo check` to verify."
- **Pros**: Just-in-time reminder.
- **Cons**: Might get annoying if too frequent.

### Idea 3: Automated "Micro-Checks" in Tooling

When the agent uses `edit_file`, the tool itself could optionally trigger a fast check (e.g., `cargo check --lib`) and return the result _with_ the tool output.

- **Pros**: Zero cognitive load. Immediate feedback.
- **Cons**: Might slow down tool execution.

## Open Questions

1.  What is the "cost" of these checks? (Time vs. Token vs. Cognitive Load)
2.  How do we distinguish between "blocking" errors and "nitpicks" during the flow?
3.  Can we leverage `lefthook` configuration for this?

## Next Steps

- Analyze the performance impact of running checks on every edit.
- Experiment with "Steering" prompts.


