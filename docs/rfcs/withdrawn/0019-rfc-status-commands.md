<!-- exo:19 ulid:01kg5kp2bvj9b2bfcwk7gkm0wa -->

# RFC 19: Consolidate Agent Workflow into rfc-status

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0019: Consolidate Agent Workflow into rfc-status

- **Superseded by**: RFC 0080


## Summary

Replace the current collection of ad-hoc bash scripts (`scripts/agent/*.sh`) and markdown prompts (`.github/prompts/*.md`) with a unified set of subcommands within the `rfc-status` tool.

## Motivation

- **Tooling Fragmentation**: Currently, the agent workflow relies on a mix of Bash scripts, Python scripts (implied), and raw Markdown prompts. This makes it hard to maintain and reason about the workflow as a whole.
- **Context Awareness**: Bash scripts have limited ability to parse and understand the structured context (TOML, Markdown frontmatter) compared to a strongly-typed Rust application.
- **Distribution**: `rfc-status` is already being compiled to WASM for VS Code integration. bundling the workflow logic into it ensures the extension and the CLI share the exact same logic.
- **"Dogfooding"**: We should use our own high-quality tools rather than relying on loose scripts.

## Detailed Design

### New Subcommands

The `rfc-status` tool (which might need a rename, e.g., to `exo` or `agent-cli`) will gain new subcommands to replace existing scripts:

| Current Script / Prompt                      | New Command                   | Description                                      |
| :------------------------------------------- | :---------------------------- | :----------------------------------------------- |
| `scripts/agent/restore-context.sh`           | `rfc-status context restore`  | Generates the context dump for the agent.        |
| `scripts/agent/context-delta.sh`             | `rfc-status context delta`    | Shows changes since the last checkpoint.         |
| `.github/prompts/phase-start.prompt.md`      | `rfc-status phase start`      | Generates the prompt to start a new phase.       |
| `.github/prompts/phase-status.prompt.md`     | `rfc-status phase status`     | Generates a status report for the current phase. |
| `.github/prompts/phase-transition.prompt.md` | `rfc-status phase transition` | Generates the prompt to transition phases.       |

### Structured Output & Composability (Nushell Philosophy)

Inspired by Nushell, all commands must fundamentally produce **structured data** (typed objects/tables) rather than unstructured text.

- **Data-First Architecture**: Internal command logic returns Rust structs (e.g., `Vec<Rfc>`, `ContextState`), never formatted strings.
- **Presentation Layer**: A distinct layer handles rendering the data based on the consumer:
  - **Human**: Pretty-printed tables, lists, or markdown (default).
  - **Machine/Agent**: JSON serialization (via `--json`).
- **Composability**: The architecture should support future enhancements for client-side filtering and sorting (e.g., `exo rfc list --sort stage --columns id,title`), treating the output as a queryable dataset.

### Context Linking & State Management

- **Explicit Linking Syntax**: Define a clear syntax for linking between agent context artifacts (e.g., `[[rfc:0028]]`, `[[decision:001]]`). The tool should validate these links and potentially expand them when generating context.
- **Referential Integrity**: The tool must support "Refactoring" commands (move, rename, remove) that:
  - **Rewrite Links**: Automatically update all `[[...]]` references when an artifact is moved or renamed.
  - **Check Integrity**: Warn or block deletion if other active artifacts link to the target.
- **State Modification**: The tool will be the sole writer for `docs/agent-context/`. Commands must be well-specified to modify the state (e.g., `exo phase start` creates the directory and files).

### Testing Strategy

- **End-to-End Testing**: We will implement rigorous E2E tests that verify the _full_ state and _full_ output of commands.
- **No Mocks/Snapshots**: Tests should not rely on mocks or fragile snapshots.
- **Descriptive Assertions**: If text contains dynamic values or repeated content, we will build test utilities to describe the expected structure programmatically, ensuring we catch missing lines, duplicates, or malformed output that snapshots might miss.

### Implementation Strategy

1.  **Port Logic**: Port the logic from the Bash scripts into Rust within the `tools/rfc-status` crate.
2.  **Template Rendering**: Embed the prompt templates into the Rust binary (or load them from `docs/agent-context/templates`) and render them with data populated from the parsed context.
3.  **Deprecation**: Remove the old scripts and prompts once the CLI commands are verified.

## Unresolved Questions

- **Renaming**: Should `rfc-status` be renamed to something more generic like `exo-cli` or `agent-cli` since it's doing more than just checking RFC status?
- **Prompt Storage**: Should prompts remain as Markdown files that the tool reads, or be embedded as string literals/resources in the binary? (Reading files allows for easier editing without recompiling).


