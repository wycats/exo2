import type { BridgeMessage } from "../../types/bridge";
import { getWebviewLogger } from "../lib/logger";

const logger = getWebviewLogger("bridge");

/**
 * A simple writable store that doesn't depend on Svelte's reactive system.
 * This allows us to create stores outside of component context.
 */
interface SimpleWritable<T> {
  set(value: T): void;
  update(fn: (value: T) => T): void;
  subscribe(fn: (value: T) => void): () => void;
}

function createSimpleWritable<T>(initial: T): SimpleWritable<T> {
  let value = initial;
  const subscribers = new Set<(value: T) => void>();

  return {
    set(newValue: T) {
      value = newValue;
      subscribers.forEach((fn) => fn(value));
    },
    update(fn: (value: T) => T) {
      value = fn(value);
      subscribers.forEach((fn) => fn(value));
    },
    subscribe(fn: (value: T) => void) {
      subscribers.add(fn);
      fn(value); // Call immediately with current value
      return () => subscribers.delete(fn);
    },
  };
}

export interface Readable<T> {
  subscribe(fn: (value: T) => void): () => void;
}

export class BridgeReceiver {
  private stores = new Map<string, SimpleWritable<any>>();

  constructor() {
    window.addEventListener("message", (event) => {
      this.handleMessage(event.data);
    });
  }

  private handleMessage(message: any) {
    logger.trace(
      "[BridgeReceiver] Received message:",
      message.type,
      message.payload?.key,
    );
    if (message.type === "BRIDGE_SYNC") {
      const { key, value } = (message as BridgeMessage<any>).payload;
      this.getStore(key).set(value);
    }
  }

  private getStore(key: string): SimpleWritable<any> {
    if (!this.stores.has(key)) {
      this.stores.set(key, createSimpleWritable(undefined));
    }
    return this.stores.get(key)!;
  }

  use<T>(key: string, defaultValue?: T): Readable<T> {
    const store = this.getStore(key);
    if (defaultValue !== undefined) {
      // Initialize with default if undefined
      store.update((v) => (v === undefined ? defaultValue : v));
    }
    return { subscribe: store.subscribe };
  }
}
