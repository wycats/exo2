# Design Review: Reactive FileSystem Binding (Svelte 5)

## Context

We are implementing the `FileSystemService` for `packages/exosuit-vscode`.
This service bridges `vscode.workspace.createFileSystemWatcher` (Push) to a Svelte 5 Reactive Interface (Pull).

## Proposed Design

We propose a **Factory Pattern** where the service dispenses `ReactiveWatcher` instances.

### 1. ReactiveWatcher

Represents a subscription to a glob pattern.

```typescript
export class ReactiveWatcher {
  // Signal: Increments on Create/Change/Delete
  // Downstream computations derive from this to invalidate their cache.
  version = $state(0);

  // The last event (optional, for debugging or fine-grained logic)
  lastEvent = $state<
    { type: "create" | "change" | "delete"; uri: vscode.Uri } | undefined
  >();

  #watcher: vscode.FileSystemWatcher;
  #disposables: vscode.Disposable[] = [];

  constructor(pattern: string) {
    this.#watcher = vscode.workspace.createFileSystemWatcher(pattern);

    const notify = (type: "create" | "change" | "delete", uri: vscode.Uri) => {
      this.version += 1;
      this.lastEvent = { type, uri };
    };

    this.#disposables.push(
      this.#watcher.onDidCreate((u) => notify("create", u)),
      this.#watcher.onDidChange((u) => notify("change", u)),
      this.#watcher.onDidDelete((u) => notify("delete", u))
    );
  }

  dispose() {
    this.#disposables.forEach((d) => d.dispose());
    this.#watcher.dispose();
  }
}
```

### 2. FileSystemService

Manages the lifecycle of watchers to ensure we don't create duplicate watchers for the same pattern.

```typescript
export class FileSystemService {
  #watchers = new Map<string, ReactiveWatcher>();

  watch(pattern: string): ReactiveWatcher {
    if (this.#watchers.has(pattern)) {
      return this.#watchers.get(pattern)!;
    }

    const watcher = new ReactiveWatcher(pattern);
    this.#watchers.set(pattern, watcher);
    return watcher;
  }

  dispose() {
    this.#watchers.forEach((w) => w.dispose());
    this.#watchers.clear();
  }
}

export const fileSystemService = new FileSystemService();
```

## Validation Questions

1.  **Scalability**: Is creating a `FileSystemWatcher` per pattern scalable in VS Code? Should we limit this?
2.  **Debouncing**: File systems often emit multiple events for a single "save". Should the `ReactiveWatcher` implement debouncing internally, or leave that to the consumer?
3.  **Consistency**: Does this `version` signal approach align with the `ReactiveDoc` design (which also uses a version signal)?
4.  **Reference Counting**: Currently, `watch()` returns a persistent instance. Should we implement reference counting to `dispose()` watchers when no longer used?

## Task

Please review the proposed design and provide feedback on:

- Resource usage (VS Code watcher limits).
- Event noise handling.
- Lifecycle management (Ref counting vs. Global cache).
