import { expect } from "vitest";
import { RSLStyle } from "../src/style/om.js";
import { defaultRegistry } from "../src/style/registry.js";

describe("RSL Object Model", () => {
  it("should compile a simple style", () => {
    const style = new RSLStyle(defaultRegistry);

    style.layout.set({ mode: "stack", gap: "md" });
    style.surface.set({ base: "surface-1" });

    const css = style.compile();

    expect(css).to.deep.include({
      display: "flex",
      flexDirection: "column",
      gap: "var(--rtd-space-md)",
      backgroundColor: "var(--rtd-color-surface-1)",
    });
  });

  it("should enforce Last Write Wins for slots", () => {
    const style = new RSLStyle(defaultRegistry);

    // First set stack
    style.layout.set({ mode: "stack" });
    // Then set grid (should overwrite stack)
    style.layout.set({ mode: "grid" });

    const css = style.compile();

    expect(css).to.deep.include({
      display: "grid",
    });
    expect(css).to.not.have.property("flexDirection");
  });

  it("should handle multiple properties", () => {
    const style = new RSLStyle(defaultRegistry);

    style.text.set({ size: "xl", weight: "bold" });
    style.border.set({ style: "subtle", radius: "md" });

    const css = style.compile();

    expect(css).to.deep.include({
      fontSize: "var(--rtd-font-size-xl)",
      fontWeight: 700,
      border: "1px solid var(--rtd-color-border-subtle)",
      borderRadius: "var(--rtd-radius-md)",
    });
  });

  it("should support fine-grained spacing control", () => {
    const style = new RSLStyle(defaultRegistry);
    style.spacing.set({ x: "sm", y: "lg" });

    const css = style.compile();

    expect(css).to.deep.include({
      paddingInline: "var(--rtd-space-sm)",
      paddingBlock: "var(--rtd-space-lg)",
    });
  });
});
