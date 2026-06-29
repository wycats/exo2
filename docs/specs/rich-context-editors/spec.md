# Rich Context Editor Protocol (RCEP)

**Version:** 1.0.0 (Draft)
**Status:** Proposal
**Target:** Exosuit VS Code Extension

## 1. Scope

This specification defines the **Rich Context Editor Protocol (RCEP)**, a mechanism for projecting structured configuration files (specifically TOML) into rich, interactive "Studio" interfaces within VS Code. It defines:

1.  The **Infoset (Data Model)**: The abstract representation of Exosuit context entities (Axioms, Decisions, Plans).
2.  The **Rendering Model**: The abstract UI components and their behaviors.
3.  The **Binding Protocol**: The normative rules for serializing the Infoset to/from the underlying TOML storage.
4.  The **Coherence Model**: The synchronization rules between the Text Document and the Webview.

## 2. Conventions

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be interpreted as described in RFC 2119.

## 3. The Infoset (Data Model)

The Infoset is a normalized, in-memory representation of the context data. It abstracts away the specific serialization format (TOML) to allow for consistent manipulation.

### 3.1 Base Entity

All context entities MUST inherit from the `Entity` interface.

```typescript
interface Entity {
  readonly id: string;
  readonly type: EntityType;
  readonly metadata: Metadata;
  feedback: Feedback[];
}

type EntityType = "axiom" | "decision" | "task" | "phase" | "epoch";

interface Metadata {
  created_at?: string; // RFC 3339
  updated_at?: string; // RFC 3339
  author?: string;
  status: "active" | "archived" | "draft";
}
```

### 3.2 Feedback Model

Feedback (comments) is a first-class citizen in the Infoset.

```typescript
interface Feedback {
  readonly id: string;
  author: string;
  content: string;
  timestamp: string; // RFC 3339
  status: "open" | "proposed" | "resolved";
  context?: string; // Field name or JSON Pointer relative to the Entity
}
```

**Constraint**: `resolved` feedback MUST NOT be persisted in the active document (see Section 5.2).
**Constraint**: Feedback MUST be anchored to a stable Entity ID. Positional anchoring (e.g., line numbers) is FORBIDDEN.

## 4. The Studio Object Model (SOM)

The **Studio Object Model (SOM)** is the DOM for the Rich Context Editor. It is a serializable tree structure that fully describes the UI state, independent of the rendering implementation (React).

### 4.1 The SOM Tree

The SOM is a tree of `SOMNode` objects.

```typescript
type SOMNode = SOMContainer | SOMField | SOMControl;

interface SOMRoot {
  kind: "root";
  schemaVersion: "1.0";
  children: SOMNode[];
  meta: {
    title: string;
    readonly: boolean;
  };
}
```

### 4.2 Containers

Containers provide layout and grouping.

```typescript
interface SOMContainer {
  kind: "section" | "group" | "list";
  id: string;
  label?: string;
  children: SOMNode[];
  collapsed?: boolean; // UI State
}

interface SOMList extends SOMContainer {
  kind: "list";
  itemSchema: SOMNode[]; // Template for new items
  allowAdd: boolean;
  allowReorder: boolean;
}
```

### 4.3 Fields

Fields are bound to specific data paths in the Infoset.

```typescript
interface BaseField {
  id: string;
  kind: string;
  path: string[]; // JSON Pointer segments (e.g., ["axioms", "0", "content"])
  label: string;
  description?: string;
  value: any;
  readonly: boolean;
  errors?: string[]; // Validation feedback
  feedback?: FeedbackSummary; // Active comments
}

interface TextField extends BaseField {
  kind: "text";
  value: string;
  multiline: boolean;
  format: "plain" | "markdown";
}

interface EnumField extends BaseField {
  kind: "enum";
  value: string;
  options: { label: string; value: string }[];
  display: "dropdown" | "radio";
}

interface BooleanField extends BaseField {
  kind: "boolean";
  value: boolean;
}

interface ReferenceField extends BaseField {
  kind: "reference";
  value: string; // Entity ID
  targetType: EntityType;
}
```

