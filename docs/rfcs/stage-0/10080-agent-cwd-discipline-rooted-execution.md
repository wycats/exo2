<!-- exo:10080 ulid:01kmzxeff4qb8ganaqsf47tpg9 -->


# RFC 10080: Agent CWD Discipline (Rooted Execution)

- **Superseded by**: RFC 0093


## Summary

This RFC proposes a strict **"Rooted Execution"** policy for the AI Agent. The agent MUST always execute commands from the Workspace Root. It MUST NOT change its persistent working directory (e.g., via `cd`). If a command requires a specific context, it must use sub-shells `(cd foo && bar)` or tool-specific flags (`npm -w`, `cargo -p`).

## Motivation

- **The "Lost Agent" Problem**: Agents often `cd` into a subdirectory, forget they are there, and then try to run a script that expects to be in the root (e.g., `./scripts/test.sh`).
- **Context Fragmentation**: Relative paths in the prompt or context become ambiguous if the CWD is mutable.
- **Tooling Fragility**: Many project scripts (`bootstrap.sh`, `generate.sh`) assume they are run from the root.

## Detailed Design

### 1. The Axiom

We add a new Operational Protocol to `AGENTS.md`:

> **Protocol: The Rooted Agent**
>
> - **Stay Rooted**: You MUST always execute commands from the workspace root.
> - **No `cd`**: Do not use `cd` to change the persistent state of the terminal.
> - **Sub-shells**: If you need to run a command in a folder, use `(cd folder && cmd)`.
> - **Flags**: Prefer tool flags: `cargo -p pkg`, `npm -w pkg`.

### 2. Tooling Enforcement (The `exo` Shell)

The `exo` CLI (or the `run_in_terminal` tool wrapper) can enforce this:

- **Warning**: Detect `cd` commands that are not in sub-shells and warn the agent.
- **Auto-Reset**: The tool could automatically `cd $WORKSPACE_ROOT` before every command (draconian but effective).

### 3. Standardizing Scripts

All scripts in `scripts/` must be written to be run from the root.

- **Good**: `cargo test -p exosuit-core`
- **Bad**: `cd packages/exosuit-core && cargo test` (as a primary workflow)

## Drawbacks

- **Verbosity**: `(cd packages/very-long-name && npm run build)` is more typing than `cd ...; npm run build`.
- **Habit**: Agents (and humans) are used to moving around.

## Alternatives

- **Smart Prompt**: Just tell the agent "You are in `packages/foo`" in every turn. (Fragile, consumes tokens).
- **Stateless Terminal**: The `run_in_terminal` tool could be stateless (always starts at root). (Maybe too restrictive for some multi-step shell operations).
