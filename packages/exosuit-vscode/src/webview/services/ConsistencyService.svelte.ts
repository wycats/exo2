/**
 * ConsistencyService.svelte.ts
 *
 * Implements the "Map of Signals" pattern to ensure fine-grained reactivity.
 *
 * Architecture:
 * 1. Registry: A `Map<string, SignalBox>` holds independent reactive states.
 * 2. Isolation: Updating one SignalBox ONLY triggers components that read that specific box.
 * 3. Consumption: `useRoot(id)` returns the SignalBox. Svelte tracks access to properties on the box.
 */

export interface RootState<T = any> {
  value: T | null;
  status: "pending" | "ready" | "error";
  error?: string;
}

let vscodeApi: any = null;

export function setVsCodeApi(api: any) {
  vscodeApi = api;
}

class ConsistencyService {
  // The Registry: Map of RootID -> Independent Signal
  // We use a standard Map. The Map itself is NOT reactive.
  // The values inside are reactive objects created with $state.
  private roots = new Map<string, { state: RootState }>();

  /**
   * Get a handle to a root's signal.
   * This is safe to call from anywhere. It doesn't trigger a read.
   * If the root doesn't exist, it creates a new "pending" signal.
   */
  getRoot<T>(id: string) {
    if (!this.roots.has(id)) {
      // Create a new independent signal box
      // $state is a rune that creates a reactive object
      const box = $state({
        value: null,
        status: "pending",
      } as RootState<T>);

      this.roots.set(id, { state: box });

      // TODO: Send message to Extension Host to subscribe to this root
      // bridge.send('subscribe', { id });
    }
    return this.roots.get(id)!;
  }

  /**
   * Called when we receive an update from the Extension Host.
   * This is the "Writer" side of the signal.
   */
  updateRoot(id: string, value: any) {
    // We get the box (creating it if it doesn't exist)
    const box = this.getRoot(id).state;

    // Fine-grained update:
    // This assignment ONLY invalidates effects that read `box.value` or `box.status`.
    // It does NOT invalidate effects that read other roots.
    box.value = value;
    box.status = "ready";
  }

  setError(id: string, error: string) {
    const box = this.getRoot(id).state;
    box.status = "error";
    box.error = error;
  }

  invalidateRoot(id: string) {
    const box = this.getRoot(id).state;
    // Mark as pending/stale
    // We might want a 'stale' status if we want to show old data while fetching
    box.status = "pending";

    // Request update from Extension Host
    if (vscodeApi) {
      vscodeApi.postMessage({
        type: "FETCH_ROOT",
        payload: { id },
      });
    }
  }
}

// Initialize Message Listener
window.addEventListener("message", (event) => {
  const message = event.data;
  if (message.type === "ROOTS_INVALIDATED") {
    const roots = message.payload as string[];
    roots.forEach((id) => consistency.invalidateRoot(id));
  } else if (message.type === "ROOT_UPDATED") {
    const { id, value } = message.payload;
    consistency.updateRoot(id, value);
  }
});

export const consistency = new ConsistencyService();

/**
 * The Composable for Components.
 * Usage:
 *   const root = useRoot('my-root');
 *   <p>{root.value}</p>
 */
export function useRoot<T>(id: string) {
  // Return the reactive state object directly
  // Accessing .value on this object will track the dependency in the component's effect
  return consistency.getRoot<T>(id).state;
}
