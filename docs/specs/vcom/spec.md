# VS Code Chat Object Model (VCOM)

**Version:** 2.2.0 (Candidate Recommendation)
**Status:** Living Standard
**Editors:** Ian Hickson, Domenic Denicola, Anders Hejlsberg, Chris Dias
**Target:** VS Code Chat API (v1.90+)

## 1. Introduction

This specification defines the **VS Code Chat Object Model (VCOM)**, the abstract document object model for the VS Code Chat interface. It defines the logical structure of documents and the normative binding between the **[Literate Kernel Protocol](../literate-kernel/spec.md)** (the Producer) and the **VS Code Extension Host** (the Consumer).

### 1.1 Conformance

A conforming **User Agent** (the VS Code Chat Renderer) MUST implement the interfaces and behaviors defined in this specification. A conforming **Authoring Tool** (the Literate Kernel) MUST produce output that aligns with the Abstract Operations defined herein.

## 2. Infrastructure

### 2.1 Trees

The VCOM is a tree of **Nodes**. The tree is ordered.

- **Root**: The `VCOMStream` node.
- **Leaf**: Nodes that cannot contain other nodes (e.g., `VCOMCommand`, `VCOMFileTree`).
- **Parent**: `VCOMText` nodes are effectively leaves in the VCOM, though they contain internal Markdown ASTs.

### 2.2 Content Categories

To define the content model (what is allowed inside what), we define two categories:

1.  **Flow Content**: Elements that are typically displayed as distinct blocks.
    - `VCOMText`
    - `VCOMFileTree`
    - `VCOMTextEdit`
    - `VCOMProgress`
    - `VCOMTool`
2.  **Phrasing Content**: Elements that mark up the text at the intra-paragraph level.
    - `VCOMCommand`
    - `VCOMLink`
    - (Markdown AST nodes: `Strong`, `Emphasis`, `CodeSpan`, `ThemeIcon`)
3.  **Metadata Content**: Elements that do not render in the flow but populate auxiliary views.

    - `VCOMReference`

**Constraint (The Block Layout Rule):** The VCOM Root (`VCOMStream`) admits ONLY **Flow Content**.

## 3. The Object Model (IDL)

The following interfaces are defined using a TypeScript-like IDL.

### 3.1 The Root

```typescript
interface VCOMStream {
  readonly children: VCOMNode[];
  readonly metadata: VCOMMetadata[];
  readonly isFinalized: boolean;
}
```

### 3.2 Nodes

```typescript
type VCOMNode =
  | VCOMText
  | VCOMFileTree
  | VCOMCommand
  | VCOMLink
  | VCOMTextEdit
  | VCOMProgress
  | VCOMWarning
  | VCOMTool;

type VCOMMetadata = VCOMReference | VCOMContext;

interface VCOMBase {
  readonly kind: string;
  readonly id: string; // Unique identifier
}
```

## 4. Elements

### 4.1 The Text Element (`VCOMText`)

- **Categories**: Flow Content.
- **Content Model**: Contains **Phrasing Content** (via Markdown parsing).
- **IDL**:
  ```typescript
  interface VCOMText extends VCOMBase {
    kind: "text";
    value: string; // Markdown source
  }
  ```
- **Rendering Rules**:
  1.  The User Agent MUST parse `value` as CommonMark.
  2.  **Coalescing**: Consecutive `VCOMText` nodes emitted to the API MUST be coalesced into a single Markdown block.
  3.  **Theme Icons**: The User Agent MUST support `$(icon-name)` syntax.
  4.  **Sanitization**: The User Agent MUST run a sanitization pass.

### 4.2 The FileTree Element (`VCOMFileTree`)

- **Categories**: Flow Content.
- **Content Model**: Empty (Leaf).
- **IDL**:
  ```typescript
  interface VCOMFileTree extends VCOMBase {
    kind: "fileTree";
    baseUri: string; // Absolute URI
    data: FileTreeEntry[];
  }
  ```
- **Rendering Rules**:
  1.  MUST render as a distinct block (forcing a newline before and after).
  2.  **Interaction**: Clicking a 'file' entry MUST trigger the `vscode.open` command.

