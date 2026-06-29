<!-- exo:10141 ulid:01kmzxbczv97n40nfdxn2qj0k3 -->


# RFC 10141: The `exo` CLI

- **Superseded by**: RFC 10014


## Summary

Replace the collection of bash scripts in `scripts/agent/` with a unified Rust CLI tool (`exo`) to manage the agent workflow, context, and RFC process.

## Motivation

- **Maintainability**: Bash scripts are brittle and hard to test. Rust provides type safety and better error handling.
- **Unified Interface**: A single entry point (`exo`) is easier to discover and use than multiple scripts.
- **Performance**: Rust is faster, especially for parsing large context files.
- **Extensibility**: Easier to add complex logic (like "Smart Task Verification" or "Context Relevance") in Rust.

## Detailed Design

### 1. Command Structure

The `exo` tool will use `clap` for subcommand parsing:

- `exo phase`: Manage the phase lifecycle.
  - `status`: Show current phase and tasks.
  - `start <id>`: Start a new phase.
  - `finish`: Complete the current phase.
    - **Commit**: Enforce a clean git state (or use `--message` to commit).
    - **Update Plan**: Mark the current phase as `completed` in `plan.toml`.
    - **Transition**: Identify the next pending phase and offer to start it (or print instructions).
- `exo context`: Manage the agent context.
  - `restore`: Print the full context for the agent.
- `exo rfc`: Manage RFCs (formerly `rfc-status`).
  - `list`: List all RFCs (default).
  - `promote <id>`: Move an RFC to the next stage.

### 2. Data Models

We will map the TOML files in `docs/agent-context/` to Rust structs using `serde`:

- `ExoState`: Maps `plan.toml`.
- `AgentContext`: Aggregates `root` path and `ExoState`.

### 3. Migration Strategy

1.  Rename `tools/rfc-status` to `tools/exo`.
2.  Port `scripts/agent/check-docs.sh` logic (RFC listing) to `exo rfc`.
3.  Port `scripts/agent/phase-status.sh` to `exo phase status`.
4.  Port `scripts/agent/restore-context.sh` to `exo context restore`.
5.  Delete legacy scripts.

## Manual Updates

When promoting to Stage 3, the following manual sections must be updated:

- `docs/manual/features/cli.md`: Document the new CLI commands.
- `docs/manual/meta/workflow.md`: Update workflow descriptions to use `exo` instead of scripts.