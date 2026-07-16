<!-- exo:159 ulid:01kg5m2yg0zrjdxayqmnb8qbza -->

# RFC 159: The `ai` Subcommand Pattern

- **Supersedes**: RFC 10081


- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0159: The `ai` Subcommand Pattern

**Context**: Tooling / DX
**Created**: 2025-12-05

## 1. Motivation

CLI tools are typically designed for human consumption (formatted tables, colors, interactive prompts) or machine consumption (JSON, strict exit codes).

AI Agents occupy a middle ground:

1.  They need **structured data** (like machines) to avoid parsing errors.
2.  They need **semantic context** (like humans) to understand _what_ the data means and _how_ to use the tool.
3.  They benefit from **concise summaries** to save context window tokens.

Currently, we force Agents to guess flags (`--json`, `--help`) or parse human output. We propose a standardized `ai` subcommand for our internal tools.

## 2. The Pattern

Any CLI tool in the Exosuit workspace SHOULD implement an `ai` subcommand.

```bash
$ my-tool ai [context]
```

### 2.1 Output Format

The output MUST be a Markdown fragment designed for direct injection into the Agent's context window.

```markdown
# Tool: my-tool

## Capabilities

- `my-tool list`: Lists items.
- `my-tool check`: Verifies integrity.

## Current State

- **Status**: Healthy
- **Pending Items**: 3

## Usage Hints

- Use `list --json` to get the raw data for processing.
- Run `check` before committing changes.
```

### 2.2 Sub-contexts

The `ai` command MAY accept arguments to provide context-specific guidance.

- `$ rfc-status ai`: Overview of the RFC process and current stats.
- `$ rfc-status ai list`: A token-optimized list of active RFCs.

## 3. Implementation in `rfc-status`

We will implement this pattern immediately in the `rfc-status` tool.

### 3.1 `rfc-status ai`

Output:

- **Philosophy**: Brief explanation of the Staged RFC process (0-4).
- **Directory Structure**: Explanation of `docs/rfcs/stage-X`.
- **Summary**: Count of RFCs in each stage.
- **Next Steps**: Instructions on how to create or promote an RFC.

## 4. Standardization & Evangelism

We believe this pattern has value beyond Exosuit. Just as `llms.txt` provides a standard for **static context**, the `ai` subcommand could become a standard for **dynamic context**.

We intend to formalize this into a standalone specification that can be evangelized to the wider developer tools community.

### 4.1 Open Questions

- **Naming**: Is `ai` the right subcommand? (vs `agent`, `context`, `llm`).
- **Prior Art**: We need to research existing CLI standards for machine readability to ensure we aren't reinventing the wheel or ignoring better precedents.

## 5. Workflow Implications

### 5.1 Renumbering on Promotion

Stage 0 RFCs are numbered sequentially (000, 001, etc.) within the `stage-0` directory.
**Crucially**, when an RFC is promoted to **Stage 1 (Proposal)**, it MUST be renumbered to the next available global RFC number (e.g., `0005-ai-subcommand.md`) and moved to the root `docs/rfcs/` directory (or a `stage-1` folder if we adopt that structure fully).

This ensures that "Official" RFCs have stable, permanent IDs, while Strawman proposals can be churned or discarded without burning official numbers.

### 5.2 Stage Definitions & Transitions

| Stage             | Meaning                         | Workflow Trigger                                                       |
| :---------------- | :------------------------------ | :--------------------------------------------------------------------- |
| **0 (Strawman)**  | "I have an idea."               | Create file in `docs/rfcs/stage-0/`. No commitment to implement.       |
| **1 (Proposal)**  | "We agree this is worth doing." | Move to `docs/rfcs/`, assign permanent ID. High-level design approved. |
| **2 (Draft)**     | "Here is exactly how it works." | Detailed spec written. Ready for `implementation-plan.md`.             |
| **3 (Candidate)** | "It is built."                  | Implementation complete. Undergoing testing.                           |
| **4 (Stable)**    | "It is shipped."                | Feature is live/stable. Spec is canonical.                             |

