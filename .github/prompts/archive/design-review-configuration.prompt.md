# Design Review: Reactive Configuration Binding (Svelte 5)

## Context

We are implementing the `ConfigurationService` for `packages/exosuit-vscode`.
This service bridges `vscode.workspace.onDidChangeConfiguration` (Push) to a Svelte 5 Reactive Interface (Pull).

## Proposed Design

We propose a **Signal-per-Key** pattern.

### 1. ConfigurationService

Manages a single global listener for configuration changes and dispatches updates to registered signals.

```typescript
import * as vscode from "vscode";

class ConfigSignal<T> {
  version = $state(0);
  #section: string;
  #key: string;
  #value: T | undefined;

  constructor(section: string, key: string) {
    this.#section = section;
    this.#key = key;
    this.#update();
  }

  #update() {
    const config = vscode.workspace.getConfiguration(this.#section);
    this.#value = config.get<T>(this.#key);
  }

  // Called by Service when configuration changes
  check(e: vscode.ConfigurationChangeEvent) {
    if (e.affectsConfiguration(`${this.#section}.${this.#key}`)) {
      this.#update();
      this.version += 1;
    }
  }

  get value() {
    return this.#value;
  }
}

export class ConfigurationService {
  #signals = new Set<ConfigSignal<any>>();
  #disposables: vscode.Disposable[] = [];

  constructor() {
    this.#disposables.push(
      vscode.workspace.onDidChangeConfiguration((e) => {
        for (const signal of this.#signals) {
          signal.check(e);
        }
      })
    );
  }

  get<T>(section: string, key: string) {
    // Note: We might want to cache these signals to share them if requested multiple times?
    // For now, returning a new one (or we could use a Map<string, ConfigSignal>).

    const signal = new ConfigSignal<T>(section, key);
    this.#signals.add(signal);

    // Return a read-only interface
    return {
      get value() {
        // Track dependency
        const _ = signal.version;
        return signal.value;
      },
      dispose: () => {
        this.#signals.delete(signal);
      },
    };
  }

  dispose() {
    this.#disposables.forEach((d) => d.dispose());
    this.#signals.clear();
  }
}

export const configurationService = new ConfigurationService();
```

## Validation Questions

1.  **Granularity**: Is `affectsConfiguration` efficient enough to run for every active signal on every config change?
2.  **Caching**: Should we cache signals by `${section}.${key}` to avoid duplicates?
3.  **Proxy**: Would a Proxy-based API be better? e.g. `config.exosuit.someSetting`? (Might be hard to type).

## Task

Please review the proposed design and provide feedback on:

- Performance of `affectsConfiguration` loop.
- API ergonomics.
- Caching strategy.
