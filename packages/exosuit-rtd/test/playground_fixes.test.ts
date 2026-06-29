import { expect } from "vitest";
import { StreamingParser } from "../src/parser/streaming.js";

describe("Playground Fixes", () => {
  it("should parse inline links", () => {
    const input = "Click [here](https://example.com) now.";
    const parser = new StreamingParser();
    parser.parse(input);
    const result = parser.flush();

    expect(result[0].kind).to.equal("paragraph");
    const children = (result[0] as any).children;
    expect(children).to.have.length(3);
    expect(children[1].kind).to.equal("link");
    expect(children[1].href).to.equal("https://example.com");
    expect(children[1].children[0].value).to.equal("here");
  });

  it("should parse nested lists", () => {
    const input = "- Item 1\n  - Nested 1\n- Item 2";
    const parser = new StreamingParser();
    parser.parse(input);
    const result = parser.flush();

    expect(result).to.have.length(1);
    expect(result[0].kind).to.equal("list");
    const items = (result[0] as any).items;
    expect(items).to.have.length(2); // Item 1, Item 2

    // Check Item 1 children
    const item1Children = items[0].children;
    expect(item1Children).to.have.length(2); // Paragraph, Nested List
    expect(item1Children[0].kind).to.equal("paragraph");
    expect(item1Children[1].kind).to.equal("list");

    // Check Nested List
    const nestedList = item1Children[1];
    expect(nestedList.items).to.have.length(1);
    expect(nestedList.items[0].children[0].children[0].value).to.equal(
      "Nested 1",
    );
  });

  it("should recover from unclosed comments on header", () => {
    const input = "<!--\nUnclosed comment\n# Header";
    const parser = new StreamingParser();
    parser.parse(input);
    const result = parser.flush();

    expect(result).to.have.length(2);
    expect(result[0].kind).to.equal("comment");
    expect((result[0] as any).value).to.contain("Unclosed comment");

    expect(result[1].kind).to.equal("heading");
    expect((result[1] as any).level).to.equal(1);
    expect((result[1] as any).children[0].value).to.equal("Header");
  });
});
