<!-- exo:18 ulid:01kg5kp2btt0cftf3snxkzsj7g -->

# RFC 18: The `exo` CLI

- **Supersedes**: RFC 10141



# RFC 0018: The `exo` CLI

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
    - **Update Plan**: Mark the current phase as `completed` in canonical SQLite state.
    - **Transition**: Identify the next pending phase and offer to start it (or print instructions).
- `exo context`: Manage the agent context.
  - `restore`: Print the full context for the agent.
- `exo rfc`: Manage RFCs (formerly `rfc-status`).
  - `list`: List all RFCs (default).
  - `promote <id>`: Move an RFC to the next stage.

### 2. Data Models

We will map canonical project state to Rust structs using typed storage models:

- `ExoState`: Maps the canonical project-state tables.
- `AgentContext`: Aggregates `root` path and `ExoState`.

### 3. Migration Strategy

1.  Rename `tools/rfc-status` to `tools/exo`.
2.  Port `scripts/agent/check-docs.sh` logic (RFC listing) to `exo rfc`.
3.  Port `scripts/agent/phase-status.sh` to `exo phase status`.
4.  Port `scripts/agent/restore-context.sh` to `exo context restore`.
5.  Delete legacy scripts.

## Manual Updates

When promoting to Stage 3, the following manual sections must be updated:

- The relevant Stage 3/4 RFCs: Document the new CLI commands and workflow descriptions using `exo` instead of scripts.

## Amendments

### CLI Taxonomy: Entity-Noun CRUD (2026-02)

The original design placed entity CRUD operations under `exo plan` (e.g., `exo plan add-epoch`,
`exo plan add-phase`, `exo plan add-task`). This was replaced with entity-noun namespacing where
CRUD operations live under their natural nouns:

- `exo epoch add/remove/bankrupt` (was `exo plan add-epoch/remove-epoch/bankrupt`)
- `exo phase add/remove` (was `exo plan add-phase/remove-phase/update-phase`)
- `exo goal add/remove/complete/abandon` (was `exo plan add-task/remove-task`)
- `exo task add/complete/remove/list` (standalone namespace)

`exo plan` now contains only plan-wide operations: `health`, `review`, `update-status`,
`linearize`, `migrate-ids`.

The CLI has grown to 22 namespaces and 98 operations. The authoritative command reference is
generated from the registry: `packages/exosuit-vscode/src/command-spec.json`.
