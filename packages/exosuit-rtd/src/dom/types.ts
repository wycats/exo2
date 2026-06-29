export type RTDNode = RTDBlock | RTDInline;

// --- Blocks ---

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
  | CommentBlock
  | XmlBlock
  | TableBlock;

export interface ParagraphBlock {
  kind: "paragraph";
  children: RTDInline[];
}

export interface TableBlock {
  kind: "table";
  header: TableRow;
  rows: TableRow[];
  alignments: ("left" | "center" | "right" | null)[];
}

export interface TableRow {
  cells: TableCell[];
}

export interface TableCell {
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

export interface XmlBlock {
  kind: "xml-block";
  tagName: string;
  attributes: Record<string, string>;
  content: string;
}

// --- Inlines ---

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
  href: string;
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
