<!-- exo:30 ulid:01kg5kp2cdzsfpxahgnfskp2zg -->

# RFC 30: CLI Overhaul & Alignment



# RFC 0030: CLI Overhaul & Alignment

- **Superseded by**: RFC 0080


## Summary

This RFC proposes a significant update to the `exo` CLI to fully align it with the "Exosuit Way" workflow protocols defined in `AGENTS.md`. It introduces missing commands for managing the phase execution artifact, creates a unified `exo ai` entry point for context retrieval, and streamlines task management.

## Motivation

The current `exo` CLI has fallen out of sync with the strict protocols defined in `AGENTS.md`. Specifically:

1.  **Violation of Read-Only TOML**: The workflow mandates that `implementation-plan.toml` be treated as a read-only file modified only via CLI tools. Legacy projection artifacts are migration-only (see RFC 0064). However, no `exo impl log` command exists, and `exo plan` is too verbose for daily task management, forcing agents to edit these files manually.
2.  **Missing AI Context Tools**: There is no standardized way for an agent to "dump context" or "get a prompt" without running ad-hoc scripts like `scripts/context-dump.ts`.
3.  **Fragmented Verification**: Verification scripts (`verify-toml.ts`, `check-wasm.sh`) are scattered and not unified under a single `exo verify` command.

This overhaul aims to make the "Exosuit Way" the _easiest_ way to work, by providing first-class CLI support for every step of the Phase Loop.

## Detailed Design

### 1. `exo impl log`

A new set of subcommands to manage the log/journal portion of `docs/agent-context/current/implementation-plan.toml`.

**Commands:**

- `exo impl log add`: Adds a new entry.
  - Flags: `--type <feat|fix|test|design>`, `--description "..."`, `--details "..."`
  - Example: `exo impl log add --type feat --description "Add PhaseHeader" --details "Implemented the header component..."`
- `exo impl log list`: Lists current entries.
- `exo impl log update <index>`: Updates an existing entry.

### 2. `exo ai`

A new subcommand to assist AI agents and users in gathering context.

**Commands:**

- `exo ai context`: Dumps the project context.
  - **Dynamic Behavior**: By default, this command is context-aware. It checks the current phase in `plan.toml` and prioritizes files relevant to that phase (e.g., files mentioned in `implementation-plan.toml`).
  - Flags:
    - `--focus <architecture|ui|testing>`: Explicitly narrows the dump to a specific domain.
    - `--full`: Ignores phase context and dumps the entire "Core Context" (Manual + Axioms).
- `exo ai prompt <name>`: Generates a prompt from `docs/agent-context/prompts.toml` or `.github/prompts/`.
  - Example: `exo ai prompt phase-start`

### 3. `exo task`

A streamlined alias/wrapper around `exo plan` for managing the _current phase's_ task list.

**Commands:**

- `exo task add "Title"`: Adds a task to the current phase.
- `exo task done <id>`: Marks a task as completed.
- `exo task list`: Lists tasks for the current phase.

### 4. `exo verify`

A unified entry point for all verification checks.

**Commands:**

- `exo verify`: Runs all standard checks (TOML validity, compilation, basic tests).
- `exo verify --full`: Runs E2E tests and other expensive checks.
- `exo verify phase`: Runs the specific checks required to finish the current phase (replacing `verify-phase.sh`).

## User Experience (UX)

**Scenario: Implementing a Feature**

1.  **Start**: `exo phase start 25`
2.  **Plan**: `exo task add "Implement Header"`
3.  **Work**: (Write code)
4.  **Document**: `exo impl log add --type feat --description "Header" --details "..."`
5.  **Verify**: `exo verify`
6.  **Finish**: `exo task done 1` -> `exo phase finish`

## Architecture

The `exo` CLI is built in Rust (`crates/exosuit-core` or similar). These new commands will be implemented as subcommands in the existing `clap` structure.

- **Implementation Plan Log**: Will use `toml_edit` (or similar) to preserve comments and formatting when modifying `implementation-plan.toml`.
- **AI**: Will port the logic from `scripts/context-dump.ts` to Rust. This is a net benefit as it centralizes the logic and makes it available without a Node.js dependency in the shell.

## Implementation Plan (Stage 2)

- [ ] **Bootstrap**: Update `bootstrap.sh` (and `setup-dev-tools.sh`) to build and install the new `exo` CLI.
- [ ] **Core**: Implement `exo impl log` (add, list, update).
- [ ] **AI**: Port `context-dump.ts` to `exo ai context`.
- [ ] **Task**: Implement `exo task` aliases.
- [ ] **Verify**: Create `exo verify` wrapper.

## Drawbacks

- **Migration**: Existing scripts will need to be deprecated and users retrained.

## Alternatives

- **Keep Scripts**: Continue using `scripts/` for context and verification. (Rejected: Inconsistent UX).
- **Manual TOML**: Continue allowing manual edits. (Rejected: Prone to syntax errors and violates "System 1" reliability).

## Unresolved Questions

- **Rich Text Input**: How do we handle complex descriptions (like "Label Paragraphs" or multi-line details) in CLI flags?
  - _Proposal_:
    - **Humans**: Support an `--editor` flag to open `$EDITOR`.
    - **Agents**: Support reading from **stdin** or a `--file` argument. This is critical because agents cannot interact with TUI editors.

