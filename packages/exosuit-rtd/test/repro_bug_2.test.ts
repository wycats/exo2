import { describe, it } from "vitest";
import assert from "node:assert";
import { parseMarkdown } from "../src/index.js";

describe("Reproduction of Nested Fence Bug 2", () => {
  it("should handle the user's second example", () => {
    const input = ["```", "bye", "```js", "bye", "```", "```"].join("\n");

    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 1);
    assert.strictEqual(blocks[0].kind, "code-block");

    const expectedValue = ["bye", "```js", "bye", "```"].join("\n");

    assert.strictEqual((blocks[0] as any).value, expectedValue);
  });
});
