import { expect } from "vitest";
import { RSLParser } from "../src/style/parser.js";
import { defaultRegistry } from "../src/style/registry.js";

describe("RSL Parser", () => {
  it("should parse explicit slot syntax", () => {
    const parser = new RSLParser(defaultRegistry);
    const rsl = `
      layout: mode stack, gap md;
      spacing: x sm, y lg;
    `;

    const style = parser.parse(rsl);
    const css = style.compile();

    expect(css).to.deep.include({
      display: "flex",
      flexDirection: "column",
      gap: "var(--rtd-space-md)",
      paddingInline: "var(--rtd-space-sm)",
      paddingBlock: "var(--rtd-space-lg)",
    });
  });

  it("should handle multiple properties and comments", () => {
    const parser = new RSLParser(defaultRegistry);
    const rsl = `
      /* This is a comment */
      surface: base surface-1;
      border: style subtle, radius md;
    `;

    const style = parser.parse(rsl);
    const css = style.compile();

    expect(css).to.deep.include({
      backgroundColor: "var(--rtd-color-surface-1)",
      border: "1px solid var(--rtd-color-border-subtle)",
      borderRadius: "var(--rtd-radius-md)",
    });
  });

  it("should warn on invalid syntax but continue", () => {
    const parser = new RSLParser(defaultRegistry);
    // 'invalid' is not 'slot token'
    const rsl = `spacing: x sm, invalid;`;

    const style = parser.parse(rsl);
    const css = style.compile();

    // Should still have the valid part
    expect(css).to.deep.include({
      paddingInline: "var(--rtd-space-sm)",
    });
  });
});
