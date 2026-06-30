<!-- exo:10159 ulid:01kmzxbcy76rzs683dma5kgghp -->


# RFC 10159: Rich Text DOM (RTD)

- **Supersedes**: RFC 0020


- **Status**: Stage 4 (Stable)
- **Created**: 2025-05-20
- **Implemented**: `packages/exosuit-rtd`

## Summary

The Rich Text DOM (RTD) is a framework-agnostic, type-safe document object model designed to represent, manipulate, and render rich content within the Exosuit ecosystem. It serves as the intermediate representation between raw data (like Markdown or TOML) and visual presentation (VS Code Webviews, HTML).

## Motivation

Exosuit needs to display complex, structured information—such as project plans, design documents, and activity logs—in various contexts.

1.  **Consistency**: We need a single source of truth for how a "Task" or "Phase" looks, regardless of where it is rendered.
2.  **Interactivity**: Static HTML strings are hard to make interactive. A DOM-like structure allows us to attach event handlers and state.
3.  **Portability**: While VS Code is the primary target, the core logic should not be coupled to the VS Code API.

## Design

### The RTD Node Hierarchy

The core of RTD is the `RtdNode` interface. All elements in the document tree implement this interface.

```typescript
export interface RtdNode {
  type: string;
  children?: RtdNode[];
  attributes?: Record<string, any>;
}
```

Common node types include:

- `Block`: Container elements (Section, Card, Callout).
- `Inline`: Text-level elements (Text, Link, Badge).
- `Interactive`: Elements that trigger actions (Button, Toggle).

### The Builder Pattern

To construct RTD trees ergonomically, we provide a fluent `RtdBuilder` API.

```typescript
const doc = new RtdBuilder()
  .section("Phase 1")
  .paragraph("This is the description.")
  .card((c) => c.title("Task 1").badge("Pending", "warning"))
  .build();
```

### Parsing and Serialization

- **Parser**: Converts Markdown (via `remark`/`mdast`) into an RTD tree.
- **Serializer**: Converts an RTD tree into HTML (for Webviews) or plain text (for LLM context).

## Implementation

The implementation resides in `packages/exosuit-rtd`.

- `src/dom/`: Node definitions.
- `src/builder/`: Fluent API.
- `src/parser/`: Markdown to RTD transformation.
- `src/renderers/`: HTML generation.
