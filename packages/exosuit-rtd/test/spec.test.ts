import { expect } from "vitest";
import { parseMarkdown } from "../src/parser/index.js";
import type { RTDBlock } from "../src/dom/types.js";

describe("RTD Spec Conformance", () => {
  describe("Indented Code Blocks", () => {
    it("should parse 4-space indented code blocks", () => {
      const input = "    const x = 1;";
      const expected: RTDBlock[] = [
        {
          kind: "code-block",
          value: "const x = 1;",
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse tab-indented code blocks", () => {
      const input = "\tconst x = 1;";
      const expected: RTDBlock[] = [
        {
          kind: "code-block",
          value: "const x = 1;",
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should not parse 3-space indentation as code block", () => {
      const input = "   Not code";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [{ kind: "text", value: "   Not code" }],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });
  });

  describe("Underscore Emphasis", () => {
    it("should parse _italic_", () => {
      const input = "_italic_";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [
            {
              kind: "emphasis",
              children: [{ kind: "text", value: "italic" }],
            },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse __bold__", () => {
      const input = "__bold__";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [
            {
              kind: "strong",
              children: [{ kind: "text", value: "bold" }],
            },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should handle intraword underscores as text (foo_bar)", () => {
      const input = "foo_bar";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [{ kind: "text", value: "foo_bar" }],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });
  });

  describe("Security (No HTML)", () => {
    it("should parse HTML tags as plain text", () => {
      const input = "<div>content</div>";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [{ kind: "text", value: "<div>content</div>" }],
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
  });

  describe("LLM Normalization", () => {
    it("should normalize block math delimiters \\[ ... \\]", () => {
      const input = "\\[ x^2 \\]";
      const expected: RTDBlock[] = [
        {
          kind: "math-block",
          value: " x^2 ",
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should normalize inline math delimiters \\( ... \\)", () => {
      const input = "\\( x^2 \\)";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [
            {
              kind: "math-inline",
              value: " x^2 ",
            },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should normalize ghost citations", () => {
      const input = "Text 【13†source】";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [
            { kind: "text", value: "Text " },
            { kind: "citation", value: "13†source" },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });
  });
});
