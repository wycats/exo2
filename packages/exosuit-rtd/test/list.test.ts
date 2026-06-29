import { describe, it } from "vitest";
import assert from "node:assert";
import { parseMarkdown } from "../src/index.js";

describe("List Parsing", () => {
  it("should parse unordered lists", () => {
    const input = "- Item 1\n- Item 2";
    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 1);
    assert.strictEqual(blocks[0].kind, "list");
    assert.strictEqual((blocks[0] as any).ordered, false);
    assert.strictEqual((blocks[0] as any).items.length, 2);
    assert.strictEqual(
      (blocks[0] as any).items[0].children[0].children[0].value,
      "Item 1",
    );
  });

  it("should parse ordered lists", () => {
    const input = "1. First\n2. Second";
    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 1);
    assert.strictEqual(blocks[0].kind, "list");
    assert.strictEqual((blocks[0] as any).ordered, true);
    assert.strictEqual((blocks[0] as any).items.length, 2);
  });

  it("should handle mixed lists by creating new blocks", () => {
    const input = "- Unordered\n1. Ordered";
    const blocks = parseMarkdown(input);
    assert.strictEqual(blocks.length, 2);
    assert.strictEqual(blocks[0].kind, "list");
    assert.strictEqual((blocks[0] as any).ordered, false);
    assert.strictEqual(blocks[1].kind, "list");
    assert.strictEqual((blocks[1] as any).ordered, true);
  });
});
