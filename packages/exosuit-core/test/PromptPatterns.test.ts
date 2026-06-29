import { expect } from "vitest";
import { validatePromptSpec, validateTemplateInterpolation } from "../src/models/PromptSpec.ts";

describe("PromptSpec validation", () => {
  it("accepts whitespace in interpolation keys", () => {
    const result = validateTemplateInterpolation("Hello { name }", "chat.greeting");
    expect(result.ok).to.equal(true);
    expect(result.errors).to.deep.equal([]);
  });

  it("rejects nested interpolation tokens", () => {
    const result = validateTemplateInterpolation(
      "Reading {path.to.file}",
      "tool.reading"
    );
    expect(result.ok).to.equal(false);
    expect(result.errors[0].message).to.include("Invalid interpolation token");
  });

  it("validates all string leaves of a parsed PromptSpec", () => {
    const spec = {
      global: {
        style: "Be concise.",
      },
      system: {
        chat: "Hello {name}!",
      },
      bad: {
        nested: "Nope {a.b}",
      },
    };

    const result = validatePromptSpec(spec);
    expect(result.ok).to.equal(false);
    expect(result.errors.some((e) => e.path === "bad.nested")).to.equal(true);
  });
});
