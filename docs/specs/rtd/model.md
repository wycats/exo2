# Rich Text Object Model (RTOM)

**Status**: Draft
**Parent**: [RTD Architecture](../architecture.md)

## 1. Overview

The **Rich Text Object Model (RTOM)** defines the strict TypeScript interfaces for the Rich Text Document tree. It is the "Infoset" that all parsers produce and all renderers consume.

## 2. Core vs. Extended Model

The RTOM is designed to support two distinct use cases:

1.  **Core RTOM**: Represents standard rich text content (Paragraphs, Lists, Code). This is the primary output of the "Ingestion Dialect" (Markdown).
2.  **Extended RTOM (Layout)**: Represents structural UI elements (Regions, Collections, Items). These are expressed via the `ContainerBlock` node.

## 3. The Node Hierarchy

All nodes in the tree are either **Blocks** (structural) or **Inlines** (phrasing).

```typescript
export type RTDNode = RTDBlock | RTDInline;
```

### 2.1 Blocks

Blocks represent distinct vertical sections of the document.

```typescript
export type RTDBlock =
  | ParagraphBlock
  | HeadingBlock
  | CodeBlock
  | DiagramBlock
  | MathBlock
  | ListBlock
  | BlockquoteBlock
  | AlertBlock
  | CalloutBlock
  | ThematicBreakBlock
  | ContainerBlock
  | CommentBlock;

export interface ParagraphBlock {
  kind: "paragraph";
  children: RTDInline[];
}

export interface HeadingBlock {
  kind: "heading";
  level: 1 | 2 | 3 | 4 | 5 | 6;
  children: RTDInline[];
}

export interface CodeBlock {
  kind: "code-block";
  language?: string;
  filename?: string;
  value: string;
}

export interface DiagramBlock {
  kind: "diagram";
  tool: "mermaid" | "graphviz";
  value: string;
}

export interface MathBlock {
  kind: "math-block";
  value: string;
}

export interface ListBlock {
  kind: "list";
  ordered: boolean;
  items: ListItem[];
}

export interface ListItem {
  checked?: boolean;
  children: RTDBlock[];
}

export interface BlockquoteBlock {
  kind: "blockquote";
  children: RTDBlock[];
}

export interface AlertBlock {
  kind: "alert";
  variant: "note" | "tip" | "important" | "warning" | "caution";
  children: RTDBlock[];
}

export interface CalloutBlock {
  kind: "callout";
  variant: "thinking" | "plan" | "scratchpad";
  children: RTDBlock[];
}

export interface ThematicBreakBlock {
  kind: "thematic-break";
}

export interface ContainerBlock {
  kind: "container";
  variant: string; // e.g., "field", "card"
  children: RTDBlock[];
}

export interface CommentBlock {
  kind: "comment";
  value: string;
}
```

### 2.2 Inlines

Inlines represent phrasing content within a block.

```typescript
export type RTDInline =
  | TextInline
  | StrongInline
  | EmphasisInline
  | StrikethroughInline
  | CodeSpanInline
  | MathInline
  | LinkInline
  | ImageInline
  | IconInline
  | CommandInline
  | CitationInline
  | CommentInline;

export interface TextInline {
  kind: "text";
  value: string;
}

export interface StrongInline {
  kind: "strong";
  children: RTDInline[];
}

export interface EmphasisInline {
  kind: "emphasis";
  children: RTDInline[];
}

export interface StrikethroughInline {
  kind: "strikethrough";
  children: RTDInline[];
}

export interface CodeSpanInline {
  kind: "code-span";
  value: string;
}

export interface MathInline {
  kind: "math-inline";
  value: string;
}

export interface LinkInline {
  kind: "link";
  target: string;
  title?: string;
  children: RTDInline[];
}

export interface ImageInline {
  kind: "image";
  src: string;
  alt: string;
  title?: string;
}

export interface IconInline {
  kind: "icon";
  name: string; // e.g., "gear"
}

export interface CommandInline {
  kind: "command";
  id: string;
  args?: any[];
  children: RTDInline[];
}

export interface CitationInline {
  kind: "citation";
  value: string; // e.g., "13†source"
}

export interface CommentInline {
  kind: "comment";
  value: string;
}
```
