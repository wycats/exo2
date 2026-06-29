import { describe, expect, it, vi } from "vitest";

import {
  buildPlanReorganizationRequest,
  queuePlanReorganizationRequest,
  resolvePlanEntityId,
} from "./PlanReorganizationRequests";

describe("PlanReorganizationRequests", () => {
  it("builds human-readable request metadata with structured action data", () => {
    const request = buildPlanReorganizationRequest({
      type: "goal.move",
      goal_id: "daemon-direct-parity",
      phase_id: "planning-reorganization-surface",
      position: "bottom",
    });

    expect(request.entityType).toBe("goal");
    expect(request.entityId).toBe("daemon-direct-parity");
    expect(request.subject).toContain("Recommend moving goal");
    expect(request.body).toContain("queued this plan reorganization request");
    expect(request.action).toEqual({
      type: "goal.move",
      goal_id: "daemon-direct-parity",
      phase_id: "planning-reorganization-surface",
      position: "bottom",
    });
  });

  it("queues reorganization through inbox add with action_json", async () => {
    const send = vi.fn().mockResolvedValue({
      protocol_version: 1,
      id: "response",
      status: "ok",
      result: { ok: true },
    });

    await queuePlanReorganizationRequest(
      "/workspace",
      {
        type: "phase.move",
        phase_id: "phase-a",
        epoch_id: "epoch-b",
        position: "after:phase-b",
      },
      send,
    );

    expect(send).toHaveBeenCalledTimes(1);
    const request = send.mock.calls[0][0];
    expect(request.op.params.address.path).toEqual(["inbox", "add"]);
    expect(request.op.params.input).toMatchObject({
      entity_type: "phase",
      entity_id: "phase-a",
      source: "user-feedback",
      intent: "fyi",
      priority: "immediate",
    });
    expect(JSON.parse(request.op.params.input.action_json)).toEqual({
      type: "phase.move",
      phase_id: "phase-a",
      epoch_id: "epoch-b",
      position: "after:phase-b",
    });
  });

  it("normalizes tree item ids from sidebar providers", () => {
    expect(resolvePlanEntityId("epoch-1/phase-1/goal-1")).toBe("goal-1");
    expect(resolvePlanEntityId("epoch-phase:phase-1")).toBe("phase-1");
    expect(resolvePlanEntityId({ id: "goal:goal-1" })).toBe("goal-1");
  });
});