### 4.3 The Command Element (`VCOMCommand`)

- **Categories**: Phrasing Content.
- **Content Model**: Empty (Leaf).
- **IDL**:
  ```typescript
  interface VCOMCommand extends VCOMBase {
    kind: "command";
    command: VSCodeCommand;
    title?: string; // Optional label override
  }
  ```
- **Rendering Rules**:
  1.  MUST render as a clickable UI widget (Button or Chip).
  2.  **Layout**: SHOULD be rendered inline with text.

### 4.4 The TextEdit Element (`VCOMTextEdit`)

- **Categories**: Flow Content.
- **Content Model**: Empty (Leaf).
- **IDL**:
  ```typescript
  interface VCOMTextEdit extends VCOMBase {
    kind: "textEdit";
    uri: string;
    edits: TextEdit[]; // Line/Column deltas
    diff: string; // Unified Diff format
  }
  ```
- **Rendering Rules**:
  1.  MUST render as an interactive Diff View (Inline or Side-by-Side).
  2.  **Interaction**: SHOULD provide "Apply" and "Discard" actions if the User Agent supports them.
  3.  **Fallback**: If a native Diff View is unavailable, the User Agent MUST render the `diff` content as a Markdown Code Block with `diff` syntax highlighting.

### 4.5 The Link Element (`VCOMLink`)

- **Categories**: Phrasing Content.
- **Content Model**: Text (Label).
- **IDL**:
  ```typescript
  interface VCOMLink extends VCOMBase {
    kind: "link";
    target: string | Location;
    title?: string;
  }
  ```
- **Rendering Rules**:
  1.  MUST render inline as a "Chip" or "Link".
  2.  **Distinct from Markdown Links**: This element represents a semantic reference, not just a hypertext link.

### 4.6 The Reference Element (`VCOMReference`)

- **Categories**: Metadata Content.
- **Content Model**: Empty.
- **IDL**:
  ```typescript
  interface VCOMReference extends VCOMBase {
    kind: "reference";
    uri: string | Location;
  }
  ```
- **Rendering Rules**:
  1.  MUST NOT render in the main flow.
  2.  MUST render in the "Used References" auxiliary area (Header/Footer).

### 4.7 The Hidden Context Element (`VCOMContext`)

- **Categories**: Metadata Content.
- **Content Model**: Text (Raw Data).
- **IDL**:
  ```typescript
  interface VCOMContext extends VCOMBase {
    kind: "context";
    data: any; // JSON Serializable
  }
  ```
- **Rendering Rules**:
  1.  MUST NOT render in the visual flow.
  2.  **Persistence**: The User Agent MUST preserve this node in the document tree.
  3.  **Binding**: This data SHOULD be persisted in the host environment's native metadata storage (e.g., VS Code `ChatResult.metadata`) if available, rather than embedded in the text stream.

### 4.8 The Warning Element (`VCOMWarning`)

- **Categories**: Flow Content.
- **Content Model**: Text (Message).
- **IDL**:
  ```typescript
  interface VCOMWarning extends VCOMBase {
    kind: "warning";
    message: string;
  }
  ```
- **Rendering Rules**:
  1.  MUST render as a distinct warning widget (e.g., yellow border/icon).
  2.  **Usage**: Used for parser errors (e.g., "Malformed JSON in tool") or deprecation notices.

### 4.9 The Tool Element (`VCOMTool`)

- **Categories**: Flow Content.
- **Content Model**: Empty (Leaf).
- **IDL**:
  ```typescript
  interface VCOMTool extends VCOMBase {
    kind: "tool";
    toolName: string;
    parameters: Record<string, any>;
    state: "running" | "complete" | "error";
    result?: any;
  }
  ```
- **Rendering Rules**:
  1.  MUST render as a distinct block (e.g., a "Tool Call" card).
  2.  **State**: MUST visually reflect the `state` (e.g., spinner for "running", checkmark for "complete").
  3.  **Collapsibility**: The parameters and result SHOULD be collapsible to reduce noise.

## 5. The Parsing & Binding Protocol

This section defines the normative binding between the **[Literate Kernel Protocol](../literate-kernel/spec.md)** (the Producer) and the **VS Code Chat API** (the Consumer).

