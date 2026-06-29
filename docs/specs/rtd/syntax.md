# RTD Source Grammar

**Status**: Draft
**Parent**: [RTD Architecture](../architecture.md)

## 1. Introduction

This specification defines the rigorous rules for parsing the **Ingestion Dialect** (Markdown+) into the [Rich Text Object Model (RTOM)](./model.md).

### 1.1 Philosophy: "CommonMark + LLM - HTML"

The parsing logic follows a specific philosophy:

1.  **Rigorous CommonMark**: We aim to support the core structural and phrasing constructs of CommonMark (including indented code blocks and `_` emphasis) to ensure compatibility with standard Markdown.
2.  **LLM Normalization**: We explicitly handle and normalize artifacts common in LLM outputs (e.g., LaTeX delimiters, "Ghost Citations").
3.  **Security by Construction**: We strictly **REJECT** any HTML or unsafe constructs. The parser does not recognize HTML tags; it treats them as plain text.
4.  **Streaming-First**: The grammar is designed to be parsed by a state machine that handles incremental updates without "flicker".
5.  **Eager Emitting**: The parser MUST emit tokens and update the AST as soon as a construct is unambiguously identified, rather than waiting for a block to close. This ensures a responsive UI during streaming.

## 2. Architecture

Following the design of the HTML5 Parser, the RTD parsing process consists of two main stages:

1.  **Tokenization**: A state machine that consumes the **Input Stream** and emits **Tokens**.
2.  **Tree Construction**: A state machine that consumes **Tokens** and builds the **RTOM Tree**.

### 2.1 The Resumable Parser

_Note: The detailed mechanics of the Resumable Parser are defined in the [Streaming Protocol](./streaming.md)._

The Tokenizer is designed as a **Resumable State Machine**. Instead of relying on an external buffer, the Tokenizer itself manages suspension and resumption when it encounters ambiguous input at the end of a stream chunk.

#### 2.1.1 Suspension & Resumption

_Moved to [Streaming Protocol](./streaming.md)._

### 2.2 LLM Normalization (Preprocessing)

Before tokenization, the input stream MUST be normalized to handle LLM idiosyncrasies.

1.  **Math Delimiters**:
    - `\[ ... \]` -> `$$ ... $$` (Block Math)
    - `\( ... \)` -> `$ ... $` (Inline Math)
2.  **Ghost Citations**:
    - Patterns like `【13†source】` are detected.
    - These are NOT removed but transformed into a specific sequence that the Tokenizer recognizes as a `Citation` token.

## 3. Tokenization (State Machine)

The Tokenizer is a state machine that consumes characters and emits tokens.

### 3.1 Tokens

The Tokenizer emits the following tokens:

- **Character**: A single character of text.
- **EOF**: End of stream.
- **BlockStart**: Signals the start of a block (type, attributes).
- **BlockEnd**: Signals the end of a block.
- **InlineStart**: Signals the start of an inline style (type).
- **InlineEnd**: Signals the end of an inline style.

### 3.2 States

The Tokenizer maintains a `State` variable. The initial state is `Data`.

#### 3.2.1 Data State

Consume the next input character:

- `#`: Switch to **Heading Start State**.
- `` ` ``: Switch to **Backtick State**.
- `>`: Switch to **Blockquote Start State**.
- `*`: Switch to **Asterisk State**.
- `-`: Switch to **Hyphen State**.
- `_`: Switch to **Underscore State**.
- `[`: Switch to **Link Start State**.
- `!`: Switch to **Image Start State**.
- `$`: Switch to **Math Start State**.
- `<`: Switch to **Less Than State**.
- `:`: Switch to **Colon State**.
- `0-9`: Switch to **Ordered List State**.
- `\n`: Emit **BlockEnd** (if applicable), switch to **Line Start State**.
- `Space/Tab`: Track indentation for **Indented Code Block**.
- `\u0000`: Switch to **Citation State**.
- EOF: Emit **EOF**.
- Anything else: Emit **Character** token.

#### 3.2.2 Heading Start State

- Consume `#` characters.
- If count <= 6 and followed by space: Emit **BlockStart(Heading, level)**. Switch to **Data State**.
- Otherwise: Emit consumed `#` as **Character** tokens. Switch to **Data State**.

#### 3.2.3 Backtick State

- Consume `` ` `` characters.
- If count >= 3: Switch to **Code Fence State**.
- If count < 3: Switch to **Code Span State**.

#### 3.2.4 Code Fence State

- Consume characters until newline.
- Emit **BlockStart(CodeBlock, language)**.
- Switch to **Raw Text State** (consumes everything until closing fence).
- **Nested Fence Repair**:
  - While in **Raw Text State**, track a `Nesting Level` (initially 0).
  - If a line matches the _start fence pattern_ (e.g., ` ```js `), increment `Nesting Level`.
  - **Colon Heuristic**: If a line matches ` ``` ` (no info string) AND the previous line ends with `:`, increment `Nesting Level`.
  - If a line matches the _closing fence pattern_ (e.g., ` ``` `):
    - If `Nesting Level > 0`: Decrement `Nesting Level`. Treat line as content.
    - If `Nesting Level == 0`: Close the block. Switch to **Data State**.

