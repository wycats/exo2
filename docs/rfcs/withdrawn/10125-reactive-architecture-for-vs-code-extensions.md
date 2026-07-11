<!-- exo:10125 ulid:01kmzxbd0bmsb70mdyjdyy966b -->


# RFC 10125: Reactive Architecture for VS Code Extensions

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

## Summary

This RFC codifies the architectural patterns discovered during the implementation of the "Reactive VS Code Bindings" (Phase 43). It establishes a standard approach for bridging VS Code's imperative, event-driven API (Push) to Svelte 5's reactive signals (Pull).

## Motivation

VS Code's API is primarily based on `Event<T>` emitters and `Disposable` lifecycles. Modern UI frameworks like Svelte 5 rely on fine-grained reactivity (Signals). To build a robust, reactive extension, we need a consistent set of adapters that handle:

1.  **Synchronization**: Keeping local state in sync with VS Code.
2.  **Lifecycle**: Managing resources (listeners, watchers) to prevent leaks.
3.  **Ergonomics**: Providing a clean, "magic" API for consumers.

## Core Concepts

### 1. The "Push-to-Pull" Adapter

The fundamental unit of this architecture is the **Service** that subscribes to VS Code events and updates a public `$state`.

```typescript
// Pattern
class Service {
  value = $state(initial);
  constructor() {
    vscode.someEvent((e) => (this.value = e));
  }
}
```

### 2. The "Weight" Rule (Lifecycle Strategy)

We bifurcate our lifecycle strategy based on the "weight" (cost) of the underlying VS Code resource.

| Resource Type | Examples                 | Cost            | Strategy          | Implementation                 |
| :------------ | :----------------------- | :-------------- | :---------------- | :----------------------------- |
| **Light**     | Config, Theme, Selection | Memory only     | **Implicit (GC)** | `WeakRef` + `Proxy`            |
| **Heavy**     | File Watchers, Terminals | OS Handles, IPC | **Explicit**      | Ref-Counting (`get`/`dispose`) |

- **Light Resources**: Can rely on Garbage Collection. We use `WeakRef` to allow signals to be collected when no longer used by the UI.
- **Heavy Resources**: Must be explicitly managed. We use Reference Counting to ensure the underlying VS Code resource is disposed immediately when the last consumer disconnects.

### 3. The "Mutability Gap" (Identity vs. State)

VS Code often uses stable object references for mutable entities (e.g., `vscode.TextDocument`).

- **Problem**: A signal holding a `TextDocument` will not trigger updates when the document content changes, because the object identity is the same.
- **Solution**: **Reactive Wrappers**. We wrap the stable object in a class that exposes a `version` signal.

```typescript
class ReactiveDoc {
  #raw = $state(doc);
  #version = $state(doc.version);

  get value() {
    this.#version; // Track dependency
    return this.#raw;
  }
}
```

### 4. Noise Reduction (Debouncing)

For noisy event sources (like File Systems), raw event forwarding causes "Render Storms".

- **Solution**: Internal **Coalescing Timers** (e.g., 50ms) within the adapter to batch updates before notifying the signal.

## Implemented Services

| Service                | Pattern          | Key Features                                                   |
| :--------------------- | :--------------- | :------------------------------------------------------------- |
| `EditorService`        | Simple Signal    | Tracks `activeTextEditor`.                                     |
| `DocumentService`      | Reactive Wrapper | Wraps `TextDocument` to expose content changes via `version`.  |
| `FileSystemService`    | Heavy Resource   | Ref-Counted Watchers + Debouncing.                             |
| `ConfigurationService` | Light Resource   | `Proxy` + `WeakRef` for ergonomic `config.section.key` access. |

## Future Work

- **CommandService**: Declarative command registration.
- **TerminalService**: Reactive terminal management.
