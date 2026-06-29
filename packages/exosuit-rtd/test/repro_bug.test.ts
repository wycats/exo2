import { describe, it } from "vitest";
import assert from "node:assert";
import { parseMarkdown } from "../src/index.js";

describe("Reproduction of Nested Fence Bug", () => {
  it("should handle the user's specific example", () => {
    const input = ["```hello", "world", "", "```omg", "omg", "```", "```"].join(
      "\n",
    );

    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 1);
    assert.strictEqual(blocks[0].kind, "code-block");

    const expectedValue = ["world", "", "```omg", "omg", "```"].join("\n");

    // The parser might add a trailing newline to value, let's check trim
    assert.strictEqual((blocks[0] as any).value.trim(), expectedValue);
  });
});
