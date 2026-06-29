import { expect } from "vitest";
import { StreamingParser } from "../src/parser/streaming.js";

describe("Phase 36 Fixes", () => {
  it("should parse double-dollar math as inline math with content", () => {
    const input = "Block: $$ x^2 + y^2 = z^2 $$";
    const parser = new StreamingParser();
    parser.parse(input);
    const result = parser.flush();

    expect(result[0].kind).to.equal("paragraph");
    const children = (result[0] as any).children;
    // Expected: Text("Block: "), MathInline(" x^2 + y^2 = z^2 ")
    expect(children).to.have.length(2);
    expect(children[1].kind).to.equal("math-inline");
    expect(children[1].value).to.equal(" x^2 + y^2 = z^2 ");
  });

  it("should allow lists to interrupt paragraphs", () => {
    const input = "Paragraph text\n- List Item";
    const parser = new StreamingParser();
    parser.parse(input);
    const result = parser.flush();

    expect(result).to.have.length(2);
    expect(result[0].kind).to.equal("paragraph");
    expect(result[1].kind).to.equal("list");
  });
});
