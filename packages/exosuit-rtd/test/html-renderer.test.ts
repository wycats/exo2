import { expect } from "vitest";
import { HTMLRenderer } from "../src/renderers/html/index.js";
import { RTDBuilder } from "../src/builder/index.js";
import { RSLStyle } from "../src/style/om.js";
import { defaultRegistry } from "../src/style/registry.js";

describe("HTMLRenderer", () => {
  it("should render basic blocks", () => {
    const doc = new RTDBuilder()
      .heading(1, "Title")
      .paragraph("Hello world")
      .build();

    const renderer = new HTMLRenderer();
    const html = renderer.render(doc);

    expect(html).to.equal("<h1>Title</h1><p>Hello world</p>");
  });

  it("should render nested lists", () => {
    const doc = new RTDBuilder()
      .list(false, (l) => {
        l.item((b) => b.paragraph("Item 1"));
        l.item((b) => b.paragraph("Item 2"));
      })
      .build();

    const renderer = new HTMLRenderer();
    const html = renderer.render(doc);

    expect(html).to.contain("<ul>");
    expect(html).to.contain("<li><p>Item 1</p></li>");
  });

  it("should render containers with styles", () => {
    const doc = new RTDBuilder()
      .container("card", (c) => c.paragraph("Content"))
      .build();

    const renderer = new HTMLRenderer({
      resolveStyle: (variant) => {
        if (variant === "card") {
          const style = new RSLStyle(defaultRegistry);
          style.surface.set({ base: "surface-1" });
          return style;
        }
        return undefined;
      },
    });

    const html = renderer.render(doc);

    expect(html).to.contain('class="rtd-container rtd-variant-card"');
    expect(html).to.contain('style="');
    expect(html).to.contain("background-color: var(--rtd-color-surface-1)");
  });

  it("should escape HTML in text", () => {
    const doc = new RTDBuilder()
      .paragraph("<script>alert('xss')</script>")
      .build();

    const renderer = new HTMLRenderer();
    const html = renderer.render(doc);

    expect(html).to.contain("&lt;script&gt;");
    expect(html).to.not.contain("<script>");
  });
});
