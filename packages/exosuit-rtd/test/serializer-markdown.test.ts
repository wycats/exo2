import { expect } from "vitest";
import { serializeBlock } from "../src/serializer/markdown.js";
import type { RTDBlock } from "../src/dom/types.js";

describe("MarkdownSerializer", () => {
  it("serializes paragraphs with text", () => {
    const block: RTDBlock = {
      kind: "paragraph",
      children: [{ kind: "text", value: "Hello World" }],
    };
    expect(serializeBlock(block)).to.equal("Hello World");
  });

  it("serializes headings", () => {
    const block: RTDBlock = {
      kind: "heading",
      level: 1,
      children: [{ kind: "text", value: "Title" }],
    };
    expect(serializeBlock(block)).to.equal("# Title");
  });

  it("serializes code blocks", () => {
    const block: RTDBlock = {
      kind: "code-block",
      language: "ts",
      value: "const x = 1;",
    };
    expect(serializeBlock(block)).to.equal("```ts\nconst x = 1;\n```");
  });

  it("serializes blockquotes", () => {
    const block: RTDBlock = {
      kind: "blockquote",
      children: [
        {
          kind: "paragraph",
          children: [{ kind: "text", value: "Quote" }],
        },
      ],
    };
    expect(serializeBlock(block)).to.equal("> Quote");
  });

  it("serializes lists", () => {
    const block: RTDBlock = {
      kind: "list",
      ordered: false,
      items: [
        {
          children: [
            {
              kind: "paragraph",
              children: [{ kind: "text", value: "Item 1" }],
            },
          ],
        },
        {
          children: [
            {
              kind: "paragraph",
              children: [{ kind: "text", value: "Item 2" }],
            },
          ],
        },
      ],
    };
    expect(serializeBlock(block)).to.equal("- Item 1\n- Item 2");
  });

  it("serializes task lists", () => {
    const block: RTDBlock = {
      kind: "list",
      ordered: false,
      items: [
        {
          checked: false,
          children: [
            {
              kind: "paragraph",
              children: [{ kind: "text", value: "Task 1" }],
            },
          ],
        },
        {
          checked: true,
          children: [
            {
              kind: "paragraph",
              children: [{ kind: "text", value: "Task 2" }],
            },
          ],
        },
      ],
    };
    expect(serializeBlock(block)).to.equal("- [ ] Task 1\n- [x] Task 2");
  });

  it("serializes inlines", () => {
    const block: RTDBlock = {
      kind: "paragraph",
      children: [
        { kind: "strong", children: [{ kind: "text", value: "Bold" }] },
        { kind: "text", value: " " },
        { kind: "emphasis", children: [{ kind: "text", value: "Italic" }] },
        { kind: "text", value: " " },
        { kind: "code-span", value: "code" },
      ],
    };
    expect(serializeBlock(block)).to.equal("**Bold** _Italic_ `code`");
  });

  it("serializes links", () => {
    const block: RTDBlock = {
      kind: "paragraph",
      children: [
        {
          kind: "link",
          href: "https://example.com",
          title: "Example",
          children: [{ kind: "text", value: "Link" }],
        },
      ],
    };
    expect(serializeBlock(block)).to.equal("[Link](https://example.com)");
  });
});
