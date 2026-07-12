<!-- exo:10144 ulid:01kmzxbd0a18h3nhw04tnhpg1w -->


# RFC 10144: Reactive Bridge Protocol

- **Status**: Withdrawn
- **Stage**: 3
- **Reason**:

## Summary

Defines a standard protocol for synchronizing reactive state between the VS Code Extension Host (Svelte 5 Runes) and Webviews (Svelte 5 Stores/Runes) using a `Bridge` pattern.

## Motivation

We have implemented `services/*.svelte.ts` in the Extension Host, which use Svelte 5 signals (`$state`) to track VS Code API state (Configuration, Active Editor, etc.). However, the UI runs in a Webview, which is an isolated environment.

We need a way to "pipe" these signals into the Webview so that the UI updates automatically when the underlying VS Code state changes, without writing ad-hoc `postMessage` code for every feature.

## The Problem

1.  **Isolation**: Extension Host and Webview share no memory.
2.  **Push vs Pull**: VS Code APIs are often event-based (Push), but UI components often want to "pull" data or subscribe to a store.
3.  **Glitch Freedom**: We must ensure that the Webview doesn't display stale data or flicker between states.

## Proposed Solution: The Bridge Pattern

We introduce a `Bridge` that connects a **Source** (Extension Host Signal) to a **Target** (Webview Store).

### 1. The Protocol (Messages)

All bridge messages follow a standard envelope:

```typescript
type BridgeMessage<T> = {
  type: "BRIDGE_SYNC";
  payload: {
    key: string; // e.g., "config.exosuit.theme" or "editor.active"
    value: T;
    version: number; // For ordering/glitch prevention
  };
};
```

### 2. The Host Side (The Transmitter)

A `BridgeTransmitter` observes a Svelte signal and broadcasts changes to the Webview. Note that this file must be a `.svelte.ts` file to use Svelte Runes (`$effect`) in the Extension Host environment.

```typescript
// packages/exosuit-vscode/src/bridge/Transmitter.svelte.ts
export class BridgeTransmitter {
  constructor(private webview: vscode.Webview) {}

  // Pipe a signal to the webview
  pipe<T>(key: string, signal: () => T) {
    $effect(() => {
      const value = signal();
      this.webview.postMessage({
        type: "BRIDGE_SYNC",
        payload: { key, value, version: Date.now() },
      });
    });
  }
}
```

### 3. The Webview Side (The Receiver)

A `BridgeReceiver` listens for messages and updates a local Svelte Store. Services in the Webview then subscribe to this store to update their own local `$state` runes, creating a "Store-to-Rune" adapter pattern.

```typescript
// packages/exosuit-vscode/src/webview/bridge/Receiver.ts
export class BridgeReceiver {
  private stores = new Map<string, Writable<any>>();

  // ... handleMessage implementation ...

  // Usage in Service:
  // this.bridge.use('key', default).subscribe(val => this.state = val);
  use<T>(key: string, defaultVal: T): Readable<T> {
    // ... return store
  }
}
```

## Implementation Notes

-   **Shared Types**: Types are located in `packages/exosuit-vscode/src/types/bridge.ts` to ensure accessibility by both the Extension Host (Node) and Webview (Browser) TS projects without path alias conflicts.
-   **Svelte Files**: The Transmitter must be a `.svelte.ts` file.
-   **Store-to-Rune Adapter**: While the Receiver uses Stores (easier for event-based updates), the consuming Services convert these back to Runes (`$state`) for idiomatic Svelte 5 usage in components.

## Implementation Plan

1.  **Define Types**: Create `packages/exosuit-vscode/src/types/bridge.ts`.
2.  **Implement Transmitter**: Create `packages/exosuit-vscode/src/bridge/Transmitter.svelte.ts`.
3.  **Implement Receiver**: Create `packages/exosuit-vscode/src/webview/bridge/Receiver.ts`.
4.  **Wire up Dashboard**: Refactor `DashboardProvider` to use `Transmitter` to sync the Plan and Phase state.


## Open Questions

- **Initial Sync**: How does the Webview request the initial state upon connection? (Likely a `BRIDGE_HELLO` handshake).
- **Two-Way Binding**: Do we need to send updates _back_ to the host? (For now, assume One-Way Data Flow: Host -> Webview).
