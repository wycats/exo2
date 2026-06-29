import { expect } from "vitest";
import { interpolateStrict } from "../src/interpolation.ts";

describe("interpolateStrict", () => {
  it("replaces declared keys", () => {
    const out = interpolateStrict("Hello {name}", { name: "World" });
    expect(out).to.equal("Hello World");
  });

  it("ignores whitespace inside braces", () => {
    const out = interpolateStrict("Hello { name }", { name: "World" });
    expect(out).to.equal("Hello World");
  });

  it("preserves unknown tokens", () => {
    const out = interpolateStrict("Hello {missing}", { name: "World" });
    expect(out).to.equal("Hello {missing}");
  });

  it("preserves invalid (nested) tokens", () => {
    const out = interpolateStrict("Hello {a.b}", { "a.b": "nope" });
    expect(out).to.equal("Hello {a.b}");
  });
});
