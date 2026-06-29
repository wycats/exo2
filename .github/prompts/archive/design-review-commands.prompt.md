# Design Review: Reactive Command Binding (Svelte 5)

## Context

We are implementing the `CommandService` for `packages/exosuit-vscode`.
Unlike other services which expose _state_, this service manages _actions_.

## Problem

In VS Code, commands are registered globally via `vscode.commands.registerCommand`, returning a `Disposable`.
In a component-based UI (Svelte), we often want to register commands that are:

1.  **Scoped**: Only active when a specific component is mounted.
2.  **Context-Aware**: Have access to the component's local state.

## Proposed Design

We propose a **Declarative Registration** pattern using a helper function (or method) that ties into the Svelte lifecycle.

### 1. CommandService

Acts as a central registry (mostly for debugging/introspection), but primarily facilitates registration.

```typescript
import * as vscode from "vscode";

export class CommandService {
  #commands = new Set<string>();
  #disposables: vscode.Disposable[] = [];

  /**
   * Registers a command that is automatically disposed when the cleanup function is called.
   * Designed to be used with $effect or onMount in Svelte.
   */
  register(command: string, callback: (...args: any[]) => any): () => void {
    if (this.#commands.has(command)) {
      console.warn(
        `Command "${command}" is already registered (or overwriting).`
      );
    }

    const disposable = vscode.commands.registerCommand(command, (...args) => {
      console.log(`[Command] Executing ${command}`);
      return callback(...args);
    });

    this.#commands.add(command);
    this.#disposables.push(disposable);

    // Return cleanup function
    return () => {
      disposable.dispose();
      this.#commands.delete(command);
      const idx = this.#disposables.indexOf(disposable);
      if (idx !== -1) this.#disposables.splice(idx, 1);
    };
  }

  dispose() {
    this.#disposables.forEach((d) => d.dispose());
    this.#commands.clear();
  }
}

export const commandService = new CommandService();
```

### 2. Usage in Svelte Component

We can create a Svelte Action or just use `$effect`.

```typescript
// In a .svelte component
<script>
    import { commandService } from '../services/CommandService.svelte';

    let count = $state(0);

    $effect(() => {
        // Register command when component mounts
        const cleanup = commandService.register('exosuit.increment', () => {
            count += 1;
            vscode.window.showInformationMessage(`Count: ${count}`);
        });

        // Unregister when component unmounts
        return cleanup;
    });
</script>
```

## Validation Questions

1.  **Global Namespace**: VS Code commands are global. If two instances of a component mount, they will try to register the same command ID. How should we handle this?
    - _Option A_: Throw error.
    - _Option B_: Auto-generate IDs (e.g., `exosuit.increment.1`).
    - _Option C_: Allow overwriting (VS Code allows this, but warns).
2.  **Menu Items**: Commands often need `package.json` entries for menus. Dynamic commands won't appear in menus unless defined in `package.json`. Is this purely for _internal_ or _keybinding-driven_ commands?
3.  **Proxy**: Should we expose `executeCommand` via a proxy for ergonomics? `commands.exosuit.increment()`?

## Task

Please review the proposed design and provide feedback on:

- Handling command ID collisions in a component model.
- The utility of dynamic registration vs. static `package.json` registration.
