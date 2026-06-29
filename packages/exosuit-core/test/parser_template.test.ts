import { expect } from "vitest";
import { ExosuitParser } from "../src/parser.js";

describe("ExosuitParser Template Handling", () => {
  const parser = new ExosuitParser();

  it("should ignore content within agent-template blocks", async () => {
    const markdown = `
<!-- agent-template start -->
## Epoch Template
### Phase Template
<!-- agent-template end -->

## Epoch 1: Real Epoch
### Phase 1: Real Phase
`;

    const plan = await parser.parsePlan(markdown);

    // Should only have 1 epoch
    expect(plan.items.length).to.equal(1);
    expect(plan.items[0].title).to.equal("Epoch 1: Real Epoch");
    expect(plan.items[0].children.length).to.equal(1);
    expect(plan.items[0].children[0].title).to.equal("Phase 1: Real Phase");
  });
});
