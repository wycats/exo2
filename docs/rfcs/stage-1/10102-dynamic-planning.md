<!-- exo:10102 ulid:01kmzxey0a3r4jz0pn7ef5zyhf -->


# RFC 10102: Dynamic Planning

- **Superseded by**: RFC 0009


---
status: provisional
---

# Design: Dynamic Phase Management

## Context

Project plans are living documents. As we iterate, we often discover that a single phase is too large and needs to be split, or that we need to insert a new "Research" phase before an "Implementation" phase.

Currently, `plan-outline.md` is a static Markdown file. Inserting a phase requires manually:

1.  Adding the new header.
2.  Renumbering subsequent phases (e.g., Phase 4 becomes Phase 5).
3.  Ensuring the Epoch structure remains valid.

This friction discourages keeping the plan up-to-date.

## Goals

1.  **Frictionless Insertion**: Allow the user (or agent) to say "Insert a new phase for X after Phase 3" and have the system handle the restructuring.
2.  **Automatic Renumbering**: Ensure phase numbers remain sequential and logical.
3.  **Epoch Awareness**: Respect the Epoch hierarchy (e.g., inserting a phase in Epoch 2 shouldn't break Epoch 3).

## Proposed Solution

We will treat `plan-outline.md` not as a text file, but as a structured data model that happens to be serialized as Markdown.

### 1. The Data Model

We can parse the markdown into a structure like:

```typescript
interface Plan {
  epochs: Epoch[];
}

interface Epoch {
  title: string;
  phases: Phase[];
}

interface Phase {
  id: string; // e.g., "Phase 4"
  title: string;
  status: "Completed" | "In Progress" | "Proposed";
  items: string[]; // The checklist items
}
```

### 2. The Operations

We need a script (e.g., `scripts/agent/plan-mod.ts`) that supports:

- **`insert`**: `insert --after "Phase 3" --title "New Phase" --epoch "Epoch 2"`
- **`split`**: `split "Phase 4" --at-item 3` (Splits a phase into two at a specific task).
- **`renumber`**: Walks the tree and normalizes phase numbers (Phase 1, 2, 3...).

### 3. Implementation Strategy

#### A. Parsing (Markdown AST)

Instead of fragile Regex, we should use a Markdown parser (like `marked` or `remark`) to build the AST.

- **Heading 2 (`##`)**: Starts an Epoch.
- **Heading 3 (`###`)**: Starts a Phase.
- **List Items**: Belong to the current Phase.

#### B. Serialization

We need a "Markdown Stringifier" that takes our Model and writes it back to `plan-outline.md` with consistent formatting.

### 4. Integration

- **VS Code**: A command `Exosuit: Insert Phase` that prompts for details and runs the script.
- **Agent**: A tool `exosuit_plan_insert` that the agent can call when it realizes the plan needs to change.

## Future Work

- **Phase 4.6**: Build the `plan-mod` script using `remark`.
- **Phase 5**: Expose this via the VS Code extension UI (e.g., a "+" button in the Project Plan view).
