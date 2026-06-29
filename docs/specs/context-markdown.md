# Exosuit Context Markdown Specification

This document defines the strict Markdown structure used for Exosuit's context files (`plan-outline.md` and `task-list.md`). The goal is to ensure reliable parsing by the `exosuit-core` library while maintaining human readability.

## 1. Project Plan (`plan-outline.md`)

The Project Plan tracks the high-level structure of the project (Epochs and Phases).

### Structure

- **Epochs**: Defined by Level 2 Headings (`##`).
- **Phases**: Defined by Level 3 Headings (`###`) nested under an Epoch.
- **Status**: Indicated by parenthetical text at the end of the heading.

### Format

```markdown
## Epoch {N}: {Title} ({Status})

**Goal**: {Description}

### Phase {M}: {Title} ({Status})

- [ ] {High-Level Task} (Optional)
```

### Parsing Rules

1.  **IDs**:

    - **Preferred**: Explicit HTML comment `<!-- id: "my-id" -->` immediately following the title text.
    - **Fallback**: Auto-generated from the title (kebab-case, alphanumeric only).
    - _Note_: The parser must handle both. Tools should preserve explicit IDs if present.

2.  **Status**:

    - `(Active)` -> `in-progress`
    - `(Completed)` -> `done`
    - `(Proposed)` / `(Status)` / Missing -> `todo`

3.  **Active Phase Rule**:

    - Exactly **one** phase in the entire plan must be marked as `(Active)` at any given time.
    - This marker is used by tooling (e.g., "Scroll to Current Phase") to identify the current context.

4.  **Lossiness**:
    - **Preserved**: Headings, Status, Goal paragraphs.
    - **Ignored**: Arbitrary text between phases (except the Goal paragraph immediately following an Epoch).

## 2. Task List (`task-list.md`)

The Task List tracks the granular work for the _current_ phase.

### Structure

- **Tasks**: Unordered list items with checkboxes (`- [ ]` or `- [x]`).
- **Nesting**: Indented list items represent sub-tasks.
- **Formatting**: Tasks often use **Bold** prefixes for categorization.

### Format

```markdown
# Phase Task List

- [x] **Category Name**
  - [x] Task description.
  - [ ] Another task.
- [ ] **Another Category**
  - [ ] Task with explicit ID <!-- id: "task-123" -->
```

### Parsing Rules

1.  **Title Extraction**:

    - The parser must extract the "human-readable" text.
    - **Bold Text**: If a task starts with bold text (e.g., `**Category**`), it should be treated as part of the title. The parser should strip the markdown syntax (`**`) for the raw title but may preserve it for display if supported.
    - _Behavior_: The parser recursively extracts text from nested nodes (like `strong` or `emphasis`) to ensure clean titles.

2.  **IDs**:

    - **Preferred**: Explicit `<!-- id: "..." -->`.
    - **Fallback**: Auto-generated from the full text content of the line.

3.  **Status**:
    - `- [x]` -> `done`
    - `- [ ]` -> `todo`
    - `- [/]` (if supported) -> `in-progress`

## 3. Tooling Compliance

All tools (VS Code extension, shell scripts) must adhere to this spec when reading or writing these files.

- **Reading**: Must be robust to missing IDs (generate them deterministically).
- **Writing**: Should prefer adding explicit IDs to prevent drift if titles change.