#### 3.2.5 Asterisk State

- Consume `*`.
- If followed by `Space`: Emit **BlockStart(List, Unordered)**. Switch to **Data State**.
- If `**`: Emit **InlineStart(Strong)**. Switch to **Data State**.
- If `*` (and not followed by `*`): Emit **InlineStart(Emphasis)**. Switch to **Data State**.

#### 3.2.6 Underscore State

- Consume `_`.
- If `__`: Emit **InlineStart(Strong)**. Switch to **Data State**.
- If `_` (and not followed by `_`): Emit **InlineStart(Emphasis)**. Switch to **Data State**.
- _Note: CommonMark "intraword emphasis" rules apply here (e.g., `foo_bar` is not italic)._

#### 3.2.7 Hyphen State

- Consume `-`.
- If followed by `Space`: Emit **BlockStart(List, Unordered)**. Switch to **Data State**.
- If `---` (and newline): Emit **BlockStart(ThematicBreak)**. Switch to **Data State**.
- Otherwise: Emit **Character(-)**. Switch to **Data State**.

#### 3.2.8 Ordered List State

- Consume digits.
- If followed by `. ` (dot space): Emit **BlockStart(List, Ordered)**. Switch to **Data State**.
- Otherwise: Emit consumed digits as **Character** tokens. Switch to **Data State**.

#### 3.2.9 Less Than State

- Consume `<`.
- If followed by `!--`: Switch to **Comment State**.
- Otherwise: Emit **Character(<)**. Switch to **Data State**.

#### 3.2.10 Comment State

- Consume characters.
- **Nesting**:
  - If `<!--` is encountered: Increment `Nesting Level`.
  - If `-->` is encountered:
    - If `Nesting Level > 0`: Decrement `Nesting Level`.
    - If `Nesting Level == 0`: Emit **BlockStart(Comment)** (or Inline), Emit content, Emit **BlockEnd**. Switch to **Data State**.
- **Sanity Limit**:
  - If line count > `MAX_COMMENT_LINES` (e.g., 20):
    - Abort comment parsing.
    - Emit `<!--` and all buffered content as **Character** tokens.
    - Switch to **Data State**.
