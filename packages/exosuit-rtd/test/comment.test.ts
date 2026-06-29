import { expect } from "vitest";
import { StreamingParser } from "../src/parser/streaming.js";
import { ParagraphBlock } from "../src/dom/types.js";

describe("StreamingParser - Comments", () => {
  it("parses a simple block comment", () => {
    const parser = new StreamingParser();
    parser.parse("<!-- hello world -->");
    const blocks = parser.flush();
    expect(blocks).to.deep.equal([
      {
        kind: "comment",
        value: "<!-- hello world -->",
        nestingLevel: 0,
      },
    ]);
  });

  it("parses a multi-line block comment", () => {
    const parser = new StreamingParser();
    parser.parse("<!--\n");
    parser.parse("hello\n");
    parser.parse("world\n-->");
    const blocks = parser.flush();

    expect(blocks).to.deep.equal([
      {
        kind: "comment",
        value: "<!--\nhello\nworld\n-->",
        nestingLevel: 0,
      },
    ]);
  });

  it("handles nested comments", () => {
    const parser = new StreamingParser();
    parser.parse("<!-- outer <!-- inner --> outer -->");
    const blocks = parser.flush();
    expect(blocks).to.deep.equal([
      {
        kind: "comment",
        value: "<!-- outer <!-- inner --> outer -->",
        nestingLevel: 0,
      },
    ]);
  });

  it("aborts to text if sanity limit exceeded", () => {
    const parser = new StreamingParser();
    parser.parse("<!-- start\n");
    for (let i = 0; i < 25; i++) {
      parser.parse(`line ${i}\n`);
    }
    const blocks = parser.flush();

    // Should NOT be a comment block
    expect(blocks[0].kind).to.equal("paragraph");
    // Should contain the raw text
    const p = blocks[0] as ParagraphBlock;
    expect((p.children[0] as any).value).to.contain("<!-- start");
  });

  it("aborts to text if unclosed at EOF", () => {
    const parser = new StreamingParser();
    parser.parse("<!-- unclosed");
    const blocks = parser.flush();

    expect(blocks[0].kind).to.equal("paragraph");
    const p = blocks[0] as ParagraphBlock;
    expect((p.children[0] as any).value).to.equal("<!-- unclosed");
  });
});
