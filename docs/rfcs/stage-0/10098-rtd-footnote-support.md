<!-- exo:10098 ulid:01kmzxeffbrsj9r9neec81hx4v -->


# RFC 10098: RTD Footnote Support

- **Superseded by**: RFC 0175


## Summary

This RFC proposes adding support for **GitHub Flavored Markdown (GFM) Footnotes** to the Rich Text Document (RTD) specification. This will allow the `exo` parser to recognize, validate, and render footnotes, enabling the rigorous citation system required by the **Formal Spec Frontmatter Upgrade**.

## Motivation

- **Citation Rigor**: We have decided (in the Spec Upgrade RFC) to use footnotes for linking Specs/Manuals to RFCs.
- **Current Limitation**: The current RTD parser (Layer 1) does not explicitly support the `[^1]` and `[^1]: ...` syntax. It treats them as plain text.
- **Readability**: Footnotes allow us to keep the main text clean while providing precise, machine-readable references at the bottom of the document.

## Detailed Design

### 1. Syntax Extension

We will adopt the standard GFM syntax:

```markdown
Here is a statement[^1].

[^1]: This is the citation.
```

### 2. Object Model (Layer 0)

We will extend the RTOM (Rich Text Object Model) to include:

- **`Inline.FootnoteReference`**: Represents the `[^1]` marker in the text.
  - `id`: string (e.g., "1")
- **`Block.FootnoteDefinition`**: Represents the `[^1]: ...` block.
  - `id`: string
  - `content`: RTOM (The definition body)

### 3. Parsing Logic (Layer 1)

The `exosuit-rtd` parser will be updated to:

1.  **Tokenize**: Recognize `[^...]` patterns.
2.  **Collect**: Gather all definitions at the end of the parsing pass (or stream them if possible).
3.  **Link**: Associate references with definitions.

### 4. Rendering (Layer 3 / HTML)

- **References**: Render as `<sup><a href="#fn-1">1</a></sup>`.
- **Definitions**: Render as a `<section class="footnotes">` at the bottom of the document.

## Tooling Implications

- **Validation**: The parser must emit warnings for:
  - References to missing definitions.
  - Unused definitions.
- **Resolution**: The `exo` tool will use the parsed Footnote Definitions to extract the `exo:` links for the Knowledge Graph.