### 4.4 Security & Sanitization

The Studio operates in a privileged context. To prevent Remote Code Execution (RCE) and Cross-Site Scripting (XSS):

1.  **Restricted Markdown**: The `TextField` MUST use `unified` (or equivalent) to parse Markdown into the **Rich Text Object Model (RTOM)** (see Section 4.6).
    - **Allowed**: Nodes defined in the RTOM (Section 4.6.1).
    - **Forbidden**: HTML tags (`<script>`, `<iframe>`), unsafe attributes (`onclick`), and `javascript:` URIs.
2.  **Content Security Policy (CSP)**: The Webview MUST enforce a strict CSP that disallows inline scripts and remote resources (except for specific, trusted CDNs if absolutely necessary).

### 4.5 Theming & Styling

To ensure the Studio feels native to VS Code, all UI components MUST adhere to the following styling constraints:

1.  **CSS Variables**: Components MUST use standard VS Code CSS variables for all colors and fonts (e.g., `--vscode-editor-background`, `--vscode-button-background`, `--vscode-editor-foreground`). Hardcoded hex values are FORBIDDEN.
2.  **Font Sizing**: Typography MUST respect the user's editor font settings.
3.  **High Contrast**: Components MUST be tested against VS Code's High Contrast themes to ensure accessibility.

### 4.6 The Rich Text Object Model (RTOM)

To ensure consistent rendering and security, text content is NOT rendered as raw HTML. Instead, it is parsed into a strict **Rich Text Object Model (RTOM)**.

**Normative Reference**: The Data Model and Parsing Rules are defined in the [RTD Architecture](../architecture.md).

- **Object Model**: See [RTD Object Model (RTOM)](../rtd/model.md).
- **Streaming**: See [RTD Streaming Protocol](../rtd/streaming.md).

**Scope Note**: The RTOM describes the _content_ of a field. External UI controls (like Feedback threads, "Copy" buttons, or validation icons) are part of the **SOM Field Wrapper** (Section 4.3), not the RTOM.

## 5. The Binding Protocol (Persistence)

This section defines how the Infoset is mapped to the underlying TOML files.

### 5.1 TOML Serialization & Binding

The Source of Truth is the TOML file. The Binding Protocol defines how SOM paths map to TOML locations.

- **Path Resolution**: The `path` property in SOM fields corresponds to the **TOML Key Path**.
  - Example: `["axioms", "0", "content"]` maps to `axioms[0].content`.
- **Round-Tripping**: The serializer MUST preserve the semantic structure of the data.
- **Formatting**: The serializer SHOULD attempt to preserve the formatting style of the document (using a CST-based parser like `taplo` or `toml-edit`), but strict whitespace preservation is NOT required.
- **Schema Validation**: All data written to disk MUST be validated against the strict Zod schemas defined in `@exosuit/core`.

### 5.2 Validation Strategy

Validation occurs in two stages to ensure both responsiveness and integrity.

1.  **Client-Side (Immediate)**:
    - The Webview SHOULD perform basic format validation (e.g., required fields, type checks) before sending an `edit` message.
    - Invalid fields SHOULD be visually marked (e.g., red border) and prevent submission if possible.
2.  **Host-Side (Authoritative)**:
    - The Extension Host MUST validate the full object against the Zod schema before writing to disk.
    - If validation fails (e.g., complex cross-field constraints), the Host MUST reject the edit and send an `error` message back to the Webview.

### 5.3 Hybrid Feedback Storage

To maintain a clean context while preserving history, feedback is stored using a **Hybrid Strategy**.

1.  **Active Feedback (Hot)**:

    - **Storage**: Stored directly in the TOML file.
    - **Location**: Within a `[[feedback]]` table array or a `feedback` field on the entity.
    - **Purpose**: Immediate context for the Agent and User.

2.  **Resolved Feedback (Cold)**:
    - **Storage**: Marked resolved in Exo state and surfaced through filtered views.
    - **Trigger**: When a feedback item's status transitions to `resolved`.
    - **Purpose**: Audit trail and historical context.

