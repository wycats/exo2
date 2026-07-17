<!-- exo:80 ulid:01kg5kp2eye5n8nk4b1er48qmv -->

# RFC 80: Agent-first CLI Discovery Ladder

- **Supersedes**: RFC 0019, RFC 0055, RFC 10072



# RFC 0080: Agent-first CLI Discovery Ladder

## Summary

Consolidate the Exosuit CLI experience around a single, agent-first entry point for discovery and “what should I do next?” guidance, with `exo map` as the primary steering command.

## Motivation

As the CLI grows, users need a predictable way to:

- Discover the “next best action” in a repo.
- Understand why an action is recommended.
- See preconditions, repository health, and required repairs before proceeding.

## Design

### Steering vs action

- **Steering commands** explain what to do next and why.
- **Action commands** mutate state (update snapshots, change context files, etc.).

### The discovery ladder

`exo map` is the recommended “what should I do next?” entry point:

- `exo map`: show active phase tasks/steps and suggested actions.
- `exo map --next`: emit a single best next action (preferring repairs when unhealthy).
- `exo map --why <command>`: explain preconditions and effects for a command.

### Output protocol

Commands should support a shared output protocol:

- `--format human` (default): agent-first text output.
- `--format json`: machine-readable output that includes a `steering` block.

## Relationship to existing RFCs

This RFC is intended as a consolidation point for prior CLI-related proposals that were superseded.

## Open questions

- What is the minimal “canonical” structure of `steering` across commands?
- Which subcommands should be *steering-only* vs *action-only*?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This document is a best-effort reconstruction based on the current CLI manual and existing CLI behavior.
