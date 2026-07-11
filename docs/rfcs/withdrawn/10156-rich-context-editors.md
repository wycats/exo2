<!-- exo:10156 ulid:01kmzxbczj7zhxdk655bjy4303 -->


# RFC 10156: Rich Context Editors

- **Status**: Withdrawn
- **Stage**: 4
- **Reason**:

## Summary

Provide a rich, interactive "Studio" editing experience for core context files (scoped axioms files in `axioms.*.toml`, plus `decisions.toml`, `plan.toml`) using VS Code's Custom Editor API.

## Motivation

Core context files are structured data (TOML). Editing them as raw text is error-prone and lacks visualization. A "Studio" view allows for better data integrity and a more intuitive interface.

## Detailed Design

### Architecture

- **Extension Side**: Implements `vscode.CustomTextEditorProvider`. Listens for document changes to keep the webview in sync.
- **Webview Side**: Svelte application rendering the UI.
- **Data Flow**: Two-way binding. Changes in UI update the document; changes in document update the UI.

### Components

- **Axiom Editor**: Form-based editor for axioms.
- **Decision Editor**: Form-based editor for decisions.
- **Plan Editor**: Tree-based editor for the project plan.

### Shared Framework

- **Data Model**: Normalized representation.
- **Rendering Model**: Common behaviors (Read-Only by default, Dirty State).
- **Components**: Reusable fields (`TextField`, `EnumField`, `ListField`).

## Alternatives

- **Webview Panel**: Not tied to the file system. Harder to manage dirty state/saving.
- **CodeLens**: Limited interactivity.
