import { beforeEach, describe, expect, it, vi } from "vitest";

const machineChannelMock = vi.hoisted(() => vi.fn());

vi.mock("../agent/lmtool/machineChannel", () => ({
  exoMachineChannel: machineChannelMock,
}));

vi.mock("../workspaceRoot", () => ({
  currentWorkspaceRoot: () => "/workspace",
}));

import { PlanService } from "../PlanService";

function planResponse(title = "Epoch") {
  return {
    protocol_version: 1,
    id: "test.response",
    status: "ok" as const,
    result: {
      epochs: [
        {
          id: "epoch-1",
          title,
          status: "in-progress",
          phases: [
            {
              id: "phase-1",
              title: "Phase",
              status: "in-progress",
              goals: [],
            },
          ],
        },
      ],
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("PlanService", () => {
  beforeEach(() => {
    machineChannelMock.mockReset();
    PlanService.instance.invalidate();
  });

  it("joins concurrent getPlan calls", async () => {
    const pending = deferred<ReturnType<typeof planResponse>>();
    machineChannelMock.mockReturnValueOnce(pending.promise);

    const first = PlanService.instance.getPlan();
    const second = PlanService.instance.getPlan();

    expect(machineChannelMock).toHaveBeenCalledTimes(1);

    pending.resolve(planResponse());

    await expect(first).resolves.toHaveLength(1);
    await expect(second).resolves.toHaveLength(1);
  });

  it("clears in-flight plan reads after failure so later calls retry", async () => {
    machineChannelMock
      .mockRejectedValueOnce(new Error("daemon unavailable"))
      .mockResolvedValueOnce(planResponse("Retry Epoch"));

    await expect(PlanService.instance.getPlan()).resolves.toEqual([]);
    await expect(PlanService.instance.getPlan()).resolves.toMatchObject([
      { title: "Retry Epoch" },
    ]);

    expect(machineChannelMock).toHaveBeenCalledTimes(2);
  });

  it("does not cache stale getPlan results after invalidate", async () => {
    const stalePending = deferred<ReturnType<typeof planResponse>>();
    machineChannelMock
      .mockReturnValueOnce(stalePending.promise)
      .mockResolvedValueOnce(planResponse("Fresh Epoch"));

    const stale = PlanService.instance.getPlan();
    PlanService.instance.invalidate();
    stalePending.resolve(planResponse("Stale Epoch"));

    await expect(stale).resolves.toEqual([]);
    await expect(PlanService.instance.getPlan()).resolves.toMatchObject([
      { title: "Fresh Epoch" },
    ]);

    expect(machineChannelMock).toHaveBeenCalledTimes(2);
  });
});
