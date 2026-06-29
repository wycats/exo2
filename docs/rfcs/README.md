# RFCs

This directory contains the immutable history of decisions for the Exosuit project.
See `docs/manual` for the current state of the system.

> **⚠️ This directory is managed by `exo`. Do not create or edit files directly.**
>
> Use `exo rfc create`, `exo rfc show`, `exo rfc promote` instead.
> See [0000-template.md](0000-template.md) for details.

## The Staged RFC Process

Exosuit uses a Staged RFC process to manage architectural decisions and feature planning. This process consolidates free-form designs, implementation plans, and decision records into a single unified workflow.

### CLI Commands

```bash
exo rfc create --title "Title" --feature "name"  # Create new RFC (Stage 0)
exo rfc list                                      # List RFCs by stage
exo rfc show <id>                                 # Show RFC details
exo rfc promote <id>                              # Promote to next stage
```

### Directory Structure

RFCs are organized into subdirectories corresponding to their lifecycle stage:

- `stage-0/`: **Strawman** (Rough ideas). Local numbering (e.g., `001-idea.md`).
- `stage-1/`: **Proposal** (Accepted direction). Global numbering (e.g., `0028-feature.md`).
- `stage-2/`: **Draft** (Detailed spec).
- `stage-3/`: **Candidate** (Implemented).
- `stage-4/`: **Stable** (Shipped).
- `withdrawn/`: Rejected or superseded ideas.

### Stage Definitions

| Stage | Name          | Definition                      | Workflow Trigger                                                            |
| :---- | :------------ | :------------------------------ | :-------------------------------------------------------------------------- |
| **0** | **Strawman**  | "I have an idea."               | Create file in `docs/rfcs/stage-0/` with next available local ID.           |
| **1** | **Proposal**  | "We agree this is worth doing." | Move to `docs/rfcs/stage-1/`, assign global ID. High-level design approved. |
| **2** | **Draft**     | "Here is exactly how it works." | Move to `docs/rfcs/stage-2/`. Detailed spec written.                        |
| **3** | **Candidate** | "It is built."                  | Move to `docs/rfcs/stage-3/`. Implementation complete.                      |
| **4** | **Stable**    | "It is shipped."                | Move to `docs/rfcs/stage-4/`. Feature is live/stable. Spec is canonical.    |

### Workflow Rules

1.  **Directory-Based**: The directory determines the stage. Frontmatter is optional but recommended for metadata.
2.  **Numbering**:
    - **Stage 0**: Local, 3-digit sequence (e.g., `001`). Reusable/temporary.
    - **Stage 1+**: Global, 4-digit sequence (e.g., `0028`). Permanent.
3.  **Single Source of Truth**: Once an RFC reaches Stage 4, the corresponding documentation in `docs/manual` becomes the source of truth. The RFC remains as a historical record.
