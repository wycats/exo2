<!-- exo:120 ulid:01kg5kp2h0w0rzbs2f2dhexyww -->

# RFC 120: RFC Lifecycle Management Tools (Authoring)

- **Supersedes**: RFC 0173, RFC 10095, RFC 10148



# RFC 0120: RFC Lifecycle Management Tools (Authoring)

## Summary

This RFC proposes a suite of CLI commands (`exo rfc create`, `exo rfc edit`, `exo rfc rename`, `exo rfc repair`) to handle the **Authoring** and **Modification** of RFCs. This establishes a strict protocol: **Agents MUST NOT edit RFC files directly.** They must use the `exo` tools to ensure schema validity, referential integrity, and correct file placement.

## Motivation

- **Schema Fragility**: Agents editing Markdown files often break the YAML frontmatter or invalidly format the `relations` block.
- **Referential Integrity**: Renaming an RFC file manually breaks links in other documents. A tool can handle the refactoring.
- **Process Enforcement**: The tool can enforce that you cannot modify a `Stage 4` (Stable) RFC without a specific override or process.
- **Agent Discipline**: By forbidding direct file edits, we force the agent to "think" in terms of semantic operations ("Update Status") rather than text operations ("Replace line 4").

## Detailed Design

### 1. The Toolset

We will implement the following subcommands in `exo`:

- **`exo rfc create <title>`**:
  - Creates a new Stage 0 RFC with the correct numbered filename handle (e.g., `00001-my-idea.md`).
  - Populates the standard template.
- **`exo rfc edit <id> [options]`**:
  - Updates metadata fields.
  - `--status <status>`
  - `--title <new-title>`
  - `--add-relation <id>:<type>`
- **`exo rfc rename <id>`**:
  - Renames the file to match the RFC title slug and current numbering policy.
  - Syncs Exo RFC metadata for the renamed path.
  - Does not rewrite cross-file links; that broader refactoring belongs in a future link-maintenance surface.
- **`exo rfc repair <id>`**:
  - Repairs filename identity and metadata path drift when an RFC file has been manually renamed or created with a non-canonical handle.
  - Repairs malformed or legacy Exo anchor comments, including anchors with missing ULIDs or the wrong RFC number.
  - Computes the canonical RFC filename from the visible repository numbering convention and the RFC title slug.
  - Relinks Exo metadata even when the file has already been moved to the expected path.
  - Refuses to mutate when multiple files are numerically equivalent, so ambiguous drift must be resolved deliberately.
  - `exo rfc status` and global verification report repair debt and recommend exactly `exo rfc repair <id>`; read commands tolerate drift but do not normalize files automatically.
- **`exo rfc promote <id>`**:
  - Moves the RFC to the next stage directory.
  - Assigns a Number ID (if moving to Stage 1).
  - Validates Entrance Criteria (e.g., "Does the required execution artifact exist in canonical state?").

### 2. The Agent Protocol

We will update `AGENTS.md` with a new protocol:

> **Protocol: The Bureaucrat**
>
> - **Identity is Sacred**: You MUST NOT edit the Exo anchor line (`<!-- exo:<n> ulid:<ulid> -->`) or filenames of RFCs directly. Use `exo rfc edit`, `exo rfc rename`, or `exo rfc repair`.
> - **Body is Free**: You MAY edit the Markdown body text below the Exo anchor and RFC heading directly using `replace_string_in_file` to fix typos or expand content.
> - **Reasoning**: This protects the "Knowledge Graph" (metadata/links) while allowing fluid writing.

### 3. VS Code Integration

- **Tool Registration**: These commands will be exposed through the Exo MCP command surface and related RFC tools.
- **UI**: The Sidebar can provide a form-based interface for these actions, calling the CLI under the hood.

### 4. Implemented Amendment: Explicit RFC Repair

Agents must not normalize RFC filenames by hand. If Exo detects an unexpected numeric width, slug drift, or stale metadata path, it reports the expected canonical path and steers the agent to `exo rfc repair <id>`. The repair command is the only supported filename-identity repair surface: it performs the move/relink as an intentional mutation and reports the old path, new path, and reason.

Malformed or legacy Exo anchors are also repair debt. If an RFC-looking Markdown file has a missing anchor ULID, an invalid anchor ULID, or an anchor RFC number that disagrees with the file/metadata identity, Exo reports the problem and recommends `exo rfc repair <id>`. Reads may surface that debt, but they must not rewrite anchors automatically; only the explicit repair command may stamp or relink the anchor.

## Drawbacks

- **Friction**: It is harder to just "fix a typo". (Mitigation: We might allow direct edits for the _body_ text, but enforce tooling for _metadata_ and _lifecycle_ events).

## Alternatives

- **Linter**: Let the agent edit files, but run a linter to catch errors. (Reactive, allows breakage).
- **Git Hooks**: Prevent committing bad RFCs. (Too late in the loop).
