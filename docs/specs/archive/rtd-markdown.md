# RTD Markdown Specification

**Status**: Draft
**Version**: 1.2

## 1. Introduction

This document specifies the "RTD Markdown" flavor used in Exosuit. It defines how a Markdown string is parsed and transformed into a Rich Text Document (RTD) structure.

The goal is to support a subset of GitHub Flavored Markdown (GFM) while adding specific structural heuristics to support "Field" patterns (e.g., `**Key**: Value`) and other AI-generated idiosyncrasies.

This spec is **restrictive**: Any Markdown feature not explicitly listed below is treated as plain text. This ensures a consistent and predictable rendering output within the Exosuit UI.

## 2. Parsing Model

The parsing process consists of three stages:

1.  **Pre-processing**: The raw input string is normalized to handle non-standard AI patterns.
    *   **Math Normalization**: Convert LaTeX-style delimiters to standard Markdown math delimiters.
        *   Replace `\[` ... `\]` with `$$` ... `$$`.
        *   Replace `\(` ... `\)` with `$` ... `$`.
2.  **Base Parsing**: The normalized string is parsed into a Markdown Abstract Syntax Tree (MDAST) using the [CommonMark](https://commonmark.org/) spec with [GFM](https://github.github.com/gfm/) extensions and **Math** extensions (e.g., `remark-math`).
3.  **RTD Transformation**: The MDAST is traversed and transformed into an RTD tree. This stage includes heuristic logic for splitting paragraphs, detecting containers, and normalizing AI-specific patterns.

## 3. RTD Transformation Algorithms

The transformation is defined as a function `ConvertBlocks(nodes)` which takes a list of MDAST nodes and returns a list of RTD blocks.

### 3.1. Block Processing

For each node in the input list, apply the following rules based on `node.type`:

#### 3.1.1. Paragraph (`paragraph`)
**Input**: An MDAST `paragraph` node.
**Output**: One or more RTD blocks.

**Algorithm**: `SplitParagraph(node)`
1.  Let `children` be the inline children of the paragraph.
2.  Let `groups` be a list of lists of inlines.
3.  Let `currentGroup` be an empty list.
4.  For each `child` in `children` at index `i`:
    a.  **Determine if `child` starts a new Field**:
        i.  Let `isFieldStart` be `false`.
        ii. If `child.type` is `strong`:
            *   **Lookahead**: If the next sibling is `text` and matches `^\s*:` (starts with optional whitespace followed by a colon), set `hasColon` to `true`.
            *   **Internal**: If the last child of `child` is `text` and matches `:\s*$` (ends with a colon followed by optional whitespace), set `hasColon` to `true`.
            *   If `hasColon` is `true`:
                *   If `i` is 0, set `isFieldStart` to `true`.
                *   Else if the previous sibling is `text` and matches `/\s*\r?\n$/` (ends with a newline, optionally preceded by whitespace), set `isFieldStart` to `true`.
    b.  If `isFieldStart` is `true` and `currentGroup` is not empty:
        i.  Append `currentGroup` to `groups`.
        ii. Reset `currentGroup` to empty.
    c.  Append `child` to `currentGroup`.
5.  If `currentGroup` is not empty, append it to `groups`.
6.  For each `group` in `groups`:
    a.  Convert the inlines in `group` to RTD inlines.
    b.  Apply `CreateBlockFromInlines(inlines)` to generate the final block.

**Algorithm**: `CreateBlockFromInlines(inlines)`
1.  **Field Detection**:
    a.  If `inlines[0]` is `strong`:
        i.  **Case A**: If `inlines[1]` is `text` and matches `^\s*:`, set `isField` to `true`.
        ii. **Case B**: If `inlines[0]` ends with `:`, set `isField` to `true`.
2.  If `isField` is `true`:
    *   Return a `container` block with `variant: "field"`.
    *   The container contains a single `paragraph` block with the given `inlines`.
3.  Else:
    *   Return a `paragraph` block with the given `inlines`.

#### 3.1.2. Heading (`heading`)
*   **Output**: An RTD `heading` block.
*   **Properties**:
    *   `level`: `node.depth` (1-6).
    *   `children`: Result of `ConvertInlines(node.children)`.

#### 3.1.3. Code Block (`code`)
*   **Output**: An RTD `code-block` block OR `diagram` block.

**Algorithm**: `ProcessCodeBlock(node)`
1.  **Check for Diagram**:
    *   If `node.lang` is "mermaid" or "graphviz":
    *   Return `diagram` block.
        *   `tool`: `node.lang`.
        *   `value`: `node.value`.
2.  **Standard Code Block**:
    *   Apply `ParseCodeMetadata(node.lang, node.meta)` to get `language` and `filename`.
    *   Return `code-block` block.
        *   `language`: `language`.
        *   `filename`: `filename`.
        *   `value`: `node.value`.

**Algorithm**: `ParseCodeMetadata(lang, meta)`
1.  Initialize `language` = `lang`, `filename` = `undefined`.
2.  **Pattern 1 (Colon)**: If `lang` matches `^([^:]+):(.+)$` (e.g., `python:src/main.py`):
    *   Set `language` to group 1.
    *   Set `filename` to group 2.
3.  **Pattern 2 (Meta Attribute)**: If `meta` contains `filename="([^"]+)"` or `filename=([^ ]+)`:
    *   Set `filename` to the captured value.
4.  Return `{ language, filename }`.

#### 3.1.4. List (`list`)
*   **Output**: An RTD `list` block.
*   **Properties**:
    *   `ordered`: `node.ordered`.
    *   `items`: Map each child `listItem` to an object:
        *   `checked`: `node.checked` (or undefined).
        *   `children`: Result of `ConvertBlocks(node.children)`.

#### 3.1.5. Blockquote & Alerts (`blockquote`)
**Input**: An MDAST `blockquote` node.
**Output**: An RTD `alert` block OR `blockquote` block.

**Algorithm**: `ProcessBlockquote(node)`
1.  **Check for GFM Alert**:
    *   If the first child is a `paragraph` and its text content starts with `[!NOTE]`, `[!WARNING]`, etc. (case-insensitive):
    *   Return `alert` block with `variant` derived from the tag (e.g., "note", "warning").
    *   Remove the alert tag from the content.
2.  **Check for Legacy Alert**:
    *   If the first child is a `paragraph`.
    *   AND the first child of that paragraph is `strong`.
    *   AND the `strong` node's text is exactly "Note", "Warning", "Important", "Tip", or "Caution".
    *   AND the `strong` node is the only content on the first line (followed by newline or end of node).
    *   Return `alert` block with `variant` derived from the text.
3.  **Fallback**:
    *   Return `blockquote` block.
    *   `children`: Result of `ConvertBlocks(node.children)`.

#### 3.1.6. HTML / Callouts (`html`)
**Input**: An MDAST `html` node.
**Output**: An RTD `callout` block OR `paragraph` (fallback).

**Algorithm**: `ProcessHTML(node)`
1.  **Check for Semantic Tags**:
    *   Check if `node.value` matches an opening tag for `<thinking>`, `<plan>`, or `<scratchpad>`.
    *   *Note*: Since CommonMark might treat the entire block (tags + content) as one HTML string, we must parse the inner content.
2.  If match found:
    *   Extract content between opening and closing tags.
    *   **Re-parse**: Run `BaseParsing` + `ConvertBlocks` on the extracted content.
    *   Return `callout` block.
        *   `variant`: The tag name (e.g., "thinking").
        *   `children`: The result of the re-parsing.
3.  Else:
    *   Treat as plain text (fallback to Paragraph).

#### 3.1.7. Thematic Break (`thematicBreak`)
*   **Output**: An RTD `thematic-break` block.

#### 3.1.8. Math Block (`math`)
*   **Output**: An RTD `math-block` block.
*   **Properties**:
    *   `value`: `node.value`.

#### 3.1.9. Fallback
*   For any other node type (e.g., YAML frontmatter):
    *   **Output**: An RTD `paragraph` block.
    *   **Content**: A single `text` inline containing the stringified content of the node.

### 3.2. Inline Processing

The transformation `ConvertInlines(nodes)` maps MDAST phrasing content to RTD inlines.

#### 3.2.1. Text (`text`)
*   **Output**: One or more RTD inlines (`text` or `citation`).

**Algorithm**: `ProcessText(node)`
1.  Find all matches of `\u3010(.*?)\u3011` (matches `【...】`).
2.  Split `node.value` into parts based on these matches.
3.  Return a list of inlines:
    *   For each match: `{ kind: 'citation', value: matchGroup1 }`.
    *   For each non-matching part: `{ kind: 'text', value: part }`.

#### 3.2.2. Strong (`strong`)
*   **Output**: `{ kind: 'strong', children: ConvertInlines(node.children) }`

#### 3.2.3. Emphasis (`emphasis`)
*   **Output**: `{ kind: 'emphasis', children: ConvertInlines(node.children) }`

#### 3.2.4. Strikethrough (`delete`)
*   **Output**: `{ kind: 'strikethrough', children: ConvertInlines(node.children) }`

#### 3.2.5. Inline Code (`inlineCode`)
*   **Output**: `{ kind: 'code-span', value: node.value }`

#### 3.2.6. Link (`link`)
*   **Output**: `{ kind: 'link', target: node.url, title: node.title, children: ConvertInlines(node.children) }`

#### 3.2.7. Image (`image`)
*   **Output**: `{ kind: 'image', src: node.url, alt: node.alt, title: node.title }`

#### 3.2.8. Inline Math (`inlineMath`)
*   **Output**: `{ kind: 'math-inline', value: node.value }`

#### 3.2.9. Fallback
*   For any other inline type:
    *   **Output**: `{ kind: 'text', value: getTextContent(node) }`

## 4. Example Transformations

### 4.1. Dense Field Parsing
*(See previous version)*

### 4.2. False Positive Prevention
*(See previous version)*

### 4.3. Relaxed Whitespace
*(See previous version)*

### 4.4. Code Block Metadata
*(See previous version)*

### 4.5. Legacy Admonition
*(See previous version)*

### 4.6. Thinking Block
*(See previous version)*

### 4.7. Mermaid Diagram

**Input**:
```markdown
```mermaid
graph TD; A-->B;
```
```

**RTD Output**:
`Diagram`
  - `tool`: "mermaid"
  - `value`: "graph TD; A-->B;\n"

### 4.8. Math Normalization

**Input**:
```markdown
The value is \( x \).
\[ E = mc^2 \]
```

**Pre-processing**:
Converts to: `The value is $ x $. $$ E = mc^2 $$`

**RTD Output**:
1.  `Paragraph` -> `[Text("The value is "), MathInline(" x "), Text(".")]`
2.  `MathBlock` -> `value: " E = mc^2 "`

### 4.9. Ghost Citations

**Input**:
```markdown
Sky is blue【13†source】.
```

**RTD Output**:
`Paragraph` -> `[Text("Sky is blue"), Citation("13†source"), Text(".")]`

## 5. RTD Schema Definition

The following TypeScript interfaces define the contract for the RTD structure.

```typescript
export type RTDBlock =
  | { kind: 'paragraph'; children: RTDInline[] }
  | { kind: 'heading'; level: 1 | 2 | 3 | 4 | 5 | 6; children: RTDInline[] }
  | { kind: 'code-block'; language?: string; filename?: string; value: string }
  | { kind: 'diagram'; tool: string; value: string }
  | { kind: 'math-block'; value: string }
  | { kind: 'list'; ordered: boolean; items: { checked?: boolean; children: RTDBlock[] }[] }
  | { kind: 'blockquote'; children: RTDBlock[] }
  | { kind: 'alert'; variant: 'note' | 'tip' | 'important' | 'warning' | 'caution'; children: RTDBlock[] }
  | { kind: 'callout'; variant: 'thinking' | 'plan' | 'scratchpad'; children: RTDBlock[] }
  | { kind: 'thematic-break' }
  | { kind: 'container'; variant: 'field'; children: RTDBlock[] };

export type RTDInline =
  | { kind: 'text'; value: string }
  | { kind: 'strong'; children: RTDInline[] }
  | { kind: 'emphasis'; children: RTDInline[] }
  | { kind: 'strikethrough'; children: RTDInline[] }
  | { kind: 'code-span'; value: string }
  | { kind: 'math-inline'; value: string }
  | { kind: 'link'; target: string; title?: string; children: RTDInline[] }
  | { kind: 'image'; src: string; alt: string; title?: string }
  | { kind: 'citation'; value: string };
```

