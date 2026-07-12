<!-- exo:10128 ulid:01kmzxey0jhq2sby6rxcgbk2g4 -->


# RFC 10128: Structured IO CLI

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**: Withdrawn by RFC 10180 storage disposition: file-backed phase context, docs/agent-context/current artifacts, and docs/agent-context/archive phase snapshots are retired.

- **Superseded by**: RFC 10014


## Summary

This RFC proposes extending the `exo` CLI to support structured manipulation of key project files, specifically `docs/agent-context/ideas.toml`, `docs/agent-context/plan.toml`, and `docs/agent-context/current/implementation-plan.toml` (with legacy projection artifacts only during migration/back-compat; see RFC 10028). This enables the agent to perform "Structured File Manipulation" (as per the "Bureaucrat" pattern) rather than risky "Text Surgery" (regex/string replace), ensuring data integrity and adherence to Axiom 16 (Tool-Mediated Mutations).

## Motivation

### The "Text Surgery" Problem

LLMs struggle with precise edits to large structured files (TOML, JSON, YAML). Common failure modes include:

- **Syntax Errors**: Breaking the TOML structure (e.g., invalid arrays, missing quotes).
- **Context Loss**: Accidentally deleting unrelated sections when trying to append.
- **Hallucination**: Inventing keys or structures that don't exist in the schema.
- **Indentation Hell**: Messing up nested structures.

### Axiom 16: Tool-Mediated Mutations

We have established Axiom 16, which states that complex state mutations should be performed via CLI tools, not raw file edits. This RFC implements the tooling required to uphold this axiom for critical workflows: **Idea Management**, **Plan Management**, **Task Tracking**, **Execution Log Recording**, and **Implementation Planning**.

### Steering & Guardrails

By forcing the agent to use `exo idea add` or `exo plan add-task`, we:

1.  **Validate Input**: The CLI ensures required fields are present and correctly typed.
2.  **Enforce Schema**: The CLI guarantees the output matches the defined TOML schema.
3.  **Provide Feedback**: The CLI returns success/failure messages that guide the agent.

## Detailed Design

### 1. Idea Management (`exo idea`)

The `exo idea` command manages the `docs/agent-context/ideas.toml` file.

#### Commands

- `exo idea add --title <TITLE> --description <DESC> --tags <TAGS>`
  - Adds a new idea with a generated UUID, timestamp, and "new" status.
  - `tags` is a comma-separated list.
- `exo idea list`
  - Lists all ideas with their ID, title, and status.

#### Data Model (`Idea`)

```rust
pub struct Idea {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String, // "new", "accepted", "rejected", "implemented"
    pub created_at: String,
    pub source: String, // "user" or "agent"
    pub tags: Vec<String>,
    pub related_tasks: Vec<String>,
}
```

### 2. Plan Management (`exo plan`)

The `exo plan` command manages the `docs/agent-context/plan.toml` file.

#### Commands

- `exo plan add-epoch --id <ID> --title <TITLE>`
  - Appends a new Epoch.
- `exo plan add-phase --epoch-id <EPOCH_ID> --id <ID> --title <TITLE>`
  - Appends a new Phase to the specified Epoch.
- `exo plan add-task --phase-id <PHASE_ID> --id <ID> --label <LABEL>`
  - Appends a new Task to the specified Phase.
- `exo plan update-status --id <ID> --status <STATUS>`
  - Updates the status of an Epoch, Phase, or Task.
  - Searches recursively for the ID.
- `exo plan review [--fix]`
  - Analyzes the plan for staleness, non-linearity, and orphans.
  - Interactive mode allows "Bankrupting" or "Rescheduling" items.

#### Data Model (`ExoState`)

The existing `ExoState`, `Epoch`, `Phase`, and `Task` structs in `context.rs` are reused and extended (e.g., adding `rfcs` to `Phase`).

### 3. Implementation Plan Management (`exo impl`)

The `exo impl` command manages the `docs/agent-context/current/implementation-plan.toml` file.

#### Commands

