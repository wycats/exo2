import { expect } from "vitest";
import { ExosuitParser } from "../src/parser.js";
import { ExosuitSerializer } from "../src/serializer.js";

describe("Exosuit Core Integration", () => {
  const parser = new ExosuitParser();
  const serializer = new ExosuitSerializer();

  it("parses a task list with IDs", async () => {
    const input = `
# Tasks

- [ ] Task 1 <!-- id: "t1" -->
- [x] Task 2 <!-- id: "t2" --> <!-- relates-to: "p1" -->
    `.trim();

    const result = await parser.parseTasks(input);

    expect(result.tasks).to.have.lengthOf(2);

    expect(result.tasks[0]).to.deep.include({
      id: "t1",
      title: "Task 1",
      status: "todo",
    });

    expect(result.tasks[1]).to.deep.include({
      id: "t2",
      title: "Task 2",
      status: "done",
      relatesTo: "p1",
    });
  });

  it("round-trips a task list (simple)", async () => {
    const input = `
# Tasks

- [ ] Task 1 <!-- id: "t1" -->
- [x] Task 2 <!-- id: "t2" --> <!-- relates-to: "p1" -->
     `.trim();

    // Note: Our simple serializer currently regenerates the header and spacing slightly differently
    // so we won't expect exact string equality with the input yet, but we can check the logic.

    const parsed = await parser.parseTasks(input);
    const output = serializer.serializeTasks(parsed);

    expect(output).to.contain('- [ ] Task 1 <!-- id: "t1" -->');
    expect(output).to.contain(
      '- [x] Task 2 <!-- id: "t2" --> <!-- relates-to: "p1" -->'
    );
  });

  it("parses a recursive plan tree", async () => {
    const input = `
# Plan

- [ ] Phase 1 <!-- id: "p1" -->
  - [x] Task A <!-- id: "t1" -->
  - [ ] Task B <!-- id: "t2" -->
- [ ] Phase 2 <!-- id: "p2" -->
      `.trim();

    const result = await parser.parsePlan(input);

    expect(result.items).to.have.lengthOf(2);
    expect(result.items[0].id).to.equal("p1");
    expect(result.items[0].children).to.have.lengthOf(2);
    expect(result.items[0].children[0].id).to.equal("t1");
    expect(result.items[0].children[0].status).to.equal("done");
    expect(result.items[1].id).to.equal("p2");
  });

  it("parses a header-based plan outline", async () => {
    const input = `
# Project Plan Outline

## Epoch 1: Foundation (Completed)

### Phase 1: Setup (Completed)

- [x] Task 1

### Phase 2: Tooling (Active)

- [ ] Task 2

## Epoch 2: Advanced (Proposed)

### Phase 3: Future (Proposed)
    `.trim();

    const result = await parser.parsePlan(input);

    expect(result.items).to.have.lengthOf(2); // 2 Epochs
    expect(result.items[0].title).to.equal("Epoch 1: Foundation (Completed)");
    expect(result.items[0].children).to.have.lengthOf(2); // 2 Phases
    expect(result.items[0].children[0].title).to.equal(
      "Phase 1: Setup (Completed)"
    );
    expect(result.items[1].title).to.equal("Epoch 2: Advanced (Proposed)");
    expect(result.items[1].children).to.have.lengthOf(1); // 1 Phase
  });
});