The architecture is a strict pipeline:
`Raw Stream` -> **Parser** -> `VCOMNode Stream` -> **Binder** -> `VS Code API`

### 5.1 The Pipeline

The User Agent MUST implement the following pipeline:

1.  **Input**: A stream of raw text chunks from the LLM.
2.  **Stage 1 (Parser)**: A state machine that consumes text and emits a stream of `VCOMNode` objects.
3.  **Stage 2 (Binder)**: A renderer that consumes `VCOMNode` objects and maps them to VS Code Chat API calls.

### 5.2 The Tree Construction Stage

The Parser operates as a stack-based state machine (The "Tree Builder"), consuming **Tokens** emitted by the Literate Kernel Tokenizer and constructing the **VCOM Tree**.

#### 5.2.1 The Stack of Open Elements

The machine maintains a stack of open `VCOMNode`s. The bottom of the stack is always the `VCOMStream` root.

**Attribute Handling**: When an `OpenTag Token` is processed, its attributes (key/value pairs) are extracted and assigned to the corresponding properties of the created `VCOMNode`.

#### 5.2.2 Insertion Modes

The machine behaves differently depending on the **Current Node** (top of stack).

1.  **Mode: In Stream (Default)**

    - **Input**: `Text Token`
      - **Action**:
        - If the last child of Current Node is `VCOMText`, append text to it.
        - Else, create new `VCOMText`, append to Current Node.
        - **Microtask**: Run **Reference Resolver** (5.4) on the text content to split `VCOMText` into `VCOMText | VCOMLink`.
    - **Input**: `OpenTag Token (<exo-tool>)`
      - **Action**: Create `VCOMTool`. Map attributes (e.g., `name` -> `toolName`). Push to stack. Switch to **In Tool Mode**.
    - **Input**: `OpenTag Token (<exo-tree>)`
      - **Action**: Create `VCOMFileTree`. Map attributes (e.g., `root` -> `baseUri`). Push to stack. Switch to **In JSON Block Mode**.
    - **Input**: `OpenTag Token (<exo-edit>)`
      - **Action**: Create `VCOMTextEdit`. Map attributes (e.g., `path` -> `uri`). Push to stack. Switch to **In Diff Block Mode**.
    - **Input**: `OpenTag Token (<exo-cmd>)`
      - **Action**: Create `VCOMCommand`. Map attributes. Push to stack. Switch to **In Inline Mode**.
    - **Input**: `CloseTag Token` (Any)
      - **Action**: Parse Error. Ignore the token.

2.  **Mode: In Tool Mode**

    - **Input**: `Text Token`
      - **Action**: Buffer text into `VCOMTool.parameters` (Raw String).
    - **Input**: `CloseTag Token (</exo-tool>)`
      - **Action**:
        - Parse buffered text as JSON.
        - If valid, assign to `parameters`.
        - If invalid, emit `VCOMWarning` and detach `VCOMTool`.
        - Pop stack. Switch to **In Stream**.
    - **Input**: `OpenTag Token` (Any)
      - **Action**: Parse Error. Treat as `Text Token` (buffer into parameters).
    - **Input**: `CloseTag Token` (Mismatch)
      - **Action**: Parse Error. Ignore token.

3.  **Mode: In JSON Block Mode** (Shared by Tree, etc.)

    - **Input**: `Text Token`
      - **Action**: Buffer text.
    - **Input**: `CloseTag Token` (Matching)
      - **Action**: Parse JSON, assign to node data, pop stack. Switch to **In Stream**.
    - **Input**: `CloseTag Token` (Mismatch)
      - **Action**: Parse Error. Ignore token.

4.  **Mode: In Diff Block Mode**

    - **Input**: `Text Token`
      - **Action**: Buffer text.
    - **Input**: `CloseTag Token (</exo-edit>)`
      - **Action**: Assign buffered text to `diff` property. Pop stack. Switch to **In Stream**.

5.  **Mode: In Inline Mode** (Cmd, Link)
    - **Input**: `Text Token`
      - **Action**: Buffer text (Label).
    - **Input**: `CloseTag Token` (Matching)
      - **Action**: Assign label, pop stack. Switch to **In Stream**.

