<!-- exo:10148 ulid:01kmzxefd19dbvqc4j3208f8w3 -->


# RFC 10148: RFC Lifecycle Management Tools (Authoring)

- **Superseded by**: RFC 0120


- **Status**: Withdrawn
- **Stage**: 3
- **Reason**:

## Summary

This RFC proposes a suite of CLI commands (`exo rfc new`, `exo rfc edit`, `exo rfc rename`) to handle the **Authoring** and **Modification** of RFCs. This establishes a strict protocol: **Agents MUST NOT edit RFC files directly.** They must use the `exo` tools to ensure schema validity, referential integrity, and correct file placement.

## Motivation

- **Schema Fragility**: Agents editing Markdown files often break the YAML frontmatter or invalidly format the `relations` block.
- **Referential Integrity**: Renaming an RFC file manually breaks links in other documents. A tool can handle the refactoring.
- **Process Enforcement**: The tool can enforce that you cannot modify a `Stage 4` (Stable) RFC without a specific override or process.
- **Agent Discipline**: By forbidding direct file edits, we force the agent to "think" in terms of semantic operations ("Update Status") rather than text operations ("Replace line 4").

## Detailed Design

### 1. The Toolset

We will implement the following subcommands in `exo`:

- **`exo rfc new <title>`**:
  - Creates a new Stage 0 RFC with the correct filename handle (e.g., `my-idea.md`).
  - Populates the standard template.
- **`exo rfc edit <id> [options]`**:
  - Updates metadata fields.
  - `--status <status>`
  - `--title <new-title>`
  - `--add-relation <id>:<type>`
- **`exo rfc rename <id> <new-handle>`**:
  - Renames the file.
  - **Crucially**: Scans the workspace and updates all `exo:` links and footnotes that reference the old ID.
- **`exo rfc promote <id>`**:
  - Moves the RFC to the next stage directory.
  - Assigns a Number ID (if moving to Stage 1).
  - Validates Entrance Criteria (e.g., "Does `implementation-plan.toml` exist?").

### 2. The Agent Protocol

We will update `AGENTS.md` with a new protocol:

> **Protocol: The Bureaucrat**
>
> - **Metadata is Sacred**: You MUST NOT edit the YAML frontmatter or filenames of RFCs directly. Use `exo rfc edit` or `exo rfc rename`.
> - **Body is Free**: You MAY edit the Markdown body text (after the second `---`) directly using `replace_string_in_file` to fix typos or expand content.
> - **Reasoning**: This protects the "Knowledge Graph" (metadata/links) while allowing fluid writing.

### 3. VS Code Integration

- **Tool Registration**: These commands will be exposed as `exosuit_rfc_new`, `exosuit_rfc_edit` tools (or via the `exo run` shell pattern).
- **UI**: The Sidebar can provide a form-based interface for these actions, calling the CLI under the hood.

## Drawbacks

- **Friction**: It is harder to just "fix a typo". (Mitigation: We might allow direct edits for the _body_ text, but enforce tooling for _metadata_ and _lifecycle_ events).

## Alternatives

- **Linter**: Let the agent edit files, but run a linter to catch errors. (Reactive, allows breakage).
- **Git Hooks**: Prevent committing bad RFCs. (Too late in the loop).
