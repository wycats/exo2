<!-- exo:173 ulid:01kg5kp2khetwmjycakrvvqt7c -->

# RFC 173: RFC Tooling Completion


# RFC: RFC Tooling Completion

## Summary

This RFC proposes to complete the implementation of the `exo rfc` command suite by adding the missing `promote`, `edit`, and `withdraw` commands, which are essential for a fully read-only context workflow.

## Motivation

A recent "Gap Analysis" revealed a discrepancy between the "Ideal Tool" (and the promises of RFC 0008) and the "Current Reality" of the `exo` CLI.

- **Missing `promote`**: Users/Agents cannot move RFCs through the lifecycle without file system hacks.
- **Missing `edit`**: There is no way to update metadata (title, feature) safely.
- **Missing `withdraw`**: There is no standard way to archive failed ideas.

To enforce the "Read-Only Context" protocol, these tools must exist.

## Detailed Design

### 1. `exo rfc promote <id>`

- **Action**: Moves the RFC file from `stage-N` to `stage-(N+1)`.
- **Logic**:
  - Validates current stage.
  - Updates the `stage` field in the frontmatter.
  - Moves the file.
  - **Constraint**: Cannot promote from Stage 4.

### 2. `exo rfc edit <id> [options]`

- **Options**:
  - `--title <new_title>`
  - `--feature <new_feature>`
  - `--status <new_status>` (Manual override, use with caution)
- **Action**: Parses the file, updates the frontmatter/metadata, and writes it back.

### 3. `exo rfc withdraw <id>`

- **Action**: Moves the RFC file to `docs/rfcs/withdrawn/`.
- **Logic**:
  - Updates status to "Withdrawn".
  - Moves file.

### 4. `exo rfc rename <id> <new_slug>` (Optional/Future)

- **Action**: Renames the file on disk.
- **Note**: Full link updating is out of scope for this iteration but desirable.

## Implementation Plan

1.  **Refactor `rfc.rs`**: Ensure `promote` logic is robust and exposed.
2.  **Implement `edit`**: Add frontmatter parsing/updating logic (using `gray_matter` or regex).
3.  **Implement `withdraw`**: Add file move logic.
4.  **Wire up CLI**: Update `main.rs` to expose these commands.