- `exo impl add-step <ID> <TITLE> --type <TYPE> --description <DESC> --files <FILES> --tests <TESTS> --no-test-reason <REASON>`
  - Adds a new step to the implementation plan.
  - `type` is one of: `feat`, `fix`, `chore`, `docs`, `refactor`, `style`, `test` (default: `feat`).
  - `files` is a comma-separated list of files to be modified.
  - `tests` is a comma-separated list of test files to be created or modified.
  - `no-test-reason` is a string explaining why no tests are included.
  - **Constraint (Strict TDD)**: For `feat`, `fix`, and `refactor`, either `--tests` OR `--no-test-reason` MUST be provided.
  - **Constraint (Relaxed TDD)**: For `chore`, `docs`, `style`, and `test`, tests are optional and no reason is required if omitted.
- `exo impl clear-steps`
  - Clears all steps from the implementation plan (useful for resetting or starting fresh).

### 4. Task Management (`exo task`)

The `exo task` command manages phase tasks in the canonical phase execution
artifact (`docs/agent-context/current/implementation-plan.toml`).

Legacy projection snapshots are deprecated
(migration-only) and must not be treated as the source of truth.

#### Commands

- `exo task add --id <ID> --label <LABEL>`
  - Adds a single task to the active change/task list inside the implementation plan.
- `exo task list`
  - Lists all tasks for the current phase.
- `exo task complete --id <ID>`
  - Marks a task as completed.
- `exo task init`
  - (DEPRECATED) Regenerates the legacy task-list projection snapshot
    from canonical context for migration/back-compat.

### 5. Execution Log / Walkthrough (`exo impl log`)

The phase execution artifact also contains the canonical log/journal for the
current phase.

Legacy projection snapshots are deprecated
(migration-only) and should not be required in normal workflows.

#### Commands

- `exo impl log add --type <TYPE> --description <DESC> --details <DETAILS>`
  - Adds a new log entry to the implementation plan.
- `exo impl log list`
  - Lists current log entries.
- `exo impl log clear`
  - Clears log entries (rare; usually you keep history).

### 6. Phase Initialization (`exo phase start`)

The `exo phase start` command is enhanced to initialize the phase execution
artifact for the new phase:

- **Implementation Plan**: Initialized with a template containing the phase title
  and RFCs.

Deprecated projection artifacts must not be
created or required in normal workflows. If they exist, tooling may regenerate
or surface them only for migration/back-compat.

### 7. Steering Messages

To ensure the agent uses these tools, we will update the system prompt or
"Steering" messages in the `exo` tool output.

**Example Steering Message (in `AGENTS.md` or System Prompt):**

> **Tool Usage Rule**: When adding ideas or modifying the plan, you MUST use the
> `exo` CLI tools (`exo idea`, `exo plan`, `exo impl`, `exo task`). DO NOT edit
> TOML files directly. Decisions should be captured as RFCs under `docs/rfcs/`.


## Implementation Details

- **Language**: Rust (in `tools/exo`).
- **Libraries**: `clap` (CLI), `toml` (Serialization), `serde` (Data Model), `anyhow` (Error Handling), `toml_edit` (Preserving comments/formatting where possible).
- **Location**: `tools/exo/src/idea.rs`, `tools/exo/src/plan.rs`, `tools/exo/src/implementation.rs`, `tools/exo/src/task.rs`, `tools/exo/src/walkthrough.rs`.

## Drawbacks

- **Maintenance**: The CLI code must be kept in sync with the TOML schema. If the schema changes, the CLI must be updated.
- **Rigidity**: The CLI might not support every possible edge case of TOML editing.

## Alternatives

- **Schema Validation Script**: Let the agent edit the file, but run a validation script afterwards. _Rejected: Doesn't prevent the initial error, just catches it._
- **JSON Schema**: Use JSON Schema to guide the LLM. _Rejected: Still relies on the LLM to generate correct JSON/TOML, which is error-prone for large files._

## Future Possibilities

- **Interactive Mode**: `exo plan edit` could open a TUI or launch an editor for specific fields.
- **Rich Text Support**: Better handling of multiline strings in the CLI (maybe opening `$EDITOR`).
- **Integration with VS Code**: The VS Code extension could wrap these CLI commands to provide a GUI for plan management.
