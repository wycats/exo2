import { describe, expect, it } from "vitest";
import { StreamingParser } from "../src/parser/streaming.js";
import { HTMLRenderer } from "../src/renderers/html/index.js";

describe("Phase 37 Audit", () => {
  const renderer = new HTMLRenderer();

  it("should render links correctly (fix target vs href)", () => {
    const parser = new StreamingParser();
    const input = "[Exosuit](https://exosuit.io)";
    const blocks = parser.parse(input);
    const output = renderer.render(blocks);
    expect(output).to.include('<a href="https://exosuit.io">Exosuit</a>');
  });

  it("should render strikethrough (missing feature)", () => {
    const parser = new StreamingParser();
    const input = "~~strikethrough~~";
    const blocks = parser.parse(input);
    const output = renderer.render(blocks);
    expect(output).to.include("<s>strikethrough</s>");
  });

  it("should render tables (missing feature)", () => {
    const parser = new StreamingParser();
    const input = `
| Header 1 | Header 2 |
| -------- | -------- |
| Cell 1   | Cell 2   |
`;
    const blocks = parser.parse(input);
    const output = renderer.render(blocks);
    expect(output).to.include("<table>");
    expect(output).to.include("<th>Header 1</th>");
    expect(output).to.include("<td>Cell 1</td>");
  });

  it("should sanitize command links by default", () => {
    const parser = new StreamingParser();
    const input = "[Run](command:test.run)";
    const blocks = parser.parse(input);
    const output = renderer.render(blocks);
    // Should be rendered as plain text (children only) because command is not in default allowedSchemes
    expect(output).to.include("Run");
    expect(output).to.not.include('<a href="command:test.run">');
  });

  it("should allow command links when configured", () => {
    const parser = new StreamingParser();
    const customRenderer = new HTMLRenderer({
      allowedSchemes: ["http", "https", "mailto", "command"],
    });
    const input = "[Run](command:test.run)";
    const blocks = parser.parse(input);
    const output = customRenderer.render(blocks);
    expect(output).to.include('<a href="command:test.run">Run</a>');
  });
});
