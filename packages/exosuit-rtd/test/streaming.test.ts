import { expect } from "vitest";
import { StreamingParser } from "../src/parser/streaming.js";
import type { RTDBlock } from "../src/dom/types.js";

describe("RTD Streaming Parser (Tail Buffering)", () => {
  it("should parse a document split into chunks", () => {
    const chunks = ["# Head", "ing\n\nPara", "graph with **bo", "ld** text."];

    const parser = new StreamingParser();
    for (const chunk of chunks) {
      parser.parse(chunk);
    }
    const result = parser.flush();

    const expected: RTDBlock[] = [
      {
        kind: "heading",
        level: 1,
        children: [{ kind: "text", value: "Heading" }],
      },
      {
        kind: "paragraph",
        children: [
          { kind: "text", value: "Paragraph with " },
          { kind: "strong", children: [{ kind: "text", value: "bold" }] },
          { kind: "text", value: " text." },
        ],
      },
    ];

    expect(result).to.deep.equal(expected);
  });

  it("should handle split newlines", () => {
    const chunks = ["Line 1\n", "Line 2"];
    const parser = new StreamingParser();
    parser.parse(chunks[0]);
    parser.parse(chunks[1]);
    const result = parser.flush();

    const expected: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [{ kind: "text", value: "Line 1\nLine 2" }],
      },
    ];

    expect(result).to.deep.equal(expected);
  });

  it("should handle split code blocks", () => {
    const chunks = ["```t", "s\nconst x", " = 1;\n```"];
    const parser = new StreamingParser();
    for (const chunk of chunks) {
      parser.parse(chunk);
    }
    const result = parser.flush();

    const expected: RTDBlock[] = [
      {
        kind: "code-block",
        language: "ts",
        value: "const x = 1;",
      },
    ];

    expect(result).to.deep.equal(expected);
  });
});