- **Heuristic Recovery**:
  - If a **Strong Block Start Signal** is encountered at the start of a line:
    - **Signals**: `#` (Heading), ` ``` ` (Code Block), `:::` (Container).
    - **Note**: `>` (Blockquote) and `-` (List) are NOT treated as strong signals to allow valid Markdown inside comments.
    - **Action**: Infer implicit comment closure.
    - Emit **BlockStart(Comment)**, Emit content, Emit **BlockEnd**.
    - Re-process the signal character in **Data State**.
- **EOF**:
  - If EOF is reached while in **Comment State**:
    - Abort comment parsing.
    - Emit `<!--` and all buffered content as **Character** tokens.
    - Emit **EOF**.

#### 3.2.11 Link Start State

- Consume `[`.
- Switch to **Link Text State** (consumes until `]`).
- If `](` follows: Switch to **Link Url State**.
- Emit **InlineStart(Link)**.

#### 3.2.12 Image Start State

- Consume `!`.
- If followed by `[`: Switch to **Link Start State** (with image flag).
- Otherwise: Emit **Character(!)**. Switch to **Data State**.

#### 3.2.13 Math Start State

- Consume `$`.
- If `$$`: Emit **BlockStart(MathBlock)**. Switch to **Data State**.
- If `(`: Switch to **Icon State**.
- If `$`: Emit **InlineStart(MathInline)**. Switch to **Data State**.
- **Note**: The parser normalizes `\[ ... \]` to `$$ ... $$` (Block) and `\( ... \)` to `$ ... $` (Inline) before tokenization.
- **Inline Double Dollar**: If `$$` is encountered in an inline context (e.g., `$$ x^2 $$`), it is treated as an **Inline Math** delimiter to support normalized block math that appears inline.

#### 3.2.14 Blockquote Start State

- Consume `>`.
- If followed by `Space`:
  - Check for `[!NOTE]` (or other variants).
  - If found: Emit **BlockStart(Alert, variant)**.
  - Otherwise: Emit **BlockStart(Blockquote)**.
  - Switch to **Data State**.
- Otherwise: Emit **Character(>)**. Switch to **Data State**.

#### 3.2.15 Line Start State

- Reset indentation counter.
- Consume whitespace (counting spaces/tabs).
- If indentation >= 4 spaces: Emit **BlockStart(CodeBlock, Indented)**. Switch to **Data State**.
- **List Interruption**: If the line starts with a list marker (`-`, `*`, `1.`) followed by a space, it interrupts the current paragraph (if any) and starts a **List**.
- Otherwise: Re-emit whitespace. Switch to **Data State** (to check for `#`, `>`, etc.).

#### 3.2.16 Citation State

- Consume `CITATION`.
- Consume characters until `\u0000`.
- Emit **Inline(Citation)**.
- Switch to **Data State**.

#### 3.2.17 Icon State

- Consume `(`.
- Consume characters until `)`.
- Emit **Inline(Icon)**.
- Switch to **Data State**.

#### 3.2.18 Colon State

- Consume `:`.
- If count >= 3:
  - Parse optional **Variant** string (e.g., `::: axiom-card`).
  - Emit **BlockStart(Container, variant)**.
  - Switch to **Data State** (content is parsed recursively).
  - _Note_: If a line matches the closing fence (e.g., `:::`) and matches the current container's fence length, Emit **BlockEnd**.
- Otherwise: Emit consumed `:` as **Character** tokens. Switch to **Data State**.

_(Full state transitions for all constructs to be implemented in code)_

## 4. Tree Construction

The Tree Builder consumes tokens and modifies the `Document` tree.

### 4.1 Insertion Modes

The Tree Builder operates in different modes depending on the current open block.

- **Initial Mode**: Expecting block starts.
- **In Body Mode**: Inside a block (Paragraph, Heading).
- **In Code Block Mode**: Inside a fenced code block.

### 4.2 Handling "Open" Blocks

- **Paragraphs**: A `ParagraphBlock` remains open until a `BlockStart` (Heading, List, etc.) or `Double Newline` is encountered.
- **Coalescing**: Consecutive **Character** tokens are coalesced into a single text node within the open block.

### 4.3 Block Refinement

Some blocks are refined during construction based on their content.

- **Fields**: If a `ParagraphBlock` matches the pattern `**Key**: Value`, it MAY be refined into a `FieldBlock` (or equivalent structure) by the Tree Builder.

## 5. Syntax Reference (Informative)

This section summarizes the valid syntax constructs supported by the State Machine.

### 5.1 Blocks

| Construct                 | Syntax                  | Notes                                     |
| :------------------------ | :---------------------- | :---------------------------------------- |
| **Heading**               | `# ` to `###### `       | ATX style only. Requires space after `#`. |
| **Code Block (Fenced)**   | ` ```lang ` ... ` ``` ` | Standard fenced code blocks.              |
| **Code Block (Indented)** | 4 spaces or 1 tab       | Standard indented code blocks.            |
| **Blockquote**            | `> `                    | Nested blockquotes supported.             |
| **List**                  | `- `, `* `, `1. `       | Ordered and Unordered. Supports nesting.  |
| **Thematic Break**        | `---`, `***`, `___`     | 3 or more.                                |
| **Alert**                 | `> [!NOTE]`             | GitHub Flavored Alerts.                   |
| **Container**             | `::: variant`           | Generic container block.                  |

### 5.2 Inlines

| Construct     | Syntax                   | Notes                                                      |
| :------------ | :----------------------- | :--------------------------------------------------------- |
| **Bold**      | `**text**` or `__text__` | CommonMark strong emphasis.                                |
| **Italic**    | `*text*` or `_text_`     | CommonMark emphasis (with intraword restrictions for `_`). |
| **Code Span** | `` `text` ``             |                                                            |
| **Link**      | `[text](url)`            |                                                            |
| **Image**     | `![alt](url)`            |                                                            |
| **Math**      | `$expr$`                 | Inline math.                                               |
| **Citation**  | `【...】`                | Normalized from LLM output.                                |
| **Comment**   | `<!-- ... -->`           | HTML Comment (hidden).                                     |
| **Icon**      | `$(icon-name)`           |                                                            |

### 5.3 Exosuit Extensions

#### 5.3.1 Fields

Key-value pairs for structured data.

```
**Key**: Value
```

## 6. Rendering & Security

This section defines rules for the **Renderer**, distinct from the Parser.

### 6.1 No HTML

- **Rule**: The Tokenizer DOES NOT recognize HTML tags.
- **Behavior**: `<div ...>` is parsed as a sequence of **Character** tokens: `<`, `d`, `i`, `v`...
- **Result**: The renderer receives plain text, ensuring no HTML is executed.

### 6.2 Link Security

To ensure security and host control, Links are subject to strict validation during rendering.

#### 6.2.1 Supported Schemes

The renderer MUST validate the `target` URL against a **Scheme Whitelist**.

- **Core Schemes**: `http`, `https`, `mailto`.
- **Relative Paths**: Paths starting with `/`, `./`, or `../` are allowed (resolved relative to the document).

#### 6.2.2 Extension Point: Scheme Registry

The host environment MAY extend the whitelist with **Host Schemes**.

- Example (VS Code): `command:`, `vscode:`, `file:`.
- Example (Web): `tel:`.

#### 6.2.3 Fallback Behavior

If a link's scheme is not in the whitelist:

1.  **Parsing**: It is still parsed as a `Link` node (preserving the AST structure).
2.  **Rendering**: The renderer MUST treat it as "unsafe". It should be rendered as:
    - Plain text (stripping the anchor tag), OR
    - A disabled/non-clickable link with a visual indicator (e.g., strikethrough or warning icon).
