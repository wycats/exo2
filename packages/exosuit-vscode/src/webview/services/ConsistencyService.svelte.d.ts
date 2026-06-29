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
export declare function setVsCodeApi(api: any): void;
declare class ConsistencyService {
    private roots;
    /**
     * Get a handle to a root's signal.
     * This is safe to call from anywhere. It doesn't trigger a read.
     * If the root doesn't exist, it creates a new "pending" signal.
     */
    getRoot<T>(id: string): {
        state: RootState;
    };
    /**
     * Called when we receive an update from the Extension Host.
     * This is the "Writer" side of the signal.
     */
    updateRoot(id: string, value: any): void;
    setError(id: string, error: string): void;
    invalidateRoot(id: string): void;
}
export declare const consistency: ConsistencyService;
/**
 * The Composable for Components.
 * Usage:
 *   const root = useRoot('my-root');
 *   <p>{root.value}</p>
 */
export declare function useRoot<T>(id: string): RootState<any>;
export {};
