<!-- exo:164 ulid:01kg5kp2k49pe27jnms37dk07p -->

# RFC 164: Directory-Based RFC Organization

- **Supersedes**: RFC 10085



# RFC 0164: Directory-Based RFC Organization

## Summary

Refactor the RFC directory structure to group RFCs by their stage (e.g., `docs/rfcs/stage-0/`, `docs/rfcs/stage-1/`). This replaces the current flat structure.

## Motivation

- **Navigability**: The flat list in `docs/rfcs/` mixes stable features, active drafts, and rough ideas, making it hard to scan the file tree for "what is happening now".
- **ID Conservation**: Stage 0 "Strawman" ideas are often fleeting. Assigning them a permanent global ID (`0022`) prematurely clutters the namespace.
- **Workflow Clarity**: Moving a file from `stage-0/` to `stage-1/` is a clear, tangible "promotion" action.

## Detailed Design

### Directory Structure

RFCs will be organized into subdirectories corresponding to their lifecycle stage:

```
docs/rfcs/
├── stage-0/          # Strawman (Rough ideas)
│   ├── 001-idea.md   # Local numbering
│   └── 002-foo.md
├── stage-1/          # Proposal (Accepted direction)
│   └── 0027-name.md  # Global numbering starts here
├── stage-2/          # Draft (Spec)
├── stage-3/          # Candidate (Implemented)
├── stage-4/          # Stable (Shipped)
└── withdrawn/        # Rejected or superseded ideas
    └── 001-bad-idea.md
```

### Numbering Strategy

1.  **Stage 0 (Local)**: RFCs in `stage-0/` use a **local, 3-digit sequence** (e.g., `001`, `002`). These numbers are temporary and reusable (or just monotonic local).
2.  **Stage 1+ (Global)**: When an RFC is promoted to Stage 1, it is assigned the next available **Global 4-digit ID** (e.g., `0028`). This ID persists as the file moves through `stage-2`, `stage-3`, and `stage-4`.
3.  **Withdrawn**: If a Stage 0 RFC is rejected, it moves to `withdrawn/` and keeps its local ID (or is renamed). If a Stage 1+ RFC is withdrawn, it moves to `withdrawn/` and keeps its global ID.

### Identity & Metadata

- **Slug as ID**: The text after the number (e.g., `directory-based-rfcs`) is the **Unique Slug**. It must be unique across the entire system.
- **Relationships**: RFCs must track their connections via frontmatter:
  ```yaml
  related:
    - native-task-list
    - rfc-status-commands
  ```
  _Note: These relationships are subject to periodic "Coherence Workflows" to ensure the graph remains meaningful._

### Unresolved Questions Lifecycle

Unresolved questions are not just text; they are **Stage Gates**.

1.  **Entry Check**: Before picking up an RFC, all existing unresolved questions must be reviewed.
2.  **Transition Check**: A stage transition (e.g., 0 -> 1) cannot occur unless:
    - The question is resolved, OR
    - It is explicitly decided that the question can be carried forward to the next stage (i.e., "We don't need this answer _yet_").
3.  **Refinement**: If carried forward, the question must be updated with new learnings from the current phase.

### Tooling Updates

- **`rfc-status`**: Must be updated to:
  - Scan subdirectories recursively or explicitly.
  - Infer `stage` from the parent directory name (overriding or validating frontmatter).
  - Handle the two different numbering schemes (Local vs Global).
- **Refactoring Commands**: The tool will provide commands to move RFCs (e.g., `exo rfc promote 001`) which automatically:
  - Moves the file to the correct directory.
  - Updates the ID (if promoting to Stage 1).
  - Rewrites links in other files to point to the new location/ID.

## Unresolved Questions

- **Archive**: Should `stage-4` be the archive, or do we keep a separate `archive/` for rejected RFCs? (Presumably `stage-4` is for _active/stable_ features, `archive` is for _rejected/replaced_).