#### 5.2.3 Error Recovery (The "Foster Parenting" Rules)

To handle "broken" markup (hallucinations or truncation), the Parser implements the following recovery strategies:

1.  **EOF Handling**: If the stream ends while the stack contains open elements (other than Root), the Parser MUST **Auto-Close** them.
    - _Effect_: Truncated JSON blocks are closed, triggering the JSON parser (which will likely fail and emit `VCOMWarning`).
2.  **Tag Soup**: If an `OpenTag` is encountered where it is not allowed (e.g., `<exo-tool>` inside `<exo-tool>`), the Parser MUST treat it as **Text Content**.
3.  **Stray Closing Tags**: If a `CloseTag` is encountered that does not match the Current Node:
    - If it matches an open node further down the stack: **Close Everything Above It** (pop until match).
    - If it matches nothing: **Ignore It**.

### 5.3 The Binder (Nodes -> API)

The Binder is responsible for visualizing the VCOM nodes using the host environment's capabilities.

- **Input**: `VCOMNode` Stream.
- **Output**: Side effects on `$API` (VS Code ChatResponseStream).

#### 5.3.1 Binding Rules

| Node Type      | Action                                                                                              |
| :------------- | :-------------------------------------------------------------------------------------------------- |
| `VCOMText`     | Call `$API.markdown(node.value)`.                                                                   |
| `VCOMLink`     | Call `$API.markdown` with a linkified path.                                                         |
| `VCOMTool`     | Render a **Synthetic View** (e.g., a Blockquote or Custom Widget) representing the tool invocation. |
| `VCOMFileTree` | Call `$API.fileTree(node.data, node.baseUri)`.                                                      |
| `VCOMCommand`  | Call `$API.button(node.command)`.                                                                   |
| `VCOMTextEdit` | Call `$API.textEdit(node.uri, node.edits)`.                                                         |
| `VCOMProgress` | Call `$API.progress(node.message)`.                                                                 |
| `VCOMWarning`  | Call `$API.warning(node.message)`.                                                                  |

### 5.4 Implicit Reference Resolution

The Parser MUST implement a **Reference Resolver** to automatically detect and linkify file paths in text.

#### 5.4.1 The Resolution Pipeline

The Reference Resolver operates as a stateful transformation pass within the Parser.

1.  **State Tracking**: The parser tracks context (e.g., `InLink`, `InCode`).
2.  **Context-Aware Processing**:
    - **Markdown Links** (`[...]`): Content inside existing links MUST NOT be processed.
    - **Code Spans** (`` `...` ``): Content inside backticks MUST be buffered and validated as a _single unit_.
      - **Hit**: If the buffered content matches a file, emit `VCOMLink`.
      - **Miss**: If not, emit `VCOMText` (preserving backticks).
    - **Plain Text**: Tokenize by whitespace and punctuation.
3.  **Validation**: Filter candidates based on syntactic heuristics.
4.  **Resolution**: Verify candidates against the **Workspace Cache**.
5.  **Output**: A sequence of `VCOMText` and `VCOMLink` nodes.

#### 5.4.2 The Workspace Cache

The User Agent MUST maintain a low-latency cache of the workspace file system.

- **Requirement**: The cache MUST support synchronous $O(1)$ existence checks for both **Files** and **Directories**.
- **Freshness**: The cache MUST be updated asynchronously via file system watchers.
- **Startup**: During the initial scan (cold start), the cache MAY return `false` for all queries (Optimistic Miss), resulting in plain text rendering.

See [Workspace Cache Design](../../design/workspace-cache.md) for non-normative implementation details.

#### 5.4.3 Candidate Validation (The "Path-Like" Heuristic)

A token $T$ is considered a **Candidate Path** if it satisfies the following constraints:

1.  **Negative Constraints** (Must be False):

    - Starts with a URI scheme (e.g., `http:`, `mailto:`).
    - Contains whitespace (unless wrapped in explicit delimiters).
    - Is a purely numeric string or version number (e.g., `1.0.2`).

