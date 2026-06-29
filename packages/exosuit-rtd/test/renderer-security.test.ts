import { expect } from "vitest";
import { HTMLRenderer } from "../src/renderers/html/index.js";
import type { RTDBlock } from "../src/dom/types.js";

describe("HTMLRenderer - Link Security", () => {
  const renderer = new HTMLRenderer();

  it("renders links with allowed schemes (https)", () => {
    const block: RTDBlock = {
      kind: "paragraph",
      children: [
        {
          kind: "link",
          href: "https://example.com",
          children: [{ kind: "text", value: "Example" }],
        },
      ],
    };
    expect(renderer.render([block])).to.equal(
      '<p><a href="https://example.com">Example</a></p>'
    );
  });

  it("renders links with allowed schemes (mailto)", () => {
    const block: RTDBlock = {
      kind: "paragraph",
      children: [
        {
          kind: "link",
          href: "mailto:user@example.com",
          children: [{ kind: "text", value: "Email" }],
        },
      ],
    };
    expect(renderer.render([block])).to.equal(
      '<p><a href="mailto:user@example.com">Email</a></p>'
    );
  });

  it("renders relative paths", () => {
    const block: RTDBlock = {
      kind: "paragraph",
      children: [
        {
          kind: "link",
          href: "./doc.md",
          children: [{ kind: "text", value: "Doc" }],
        },
      ],
    };
    expect(renderer.render([block])).to.equal(
      '<p><a href="./doc.md">Doc</a></p>'
    );
  });

  it("sanitizes links with disallowed schemes (javascript)", () => {
    const block: RTDBlock = {
      kind: "paragraph",
      children: [
        {
          kind: "link",
          href: "javascript:alert(1)",
          children: [{ kind: "text", value: "Click me" }],
        },
      ],
    };
    // Should render as plain text or disabled link.
    // For now, let's assume we strip the <a> tag but keep content.
    expect(renderer.render([block])).to.equal("<p>Click me</p>");
  });

  it("allows custom schemes if configured", () => {
    const customRenderer = new HTMLRenderer({
      allowedSchemes: ["vscode"],
    });
    const block: RTDBlock = {
      kind: "paragraph",
      children: [
        {
          kind: "link",
          href: "vscode:open?url=...",
          children: [{ kind: "text", value: "Open" }],
        },
      ],
    };
    expect(customRenderer.render([block])).to.equal(
      '<p><a href="vscode:open?url=...">Open</a></p>'
    );
  });
});
