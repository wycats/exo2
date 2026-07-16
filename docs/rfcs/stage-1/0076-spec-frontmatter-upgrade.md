<!-- exo:76 ulid:01kg5kp2epkhwz0v4826a6gpcd -->

# RFC 76: Formal Spec Frontmatter Upgrade

- **Supersedes**: RFC 10127



# RFC: Formal Spec Frontmatter Upgrade

## Summary

This RFC proposes upgrading the existing Formal Specifications in `docs/specs/` to use the standardized YAML frontmatter schema defined in the **Copilot Resources** RFC. This will transform them from "floating text files" into first-class nodes in the Exosuit Knowledge Graph, capable of being indexed, linked, and validated by our tooling.

## Motivation

- **Inconsistency**: Currently, specs use ad-hoc header blocks (e.g., `**Status**: Draft`, `**Context**: ...`) which are not machine-readable.
- **Invisibility**: Because they lack standard metadata, they are invisible to the proposed `exo` tooling and the Copilot Resource Provider.
- **Disconnection**: There is no formal link between a Spec (The Building Code) and the RFC that authorized it (The Law).

## Detailed Design

### 1. The Schema

We will apply the standard Exosuit Frontmatter schema to all files in `docs/specs/`.

```yaml
---
title: VS Code Chat Object Model (VCOM)
type: spec
status: candidate # draft | candidate | stable | deprecated
version: 2.2.0
relations:
  - id: 0004
    type: implements
    description: "Formalizes the Rich Context Editors vision."
  - id: literate-kernel
    type: depends-on
    description: "Consumes tokens from the Literate Kernel."
---
```

### 2. The Migration Strategy

We will migrate the following existing specs:

- `docs/specs/architecture.md` (RTD Architecture)
- `docs/specs/vcom/spec.md` (VCOM)
- `docs/specs/rsl-spec.md` (RSL)
- `docs/specs/literate-kernel/spec.md` (Literate Kernel)
- `docs/specs/rich-context-editors/spec.md` (Rich Context Editors)

### 3. Metadata Mapping

We will map existing ad-hoc headers to the new schema:

| Legacy Header | New Frontmatter Field                         |
| :------------ | :-------------------------------------------- |
| `# Title`     | `title`                                       |
| `**Status**`  | `status` (normalized to lowercase)            |
| `**Version**` | `version`                                     |
| `**Context**` | `relations` (type: `related` or `depends-on`) |

### 4. Inline Citations (Footnotes)

To maintain readability while ensuring rigorous traceability, Specs and the Manual SHOULD use **GFM Footnotes** to cite the RFCs that authorize specific behaviors.

- **Syntax**: Use standard Markdown footnotes `[^id]`.
- **URI Scheme**: The footnote definition MUST use the `exo:` URI scheme defined in **RFC 0008**.
- **Scope**: This requirement applies to:
  - **Specs**: Linking to the RFCs that defined the feature.
  - **The Manual**: Linking to the RFCs that established the "Law".

**Example:**

```markdown
The parser uses a recursive descent algorithm[^rfc-12] to handle nested structures.

[^rfc-12]: [RFC 0012: Parser Architecture](../stage-3/0012-externalized-prompts.md)
```

### 5. Tooling Integration

The `exo` CLI will be updated to enforce this rigor:

- **Copilot Resources**: The resource provider will be updated to scan `docs/specs/` and index files with `type: spec`.
- **URI Scheme**: These resources will be addressable via `exo://spec/<filename-stem>` (e.g., `exo://spec/vcom`).
- **Link Validation**: `exo check coherence` will:
  - Parse all footnotes in Specs and the Manual.
  - Verify that every footnote definition resolves to a valid `exo:` resource (RFC, Spec, or Manual page).
  - Warn on "Orphan Footnotes" (defined but not used) or "Missing Definitions" (used but not defined).

## Drawbacks

- Requires touching multiple files.
- Might break any existing (fragile) scripts that parse the raw markdown headers (though none are known).

## Alternatives

- **Do Nothing**: Specs remain second-class citizens.
- **Separate Metadata File**: Keep metadata in a `specs.toml` file. Rejected because we prefer "Self-Contained Documents" (Axiom: Context is King).

