import { expect } from "vitest";
import { RSLStyle } from "../src/style/om.js";
import { defaultRegistry } from "../src/style/registry.js";

describe("RSL Object Model (Slots)", () => {
  it("should handle explicit spacing slots", () => {
    const style = new RSLStyle(defaultRegistry);

    // spacing: x sm, y lg
    style.spacing.set({ x: "sm", y: "lg" });

    const css = style.compile();

    expect(css).to.deep.include({
      paddingInline: "var(--rtd-space-sm)",
      paddingBlock: "var(--rtd-space-lg)",
    });
  });

  it("should handle layout slots (mode + gap)", () => {
    const style = new RSLStyle(defaultRegistry);

    // layout: mode stack, gap md
    style.layout.set({ mode: "stack", gap: "md" });

    const css = style.compile();

    expect(css).to.deep.include({
      display: "flex",
      flexDirection: "column",
      gap: "var(--rtd-space-md)",
    });
  });

  it("should overwrite slots correctly (Last Write Wins per slot)", () => {
    const style = new RSLStyle(defaultRegistry);

    style.spacing.set({ x: "sm" });
    style.spacing.set({ x: "xl" }); // Should overwrite sm

    const css = style.compile();

    expect(css).to.deep.include({
      paddingInline: "var(--rtd-space-xl)",
    });
  });
});
