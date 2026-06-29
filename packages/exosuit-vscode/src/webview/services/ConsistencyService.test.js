import { describe, it, expect, vi } from "vitest";
import { consistency, useRoot } from "./ConsistencyService.svelte";
import { flushSync } from "svelte";
// We need to mock $effect to verify reactivity, but in a unit test environment
// without a component, we can use $effect.root or just observe the values.
// However, to test "re-renders" (or effect re-runs), we need to use $effect.
describe("ConsistencyService", () => {
    it("should return a pending signal for a new root", () => {
        const root = useRoot("test-root-1");
        expect(root.value).toBe(null);
        expect(root.status).toBe("pending");
    });
    it("should update the signal when updateRoot is called", () => {
        const id = "test-root-2";
        const root = useRoot(id);
        consistency.updateRoot(id, "new-value");
        expect(root.value).toBe("new-value");
        expect(root.status).toBe("ready");
    });
    it("should maintain isolation between roots", async () => {
        const rootA = useRoot("root-A");
        const rootB = useRoot("root-B");
        let countA = 0;
        let countB = 0;
        // We use a simple tracking function to simulate an effect
        // In a real Svelte environment, we'd use $effect.
        // Since we are in a .ts test file, we can't use $effect directly unless we compile this test with Svelte.
        // But we can verify that the objects are distinct.
        expect(rootA).not.toBe(rootB);
        // Update B
        consistency.updateRoot("root-B", "value-B");
        expect(rootB.value).toBe("value-B");
        expect(rootA.value).toBe(null); // A should be untouched
        // Update A
        consistency.updateRoot("root-A", "value-A");
        expect(rootA.value).toBe("value-A");
        expect(rootB.value).toBe("value-B"); // B should be untouched
    });
});
