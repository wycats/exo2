# Exosuit RTD Markdown Specification

This document defines the subset of Markdown supported by the Exosuit Rich Text Document (RTD) system. The goal is to provide a strict mapping between Markdown syntax and RTD nodes, ensuring consistent rendering across the Exosuit Studio.

## Philosophy

- **Restrictive**: Only Markdown features that map directly to RTD nodes are supported.
- **Graceful Degradation**: Unsupported features should fall back to plain text rather than breaking the renderer or disappearing.
- **Unified**: We use the `unified` ecosystem (remark) to parse Markdown.

## Supported Features

### Blocks

| Markdown | RTD Node | Notes |
| :--- | :--- | :--- |
| Paragraphs | `paragraph` | |
| Headings (`#` - `######`) | `heading` | Levels 1-6 |
| Lists (`-`, `1.`) | `list` | Ordered and Unordered. Supports nesting. |
| Task Lists (`- [ ]`) | `list` | `checked` property on items. |
| Code Blocks (```) | `code-block` | Language support via info string. |
| Blockquotes (`>`) | `blockquote` | Nested blocks supported. |
| Horizontal Rules (`---`) | `thematic-break` | |

### Inlines

| Markdown | RTD Node | Notes |
| :--- | :--- | :--- |
| Text | `text` | |
| Bold (`**`) | `strong` | |
| Italic (`*`, `_`) | `emphasis` | |
| Code (`` ` ``) | `code-span` | |
| Links (`[]()`) | `link` | |
| Images (`![]()`) | `image` | |

## Unsupported Features & Fallback Strategy

Any Markdown feature not listed above is considered **unsupported**.

- **Unsupported Blocks** (e.g., Tables, HTML):
  - **Strategy**: Convert to a `paragraph` containing the plain text content of the node.
  - **Rationale**: Preserves the information without implying formatting we cannot render.

- **Unsupported Inlines** (e.g., Strikethrough, Footnotes):
  - **Strategy**: Convert to `text` node containing the plain text value.
  - **Rationale**: Formatting is lost, but content is preserved.

## Implementation Details

The conversion is handled by `markdown-adapter.ts` using `remark-parse` and `remark-gfm`.

- **GFM Support**: Enabled for Task Lists.
- **Tables**: Currently unsupported by RTD. Will be rendered as plain text paragraphs.
- **Strikethrough**: Currently unsupported by RTD. Will be rendered as plain text.
