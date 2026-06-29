import { expect } from "vitest";
import { parseMarkdown } from "../src/parser/index.js";
import type { RTDBlock } from "../src/dom/types.js";

describe("RTD Security (No HTML)", () => {
  it("should parse HTML tags as plain text", () => {
    const input = "Hello <b>world</b>";
    const expected: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [{ kind: "text", value: "Hello <b>world</b>" }],
      },
    ];
    expect(parseMarkdown(input)).to.deep.equal(expected);
  });

  it("should parse script tags as plain text", () => {
    const input = "<script>alert(1)</script>";
    const expected: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [{ kind: "text", value: "<script>alert(1)</script>" }],
      },
    ];
    expect(parseMarkdown(input)).to.deep.equal(expected);
  });

  it("should parse HTML blocks as paragraphs", () => {
    const input = "<div>\n  Content\n</div>";
    const expected: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [{ kind: "text", value: "<div>\n  Content\n</div>" }],
      },
    ];
    expect(parseMarkdown(input)).to.deep.equal(expected);
  });
});
