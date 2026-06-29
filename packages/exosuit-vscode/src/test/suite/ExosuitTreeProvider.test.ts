import { describe, it, beforeEach } from "./harness.js";
import * as assert from "assert";
import { ExosuitTreeProvider } from "../../ExosuitTreeProvider";
import { PlanService } from "../../PlanService";
import type { PlanItem } from "@exosuit/core";

describe("ExosuitTreeProvider Test Suite", () => {
  let provider: ExosuitTreeProvider;
  let planService: PlanService;

  const mockPlan: PlanItem[] = [
    {
      id: "epoch-1",
      title: "Epoch 1",
      status: "done",
      type: "epoch",
      children: [
        {
          id: "phase-1",
          title: "Phase 1",
          status: "done",
          type: "phase",
          children: [
            {
              id: "task-1",
              title: "Task 1",
              status: "done",
              type: "task",
              children: [],
            },
          ],
        },
      ],
    },
    {
      id: "epoch-2",
      title: "Epoch 2",
      status: "in-progress",
      type: "epoch",
      children: [
        {
          id: "phase-2",
          title: "Phase 2",
          status: "in-progress",
          type: "phase",
          children: [
            {
              id: "task-2",
              title: "Task 2",
              status: "in-progress",
              type: "task",
              children: [],
            },
          ],
        },
      ],
    },
  ];

  beforeEach(() => {
    planService = PlanService.instance;
    // Mock getPlan
    planService.getPlan = async () => mockPlan;
    provider = new ExosuitTreeProvider("project-plan");
  });

  it("getActivePhaseId should return the in-progress phase", async () => {
    const activeId = await provider.getActivePhaseId();
    assert.strictEqual(activeId, "phase-2");
  });

  it("getItem should return the correct item with correct context", async () => {
    const item = await provider.getItem("phase-2");
    assert.ok(item);
    assert.strictEqual(item?.id, "epoch-2/phase-2");
    assert.strictEqual(item?.label, "Phase 2");
    // Check context value or icon path if possible, but mainly we want to ensure it exists
  });

  it("getParent should return the correct parent for a phase", async () => {
    const phaseItem = await provider.getItem("phase-2");
    assert.ok(phaseItem);

    const parent = await provider.getParent(phaseItem!);
    assert.ok(parent);
    assert.strictEqual(parent?.id, "epoch-2");
    assert.strictEqual(parent?.label, "Epoch 2");
  });

  it("getParent should return the correct parent for a task", async () => {
    const taskItem = await provider.getItem("task-2");
    assert.ok(taskItem);

    const parent = await provider.getParent(taskItem!);
    assert.ok(parent);
    assert.strictEqual(parent?.id, "epoch-2/phase-2");
  });

  it("getParent should return undefined for an epoch", async () => {
    const epochItem = await provider.getItem("epoch-2");
    assert.ok(epochItem);

    const parent = await provider.getParent(epochItem!);
    assert.strictEqual(parent, undefined);
  });

  it("retries empty root plans so cold-start sidebar data can appear", async () => {
    let calls = 0;
    let refreshes = 0;
    planService.getPlan = async () => {
      calls++;
      return calls <= refreshes + 1 ? [] : mockPlan;
    };

    provider = new ExosuitTreeProvider("project-plan", { retryBackoffMs: [1] });
    provider.onDidChangeTreeData(() => {
      refreshes++;
    });

    const first = await provider.getChildren();
    assert.strictEqual(first.length, 0);

    await new Promise((resolve) => setTimeout(resolve, 10));

    assert.strictEqual(refreshes, 1);
    const second = await provider.getChildren();
    assert.ok(second.length > 0);
    assert.ok(calls >= 2);
  });

  it("caps retries for genuinely empty plans", async () => {
    let refreshes = 0;
    planService.getPlan = async () => [];
    provider = new ExosuitTreeProvider("project-plan", {
      retryBackoffMs: [1, 1],
      maxEmptyRetries: 2,
    });
    provider.onDidChangeTreeData(() => {
      refreshes++;
      void provider.getChildren();
    });

    const first = await provider.getChildren();
    assert.strictEqual(first.length, 0);

    await new Promise((resolve) => setTimeout(resolve, 30));

    assert.strictEqual(refreshes, 2);
  });
});
