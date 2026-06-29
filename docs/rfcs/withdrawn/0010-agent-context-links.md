<!-- exo:10 ulid:01kg5kp2bcj6q0sewecqza56qf -->

# RFC 10: Dedicated Format for Agent Context Links


# RFC 0010: Dedicated Format for Agent Context Links

## Summary

Establish a dedicated syntax and resolution strategy for internal links within Agent Context files, RFCs, and the Manual. This system will allow unambiguous references to semantic entities (like "RFC 12", "The Architecture Axioms", "Phase 1 Plan") rather than just file paths.

## Motivation

The current documentation structure relies on relative file paths (e.g., `../rfcs/stage-1/0012.md`). This is fragile because:
1.  **Refactoring breaks links**: Moving a file requires updating all back-links.
2.  **Cognitive load**: Agents and humans have to calculate relative paths.
3.  **Lack of Semantics**: A link to a file doesn't explicitly state *what* it is referencing (e.g., is it the *current* state of the plan, or a historical snapshot?).

A dedicated format will enable:
-   **Stable References**: Link to "RFC 12" regardless of its stage directory.
-   **Tooling Support**: CLI tools can validate links, generate graphs, and resolve references for the agent.
-   **Semantic Clarity**: Explicitly distinguish between referencing a *Decision*, a *Task*, or a *Concept*.

## Detailed Design

### Proposed Syntax

We propose using a URI-like scheme or a tag-based syntax that resolves to the correct file location at runtime or compile time.

#### Option 1: Custom URI Scheme (Recommended)

Use standard Markdown links with a custom scheme:

-   `[RFC 12](../stage-3/0012-externalized-prompts.md)` -> Resolves to `docs/rfcs/stage-X/0012-name.md`
-   `[Architecture Axioms](../../manual/architecture/axioms.md)`
-   `[Current Plan](../../agent-context/plan.toml)`

#### Option 2: Wiki-style Links

-   `[[rfc:12]]`
-   `[[decision:use-rust]]`

### Resolution Logic

The `exo` tool will be responsible for resolving these references.

1.  **RFCs**: Look up by ID in the RFC index.
2.  **Manual**: Look up by logical path in the manual structure.
3.  **Context**: Look up well-known context files (plan, decisions, etc.).

### User Experience (UX)

-   **Writing**: Users write `[RFC 12](../stage-3/0012-externalized-prompts.md)`.
-   **Reading (VS Code)**: A VS Code extension (or the existing one) intercepts these links and opens the correct file.
-   **Reading (Agent)**: The agent receives the resolved path or the content directly when requesting context.

## Drawbacks

-   **Non-Standard**: Standard Markdown viewers (GitHub web UI) won't understand `exo:` links without a build step or pre-processing.
-   **Tooling Dependency**: Requires the `exo` tool or extension to be functional for navigation.

## Alternatives

-   **Strict Relative Paths**: Enforce strict discipline on relative paths (status quo).
-   **Symlinks**: Use a `docs/latest/` symlink structure to stabilize paths.

## Unresolved Questions

-   How do we handle links in the GitHub UI? Should we have a pre-commit hook that "compiles" them to relative links?
-   Should we support fragment identifiers (e.g., `exo:rfc/12#detailed-design`)?


