import { expect } from "vitest";
import { RTDBuilder } from "../src/builder/index.js";

describe("RTDBuilder", () => {
  it("should build a simple document", () => {
    const doc = new RTDBuilder()
      .heading(1, "Hello World")
      .paragraph("This is a test.")
      .build();

    expect(doc).to.have.lengthOf(2);
    expect(doc[0]).to.deep.include({ kind: "heading", level: 1 });
    expect(doc[1]).to.deep.include({ kind: "paragraph" });
  });

  it("should build a container with children", () => {
    const doc = new RTDBuilder()
      .container("card", (c) => {
        c.paragraph("Inside container");
      })
      .build();

    expect(doc).to.have.lengthOf(1);
    expect(doc[0].kind).to.equal("container");
    if (doc[0].kind === "container") {
      expect(doc[0].variant).to.equal("card");
      expect(doc[0].children).to.have.lengthOf(1);
      expect(doc[0].children[0].kind).to.equal("paragraph");
    }
  });
});
