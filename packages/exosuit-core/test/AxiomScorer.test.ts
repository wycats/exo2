import { expect } from "vitest";
import { AxiomScorer } from "../src/AxiomScorer.js";

describe("AxiomScorer", () => {
  const markdown = `
## 1. Context is King

**Principle**: The context is the source of truth.
**Why**: Agents need memory.

## 2. Phased Execution

**Principle**: Work in phases.
**Why**: Focus.
`;

  it("should parse axioms correctly", () => {
    const axioms = AxiomScorer.parse(markdown);
    expect(axioms).to.have.length(2);
    expect(axioms[0].title).to.equal("1. Context is King");
    expect(axioms[0].body).to.contain("source of truth");
    expect(axioms[1].title).to.equal("2. Phased Execution");
  });

  it("should score axioms based on query", () => {
    const axioms = AxiomScorer.parse(markdown);
    const query = "context truth";
    const scored = AxiomScorer.score(axioms, query);

    expect(scored).to.have.length(1);
    expect(scored[0].title).to.equal("1. Context is King");
    expect(scored[0].score).to.be.greaterThan(0);
  });

  it("should return empty list if no match", () => {
    const axioms = AxiomScorer.parse(markdown);
    const query = "banana";
    const scored = AxiomScorer.score(axioms, query);

    expect(scored).to.have.length(0);
  });

  it("should ignore short tokens", () => {
    const axioms = AxiomScorer.parse(markdown);
    const query = "is the"; // both < 3 chars (wait, 'the' is 3, filter is > 3)
    const scored = AxiomScorer.score(axioms, query);

    expect(scored).to.have.length(0);
  });
});
