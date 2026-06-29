import { describe, it } from "./harness.js";
import * as assert from "assert";
import type { Cell } from "../../agent/StreamTokenizer";
import { StreamTokenizer } from "../../agent/StreamTokenizer";

describe("StreamTokenizer Test Suite", () => {
  it("Passes through plain text", () => {
    const tokenizer = new StreamTokenizer();
    let output = "";
    tokenizer.on("text", (text) => {
      output += text;
    });

    tokenizer.processChunk("Hello world");
    assert.strictEqual(output, "Hello world");
  });

  it("Detects and emits simple cells", () => {
    const tokenizer = new StreamTokenizer();
    const cells: Cell[] = [];
    let textOutput = "";

    tokenizer.on("text", (text) => (textOutput += text));
    tokenizer.on("cell", (cell) => cells.push(cell));

    tokenizer.processChunk("Start <cmd>ls -la</cmd> End");

    assert.strictEqual(textOutput, "Start  End");
    assert.strictEqual(cells.length, 1);
    assert.strictEqual(cells[0].type, "cmd");
    assert.strictEqual(cells[0].content, "ls -la");
  });

  it("Parses attributes", () => {
    const tokenizer = new StreamTokenizer();
    const cells: Cell[] = [];

    tokenizer.on("cell", (cell) => cells.push(cell));

    tokenizer.processChunk('<diff path="src/main.ts">Content</diff>');

    assert.strictEqual(cells.length, 1);
    assert.strictEqual(cells[0].type, "diff");
    assert.strictEqual(cells[0].attributes["path"], "src/main.ts");
    assert.strictEqual(cells[0].content, "Content");
  });

  it("Handles zombie buffers (invalid tags)", () => {
    const tokenizer = new StreamTokenizer();
    let output = "";
    tokenizer.on("text", (text) => (output += text));

    // <invalid is not a known tag, so it should eventually flush as text
    // The current implementation flushes after 20 chars
    tokenizer.processChunk("<invalid tag that is very long and should flush>");

    assert.ok(output.includes("<invalid"));
  });

  it("Handles split chunks", () => {
    const tokenizer = new StreamTokenizer();
    const cells: Cell[] = [];

    tokenizer.on("cell", (cell) => cells.push(cell));

    tokenizer.processChunk("Before <cm");
    tokenizer.processChunk('d>echo "split"</c');
    tokenizer.processChunk("md> After");

    assert.strictEqual(cells.length, 1);
    assert.strictEqual(cells[0].content, 'echo "split"');
  });
});
