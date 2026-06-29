<!-- exo:10145 ulid:01kmzxefd9pmd128qte78c0d1a -->


# RFC 10145: Robust Extension Architecture

- **Status**: Stage 3 (Candidate)
- **Created**: 2025-12-08
- **Tags**: architecture, vscode, testing, reactivity

## Summary

This RFC codifies two critical architectural principles discovered during the development and testing of the Exosuit VS Code extension (specifically Phase 46). These principles are essential for ensuring the stability, testability, and correctness of the extension.

1.  **Strict Runtime Separation**: The Extension Host and Webview runtimes must be kept strictly separate. Svelte (and other UI-specific libraries) must be confined to the Webview. The Extension Host must remain "Logic-Heavy" but "UI-Framework-Agnostic".
2.  **API Type Fidelity**: When interacting with VS Code APIs, always prefer passing rich objects (like `vscode.WorkspaceFolder`) over primitive representations (like string paths) to ensure correct behavior across all environments (Development, Production, Test).

## Motivation

### The "Svelte in Node" Crash
During Phase 46, we attempted to use `.svelte.ts` files (containing Svelte 5 runes) within the Extension Host process to share reactive logic between the backend and frontend. This caused the extension to crash or fail activation because the Svelte compiler/runtime for runes is not fully compatible with the Node.js environment in the VS Code Extension Host context.

### The "Silent Watcher" Bug
We encountered a persistent failure in E2E tests where `vscode.FileSystemWatcher` would fail to trigger `onDidCreate` events. This was traced to initializing `RelativePattern` with a string path (`folder.uri.fsPath`) instead of the `vscode.WorkspaceFolder` object. While the string path worked in the development window, it failed in the `vscode-test-electron` environment, likely due to internal VS Code logic that relies on object identity or specific workspace context attached to the folder object.

## Design Principles

### 1. Strict Runtime Separation (Host vs. View)

**Principle**: The Extension Host and Webview are distinct computing environments with different capabilities and constraints. They must not share code that depends on environment-specific runtimes (like the DOM or Svelte).

**Rules**:
*   **Extension Host (Node.js)**:
    *   Must be written in pure TypeScript or Rust.
    *   **Forbidden**: Importing `.svelte` components or `.svelte.ts` files that rely on the Svelte runtime.
    *   **Responsibility**: Business logic, File System operations, VS Code API interactions, Data processing.
    *   **State**: Manages the "Source of Truth".
*   **Webview (Browser/Electron Renderer)**:
    *   May use Svelte (and Runes).
    *   **Responsibility**: Rendering UI, User Interaction.
    *   **State**: Reflects the state provided by the Extension Host.
*   **The Bridge**:
    *   Communication happens *only* via message passing (JSON-serializable payloads).
    *   Shared code (in `shared/` or similar) must be strictly platform-agnostic (e.g., pure interfaces, DTOs, utility functions).

**Implication**:
Services running in the Extension Host (e.g., `DashboardService`) must be implemented as Plain Old TypeScript Objects (POTOs). They cannot use Svelte runes for reactivity. If reactivity is needed within the Extension Host, it should use a platform-agnostic mechanism (like `EventEmitter` or a custom observable pattern) or simply rely on the VS Code event loop.

### 2. API Type Fidelity (Object > String)

**Principle**: When a VS Code API accepts a rich object (e.g., `vscode.WorkspaceFolder`, `vscode.Uri`) or a primitive (e.g., `string`), **always prefer the rich object**.

**Why**:
*   **Context**: Rich objects often carry hidden context or metadata that VS Code uses internally for optimization, scoping, or correct event dispatching.
*   **Reliability**: Primitives (like string paths) are brittle. They can suffer from normalization issues (separators, casing) and lose the connection to the logical workspace structure.
*   **Testing**: The `vscode-test-electron` environment is often stricter or subtly different from the standard window. Using the official object types ensures the test runner behaves identically to the production environment.

**Rule**:
*   **RelativePattern**: Always use `new vscode.RelativePattern(workspaceFolder, pattern)`. Never use `new vscode.RelativePattern(pathString, pattern)`.
*   **URIs**: Pass `vscode.Uri` objects instead of `fsPath` strings whenever an API supports it.

## Implementation Strategy

1.  **Refactor**: Ensure all existing services in `packages/exosuit-vscode/src/` are free of Svelte dependencies.
2.  **Lint/Check**: (Optional) Add a lint rule or build check to prevent importing `.svelte` files into the `src/` (extension host) directory.
3.  **Pattern Enforcement**: During code review, flag any use of `fsPath` strings where a `Uri` or `WorkspaceFolder` could be used in a VS Code API call.

## Context Updates

This RFC updates the following context:
*   **Axioms**: Adds/Refines axioms regarding Architecture and Testing.
*   **Manual**: Updates `docs/manual/architecture/` to reflect the strict separation.