# Design Review: Reactive VS Code Bindings (Svelte 5)

## Context

We are implementing "Reactive Bindings" for the VS Code Extension API in `packages/exosuit-vscode`.
The goal is to bridge the **Push-Based** VS Code API (Events) to a **Pull-Based** Reactive Interface (Signals) using **Svelte 5 Runes**.

## Proposed Design

We propose using a **Class-Based Service Pattern** to expose VS Code state.

### 1. Editor Binding

```typescript
// src/bindings/Editor.svelte.ts
import * as vscode from "vscode";

export class EditorService {
  // State: The current active editor
  active = $state<vscode.TextEditor | undefined>(
    vscode.window.activeTextEditor
  );

  constructor() {
    // Adapter: Listen to VS Code event and update state
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      this.active = editor;
    });
  }
}

export const editor = new EditorService();
```

### 2. Document Binding

```typescript
// src/bindings/Document.svelte.ts
import * as vscode from "vscode";

export class DocumentService {
  // State: Map of URI -> TextDocument
  documents = $state<Map<string, vscode.TextDocument>>(new Map());

  constructor() {
    // Initial population
    vscode.workspace.textDocuments.forEach((doc) => {
      this.documents.set(doc.uri.toString(), doc);
    });

    // Adapters
    vscode.workspace.onDidOpenTextDocument((doc) => {
      this.documents.set(doc.uri.toString(), doc);
    });
    vscode.workspace.onDidCloseTextDocument((doc) => {
      this.documents.delete(doc.uri.toString());
    });
  }

  get(uri: string) {
    return this.documents.get(uri);
  }
}
```

## Validation Questions

1.  **Correctness**: Does this pattern correctly implement the "Push-to-Pull" adapter pattern?
2.  **Memory Leaks**: Are there potential memory leaks if we don't explicitly dispose of the event listeners? (Note: These are singletons, so they live for the extension lifetime).
3.  **Granularity**: Is `$state` on the `Map` sufficient, or do we need finer-grained reactivity for individual documents?
4.  **Idioms**: Is this the most "Svelte 5" way to handle external event sources?

## Task

Please review the proposed design and provide feedback on:

- Theoretical soundness (Push-to-Pull conversion).
- Implementation details (Svelte 5 Runes usage).
- Potential pitfalls (Performance, Memory).
