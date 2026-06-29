import { describe, it } from "vitest";
import assert from "node:assert";
import { parseMarkdown } from "../src/index.js";

describe("Nested Fence Repair", () => {
  it("should handle nested fences by counting levels", () => {
    const input = [
      "```md",
      "Here is a nested block:",
      "```js",
      "console.log('nested');",
      "```",
      "Back in outer.",
      "```",
    ].join("\n");

    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 1);
    assert.strictEqual(blocks[0].kind, "code-block");
    assert.strictEqual(blocks[0].language, "md");

    const expectedValue = [
      "Here is a nested block:",
      "```js",
      "console.log('nested');",
      "```",
      "Back in outer.",
    ].join("\n");

    assert.strictEqual((blocks[0] as any).value, expectedValue);
  });

  it("should handle multiple levels of nesting", () => {
    const input = [
      "```outer",
      "L1",
      "```inner",
      "L2",
      "```deep",
      "L3",
      "```",
      "L2",
      "```",
      "L1",
      "```",
    ].join("\n");

    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 1);

    const expectedValue = [
      "L1",
      "```inner",
      "L2",
      "```deep",
      "L3",
      "```",
      "L2",
      "```",
      "L1",
    ].join("\n");

    assert.strictEqual((blocks[0] as any).value, expectedValue);
  });

  it("should use Colon Heuristic to detect nested blocks without info string", () => {
    const input = [
      "```md",
      "Here is code:",
      "```",
      "console.log('inner');",
      "```",
      "Done.",
      "```",
    ].join("\n");

    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 1);

    const expectedValue = [
      "Here is code:",
      "```",
      "console.log('inner');",
      "```",
      "Done.",
    ].join("\n");

    assert.strictEqual((blocks[0] as any).value, expectedValue);
  });
});
