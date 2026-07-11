<!-- exo:10150 ulid:01kmzxefd5wqyq4cvtreebkrjh -->


# RFC 10150: Structured Context Criteria (TOML vs Markdown)

- **Status**: Withdrawn
- **Stage**: 3
- **Reason**:

## Problem

The `docs/agent-context` directory currently contains a mix of TOML files (`plan.toml`, `axioms.*.toml`) and Markdown files (`walkthrough.md`, `task-list.md`). As we migrate more content to structured formats (like `modes.toml`), we need a clear set of criteria to decide when a piece of context should be **Structured (TOML)** versus **Narrative (Markdown)**.

Without clear rules, we risk:

1.  **Over-engineering**: Forcing free-form text into rigid TOML strings.
2.  **Under-engineering**: Keeping queryable data in hard-to-parse Markdown lists.
3.  **Inconsistency**: Similar concepts stored in different formats.

## Proposal

We propose the following heuristic: **"If the Agent needs to query it, structure it. If the User needs to read it, narrate it."**

### Criteria for TOML (Structured Data)

Use TOML when the information:

1.  **Has a Schema**: The data fits a strict Zod schema (e.g., `id`, `title`, `status`).
2.  **Is Queryable**: The Agent needs to look up specific items by ID or tag (e.g., "Find Axiom #1").
3.  **Is Aggregated**: The data is meant to be displayed in a UI dashboard or list view.
4.  **Is Mutable by Tooling**: Scripts or CLI tools need to update specific fields without parsing natural language.

**Examples**:

- `plan.toml`: Tasks have states (`[ ]`, `[x]`) and IDs.
- `axioms.*.toml`: Axioms have IDs and tags.
- `decisions.toml`: Decisions have dates and statuses.
- `modes.toml`: Modes have distinct attributes (Focus, Mindset).

### Criteria for Markdown (Narrative)

Use Markdown when the information:

1.  **Is Linear**: The content is meant to be read from top to bottom.
2.  **Is Free-form**: The structure varies significantly between entries.
3.  **Is Explanatory**: The primary goal is to explain "Why" or "How" in natural language.
4.  **Is Ephemeral**: The document is a temporary artifact (e.g., a scratchpad).

**Examples**:

- `walkthrough.toml` (Wait, this is TOML but contains Markdown strings. This is a hybrid).
- `docs/manual/*.md`: The authoritative documentation.
- `docs/rfcs/*.md`: Historical context and argumentation.
- `docs/vision.md`: High-level philosophy.

## The "Structured Narrative" Pattern

Embedding Markdown within TOML multi-line strings is not a compromise; it is a deliberate architectural choice that preserves **Conceptual Integrity** by separating **Structure** from **Content**.

- **TOML (The Container)**: Handles metadata, relationships, and queryability. It is the "Card Catalog" that allows the Agent to index and retrieve information efficiently.
- **Markdown (The Payload)**: Handles the human-readable narrative. It is the "Book Content" that allows for expressive communication.

This pattern ensures that context is both **Computable** (for the Agent) and **Comprehensible** (for the User).

```toml
[entry]
title = "My Update" # Structured Metadata (Queryable)
content = """
This is a **markdown** block inside TOML.
It allows for *nuance* and formatting that pure data structures lack.
""" # Narrative Payload (Readable)
```

## Decision Matrix

| Feature              | Use TOML           | Use Markdown     |
| :------------------- | :----------------- | :--------------- |
| **Primary Consumer** | Agent / CLI / UI   | Human Reader     |
| **Structure**        | Rigid (Schema)     | Flexible (Prose) |
| **Updates**          | Field-level (CRUD) | Append / Rewrite |
| **Validation**       | Strict (Zod)       | Loose (Linting)  |

## Next Steps

1.  Adopt this RFC as a guideline.
2.  Audit `docs/agent-context` for compliance.
3.  **Execute Phase 32: Context Cleanup**:
    - Delete `decisions.md` (replaced by `decisions.toml`).
    - Delete `plan-outline.test.md` (test artifact).
    - Delete `EXOSUIT.md` (redundant).
    - Move `markdown-spec.md` to `docs/specs/context-markdown.md`.
    - Delete `plan-outline.md` (replaced by `plan.toml`).