**Transition Rule**: Archiving resolved feedback MUST be an **explicit Agent Action** (e.g., "Archive Resolved Feedback"). It MUST NOT happen implicitly on document save to prevent data loss or accidental archiving.

### 5.4 Document-to-SOM Mapping

The Host MUST implement a **Mapper** that transforms the raw TOML AST into a `SOMRoot`.

**Normative Reference**: The detailed algorithms for Schema Detection, Root Generation, and Structure Mapping are defined in the **[RCEP Mapping Specification](./mapping-spec.md)**. The Host MUST conform to the algorithms defined therein.

## 6. Coherence Model

The "Studio" (Webview) and the "Document" (Text Model) must remain in sync. This synchronization is managed by the **Studio Object Model (SOM)**, which acts as the bridge between the VS Code `CustomTextEditorProvider` and the React-based UI.

### 6.1 The Custom Editor Lifecycle (SOM Integration)

The implementation MUST use the VS Code `CustomTextEditorProvider` API, mediated by the SOM Host.

#### 6.1.1 The Message Protocol

The Host and Webview communicate via a strict message passing protocol.

**Host -> Webview (`HostMessage`)**

```typescript
type HostMessage =
  | { type: "update"; tree: SOMRoot }
  | { type: "patch"; ops: JSONPatch[] } // Optimization for small updates
  | { type: "error"; message: string };
```

**Webview -> Host (`WebviewMessage`)**

```typescript
type WebviewMessage =
  | { type: "ready" } // Webview is initialized
  | { type: "edit"; path: string[]; value: any } // Field update
  | { type: "action"; id: string; args?: any[] } // Button click / Command
  | {
      type: "feedback";
      action: "create" | "reply" | "resolve";
      target: string;
      content: string;
    };
```

#### 6.1.2 The Synchronization Loop

1.  **Document Change (External)**:

    - **Trigger**: User edits text directly, Git pull, or Agent modification.
    - **Action**: The `CustomTextEditorProvider` receives a `onDidChangeTextDocument` event.
    - **SOM Host**: Parses the TOML into the Infoset and computes the SOM Tree.
    - **Webview**: The Host sends a `update` message with the serialized SOM Tree. The Webview re-renders.

2.  **Webview Change (Internal)**:
    - **Trigger**: User interacts with a control in the Studio.
    - **Action**: The Webview sends a `edit` message to the Extension Host.
    - **SOM Host**:
      1.  Receives `edit` message.
      2.  Locates the target node in the TOML AST using the `path`.
      3.  Applies the modification to the AST.
      4.  Generates a `WorkspaceEdit` and applies it to the `TextDocument`.
    - **Result**: VS Code marks the document as "Dirty". The user can then Save or Undo using standard VS Code commands.
    - **Concurrency**: For v1, the Host applies a **"Last Write Wins"** policy. If the document has changed on disk since the Webview last rendered, the Host MAY reject the edit if it conflicts with a specific field, but generally relies on VS Code's internal dirty-state handling to manage file-level conflicts.

### 6.2 The "Source of Truth" Axiom

**Axiom**: The Text Document is the absolute Source of Truth. The Webview is merely a projection.

- If the Text Document is invalid (parse error), the Webview MUST display an "Error State" and disable editing controls until the document is valid.
- The Webview MUST NOT maintain internal state that is not represented in the Text Document (except for transient UI state like "expanded/collapsed" sections).

## Annex A: Schema Definitions (Informative)

### A.1 Axiom Schema

```toml
[[axioms]]
id = "axiom-1"
content = "Context is King"
category = "philosophy"
status = "active"

[[axioms.feedback]]
id = "fb-1"
author = "User"
content = "Should we clarify this?"
status = "open"
```

### A.2 Decision Schema

```toml
[[decisions]]
id = "dec-1"
title = "Use TOML for configuration"
status = "approved"
context = "We need a format that is both human-readable and machine-writable."
implications = ["Requires a robust TOML parser."]
```