2.  **Positive Indicators** (At least one Must be True):
    - **Separator Check**: Contains a forward slash `/` (or `\` on Windows).
    - **Extension Check**: Ends with a file extension matching `\.[a-zA-Z0-9]{1,10}$`.

#### 5.4.4 Resolution Logic

For each Candidate Path $C$, the Resolver applies the following logic in order:

1.  **Exact Match (The "Gold Standard")**:

    - Query: `Cache.hasFile(C)`
    - Result: If `true`, emit `VCOMLink(target=C)`.

2.  **Root Validation (The "Prose" Filter)**:

    - _Context_: Used to distinguish paths like `src/utils` from prose like `and/or`.
    - Logic: Extract the first segment $R$ of $C$ (e.g., `src` from `src/utils`).
    - Query: `Cache.hasDirectory(R)`
    - Result: If `true`, emit `VCOMLink(target=C)`. This assumes that if the root exists, the path is intentional, even if the specific file is missing (e.g., a new file proposal).

3.  **Heuristic Fallback (The "New File" Edge Case)**:
    - _Context_: The user or model proposes a file in a non-existent directory (e.g., `new-lib/mod.rs`).
    - Logic: If $C$ ends in a **Known Source Extension** (e.g., `.ts`, `.rs`, `.py`, `.md`, `.json`), emit `VCOMLink(target=C)`.
    - _Constraint_: The list of "Known Extensions" SHOULD be static or derived from the workspace's language usage.

#### 5.4.5 Punctuation Handling

The Tokenizer MUST handle trailing punctuation intelligently.

- **Rule**: If a token ends with punctuation (`.`, `,`, `:`, `?`, `!`, `)`), the punctuation MUST be stripped from the Candidate Path $C$ but preserved in the following `VCOMText` node.
- **Exception**: If the punctuation is part of a valid file name in the Cache (rare, but possible), it is preserved.

**Example**:

- Input: `Check src/main.ts.`
- Token: `src/main.ts.`
- Candidate: `src/main.ts`
- Output: `VCOMLink("src/main.ts")` + `VCOMText(".")`

## 6. Event Loop & Lifecycle

### 6.1 The Update Cycle

VCOM is **Append-Only**.

1.  **Mutation**: The Kernel emits a chunk.
2.  **Microtask**: The User Agent processes the binding.
3.  **Render**: The User Agent updates the DOM.

### 6.2 The Interaction Task Source

User interactions (clicks) are queued on the **Interaction Task Source**.

1.  **Event**: User clicks `VCOMCommand`.
2.  **Task**:
    - Resolve `command.id`.
    - Check **Capability Whitelist** (Section 7).
    - Execute Command via `vscode.commands.executeCommand`.

## 7. Security Context

### 7.1 The "Same-Origin" Policy

VCOM operates within the trust boundary of the Workspace.

- **Allowed**: Access to files within the workspace.
- **Restricted**: Access to files outside the workspace (requires User Confirmation).

### 7.2 Script Execution

**Constraint**: VCOM DOES NOT support client-side scripting. There is no `eval()`, no `<script>`, and no `onclick` handler that executes arbitrary code. All interactivity is delegated to the **VS Code Command Registry**.

## Annex A: The "Common" Environment (Normative for Compatibility)

This annex defines the set of "Intrinsic Tools" and behaviors that are not strictly part of the VCOM rendering layer but are so pervasive in the training data of Large Language Models (LLMs) that a conforming User Agent MUST support them (or alias them) to prevent hallucination loops.

This is analogous to **Annex B of ECMA-262** (Legacy Web Compatibility).

### A.1 The "Copilot" Toolset

Models trained on GitHub Copilot data or VS Code extension code often exhibit a strong prior bias towards specific tool names. To ensure robust operation, the Literate Kernel SHOULD support the following aliases.

| Canonical Tool (VCOM) | Legacy/Hallucinated Alias     | Parameters (Likely Hallucination) | Behavior                                                                                                                                                         |
| :-------------------- | :---------------------------- | :-------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `listDirectory`       | `list_files`, `ls`, `dir`     | `path`, `recursive`               | Maps to `listDirectory`.                                                                                                                                         |
| `readFile`            | `read_file`, `cat`, `read`    | `path`, `start_line`, `end_line`  | Maps to `readFile`.                                                                                                                                              |
| `editFile`            | `edit_file`, `replace`, `sed` | `path`, `search`, `replace`       | Maps to `editFile`. **Warning**: Models often hallucinate `search`/`replace` semantics instead of `diff`. The Kernel MUST attempt to adapt or reject gracefully. |
| `runCommand`          | `run_cmd`, `exec`, `shell`    | `command`, `cwd`                  | **Security Risk**: Often hallucinated. MUST be gated behind explicit capability negotiation.                                                                     |

### A.2 The "VS Code" API Hallucinations

Models often attempt to call VS Code API methods directly as tools.

- `vscode.workspace.findFiles` -> Should map to `listDirectory` (recursive).
- `vscode.window.showInformationMessage` -> Should be rendered as a standard Markdown blockquote or info box.
- `vscode.commands.executeCommand` -> Should be rendered as a `<exo-cmd>` button if possible, or rejected if it requires auto-execution.

### A.3 The "Thinking" Block

Models trained on Chain-of-Thought (CoT) data often emit `<thought>`, `<thinking>`, or `<scratchpad>` tags.

- **Requirement**: The VCOM Parser MUST treat these tags as **Collapsed Content**.
- **Rendering**: The User Agent SHOULD render this as a `<details>`/`<summary>` block labeled "Thinking..." or similar. It MUST NOT be hidden entirely (to allow for debugging), but it MUST NOT clutter the primary response flow.

### A.4 The "Plan" Block

Models often emit a `<plan>` tag containing a structured list of intended actions.

- **Requirement**: The VCOM Parser MUST treat this as **Flow Content**.
- **Rendering**: The User Agent SHOULD render this as a distinct "Plan" component (e.g., a card or a checklist).
- **Future Work**: A conforming User Agent MAY attempt to parse the plan items and visually check them off as subsequent tools are executed.

### A.5 The "Artifact" Block (Claude-style)

Models trained on Anthropic data often emit `<antArtifact>` tags to encapsulate self-contained content.

- **Pattern**: `<antArtifact identifier="..." type="..." title="...">...</antArtifact>`
- **VCOM Mapping**:
  - If `type="application/vnd.ant.code"`, map to `VCOMText` (Code Block).
  - If `type="text/markdown"`, map to `VCOMText`.
  - **Recommendation**: The User Agent SHOULD render these with a distinct border or header using the `title` attribute.

### A.6 The "Search/Replace" Block (Aider-style)

Models trained on coding agent data often emit "Search/Replace" blocks instead of Unified Diffs.

- **Pattern**:
  ```
  <<<<<<< SEARCH
  [Original Code Context]
  =======
  [New Code]
  >>>>>>> REPLACE
  ```
- **VCOM Mapping**: The Parser MUST transform this construct into a `VCOMTextEdit`. This transformation occurs in the **Deterministic Binding Layer** (Section 5).
- **Resolution Algorithm**:
  1.  **Extraction**: Isolate the `SEARCH` block ($S$) and `REPLACE` block ($R$).
  2.  **Targeting**: Identify the target file from the enclosing `<exo-edit>` context or the most recently referenced file.
  3.  **Matching Strategy**:
      a. **Exact Match**: Attempt to find $S$ in the target file content exactly.
      b. **Flexible Match**: If Exact fails, attempt a **Whitespace-Normalized Match**. \* _Definition_: Strip leading/trailing whitespace from each line of $S$ and the target file. Collapse internal runs of whitespace to a single space. \* _Constraint_: This relaxation applies ONLY to whitespace, not to identifier names or punctuation.
  4.  **Outcome**:
      - **Unique Match**: Calculate the line range and generate a standard `TextEdit`.
      - **No Match / Ambiguous Match**: The User Agent MUST render a **Conflict View**. It MUST NOT auto-apply the edit. The UI should present the "Expected Context" ($S$) alongside the "Actual File Content" to allow the user to manually reconcile.
- **Rationale**: This format is more robust than line numbers but requires the User Agent to handle the "Fuzzy -> Exact" bridging logic.
