import { expect } from "vitest";
import { parseMarkdown } from "../src/parser/index.js";
import type { RTDBlock } from "../src/dom/types.js";

describe("RTD Streaming Parser (Spec Subset)", () => {
  describe("Blocks", () => {
    it("should parse a simple paragraph", () => {
      const input = "Hello world";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [{ kind: "text", value: "Hello world" }],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse multiple paragraphs", () => {
      const input = "Para 1\n\nPara 2";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [{ kind: "text", value: "Para 1" }],
        },
        {
          kind: "paragraph",
          children: [{ kind: "text", value: "Para 2" }],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse headings (ATX)", () => {
      const input = "# Heading 1\n## Heading 2";
      const expected: RTDBlock[] = [
        {
          kind: "heading",
          level: 1,
          children: [{ kind: "text", value: "Heading 1" }],
        },
        {
          kind: "heading",
          level: 2,
          children: [{ kind: "text", value: "Heading 2" }],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse fenced code blocks", () => {
      const input = "```ts\nconst x = 1;\n```";
      const expected: RTDBlock[] = [
        {
          kind: "code-block",
          language: "ts",
          value: "const x = 1;",
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse blockquotes", () => {
      const input = "> Quote";
      const expected: RTDBlock[] = [
        {
          kind: "blockquote",
          children: [
            {
              kind: "paragraph",
              children: [{ kind: "text", value: "Quote" }],
            },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse alerts (GFM)", () => {
      const input = "> [!NOTE]\n> This is a note.";
      const expected: RTDBlock[] = [
        {
          kind: "alert",
          variant: "note",
          children: [
            {
              kind: "paragraph",
              children: [{ kind: "text", value: "This is a note." }],
            },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });
  });

  describe("Inlines", () => {
    it("should parse bold text", () => {
      const input = "**Bold**";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [
            {
              kind: "strong",
              children: [{ kind: "text", value: "Bold" }],
            },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse italic text", () => {
      const input = "*Italic*";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [
            {
              kind: "emphasis",
              children: [{ kind: "text", value: "Italic" }],
            },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });

    it("should parse VS Code icons", () => {
      const input = "Click $(gear) settings";
      const expected: RTDBlock[] = [
        {
          kind: "paragraph",
          children: [
            { kind: "text", value: "Click " },
            { kind: "icon", name: "gear" },
            { kind: "text", value: " settings" },
          ],
        },
      ];
      expect(parseMarkdown(input)).to.deep.equal(expected);
    });
  });
});
