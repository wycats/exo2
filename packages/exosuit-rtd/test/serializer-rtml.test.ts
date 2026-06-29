import { expect } from "vitest";
import { RTMLSerializer } from "../src/serializer/rtml.js";
import type { RTDBlock } from "../src/dom/types.js";

describe("RTMLSerializer", () => {
  const serializer = new RTMLSerializer();

  it("serializes paragraphs with text", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [{ kind: "text", value: "Hello World" }],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal("<p>Hello World</p>");
  });

  it("serializes headings", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "heading",
        level: 1,
        children: [{ kind: "text", value: "Title" }],
      },
      {
        kind: "heading",
        level: 3,
        children: [{ kind: "text", value: "Subtitle" }],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      "<h1>Title</h1><h3>Subtitle</h3>"
    );
  });

  it("serializes code blocks", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "code-block",
        language: "ts",
        value: "const x = 1;",
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      '<pre data-lang="ts">const x = 1;</pre>'
    );
  });

  it("serializes blockquotes", () => {
    const blocks: RTDBlock[] = [
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
    expect(serializer.serialize(blocks)).to.equal(
      "<blockquote><p>Quote</p></blockquote>"
    );
  });

  it("serializes lists", () => {
    const blocks: RTDBlock[] = [
      {
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
            checked: true,
            children: [
              {
                kind: "paragraph",
                children: [{ kind: "text", value: "Item 2" }],
              },
            ],
          },
        ],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      '<ul><li><p>Item 1</p></li><li data-checked="true"><p>Item 2</p></li></ul>'
    );
  });

  it("serializes inlines", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [
          { kind: "strong", children: [{ kind: "text", value: "Bold" }] },
          { kind: "text", value: " " },
          { kind: "emphasis", children: [{ kind: "text", value: "Italic" }] },
          { kind: "text", value: " " },
          { kind: "code-span", value: "code" },
        ],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      "<p><strong>Bold</strong> <em>Italic</em> <code>code</code></p>"
    );
  });

  it("serializes links", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [
          {
            kind: "link",
            href: "https://example.com",
            title: "Example",
            children: [{ kind: "text", value: "Link" }],
          },
        ],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      '<p><a href="https://example.com" title="Example">Link</a></p>'
    );
  });

  it("serializes icons", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [{ kind: "icon", name: "gear" }],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      '<p><rtd-icon name="gear"></rtd-icon></p>'
    );
  });

  it("serializes commands", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [
          {
            kind: "command",
            id: "test.command",
            args: ["arg1"],
            children: [{ kind: "text", value: "Run" }],
          },
        ],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      '<p><rtd-command id="test.command" args="[&quot;arg1&quot;]">Run</rtd-command></p>'
    );
  });

  it("escapes HTML in text", () => {
    const blocks: RTDBlock[] = [
      {
        kind: "paragraph",
        children: [{ kind: "text", value: "<script>alert(1)</script>" }],
      },
    ];
    expect(serializer.serialize(blocks)).to.equal(
      "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>"
    );
  });
});
