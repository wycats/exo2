import { describe, expect, it, vi } from "vitest";

import {
  renderWorkLanes,
  WorkLaneTreeItem,
  WorkLanesStateItem,
} from "../WorkLanesProvider";
import {
  focusableWorkbenchLanes,
  focusWorkbenchLane,
  listWorkbenchLanes,
  workbenchLaneFocusTargetId,
  type WorkbenchLaneList,
  workbenchLaneQuickPickItem,
} from "./WorkLanesClient";
import type { TraceCacheRootDiagnostic } from "./TraceCache";

const preparedLane = {
  id: "lane-prepared",
  title: "Prepare editor proof",
  intent: "Make the first editor surface inspectable.",
  state: "prepared" as const,
  created_at: "2026-07-25T00:00:00Z",
  updated_at: "2026-07-25T00:00:00Z",
  phase_id: "phase-editor",
  phase_title: "Editor Proof",
  phase_status: "in-progress",
  focused_here: false,
};

const focusedLane = {
  ...preparedLane,
  id: "lane-focused",
  title: "Focused lane",
  state: "executing" as const,
  focused_here: true,
};

function laneList(overrides: Partial<WorkbenchLaneList> = {}): WorkbenchLaneList {
  return {
    kind: "lane.list",
    ok: true,
    lanes: [preparedLane, focusedLane],
    diagnostics: [],
    ...overrides,
  };
}

function roots(value: unknown): ReadonlyMap<string, unknown> {
  return new Map([["work-lanes", value]]);
}

function diagnostics(
  value?: TraceCacheRootDiagnostic,
): ReadonlyMap<string, TraceCacheRootDiagnostic | undefined> {
  return new Map([["work-lanes", value]]);
}

describe("Work Lanes provider", () => {
  it("renders lane state, phase, and workspace focus", () => {
    const items = renderWorkLanes(roots(laneList()), diagnostics());

    expect(items).toHaveLength(2);
    expect(items[0]).toBeInstanceOf(WorkLaneTreeItem);
    expect(items[0]?.label).toBe("Prepare editor proof");
    expect(items[0]?.description).toBe("prepared • Editor Proof");
    expect(items[0]?.contextValue).toBe("workbench-lane");
    expect(items[1]?.description).toBe(
      "focused • executing • Editor Proof",
    );
    expect(items[1]?.contextValue).toBe("workbench-lane-focused");
    expect(items[1]?.iconPath).toMatchObject({ id: "target" });
  });

  it("offers the row focus action only for lanes in an in-progress phase", () => {
    for (const phaseStatus of [
      "pending",
      "completed",
      "abandoned",
      "deferred",
    ]) {
      const item = new WorkLaneTreeItem({
        ...preparedLane,
        phase_status: phaseStatus,
      });
      expect(item.contextValue).toBe("workbench-lane-unfocusable");
    }

    expect(new WorkLaneTreeItem(preparedLane).contextValue).toBe(
      "workbench-lane",
    );
  });

  it("renders loading, empty, and transport-error states", () => {
    expect(
      renderWorkLanes(roots(undefined), diagnostics())[0],
    ).toMatchObject({
      label: "Loading work lanes",
    });
    expect(
      renderWorkLanes(roots(laneList({ lanes: [] })), diagnostics())[0],
    ).toMatchObject({
      label: "No workbench lanes",
    });
    expect(
      renderWorkLanes(
        roots(null),
        diagnostics({
          rootId: "work-lanes",
          namespace: "lane",
          operation: "list",
          status: "error",
          input: {},
          explicitInput: false,
          fetchedAt: 1,
          error: { message: "daemon unavailable" },
        }),
      )[0],
    ).toMatchObject({
      label: "Work lanes unavailable",
      description: "retrying",
      tooltip: "daemon unavailable",
    });
  });

  it("renders focus mismatch diagnostics ahead of lanes", () => {
    const items = renderWorkLanes(
      roots(
        laneList({
          diagnostics: [
            {
              code: "lane.phase_focus_mismatch",
              message: "Focused lane and phase do not match",
              lane_id: focusedLane.id,
              lane_phase_id: focusedLane.phase_id,
              focused_phase_id: "other-phase",
            },
          ],
        }),
      ),
      diagnostics(),
    );

    expect(items[0]).toBeInstanceOf(WorkLanesStateItem);
    expect(items[0]).toMatchObject({
      label: "Lane focus needs attention",
      description: "phase mismatch",
      tooltip: "Focused lane and phase do not match",
    });
    expect(items.slice(1)).toHaveLength(2);
  });

  it("renders linked-worktree focus entirely from each canonical payload", () => {
    const primary = laneList({
      lanes: [
        { ...preparedLane, focused_here: true },
        { ...focusedLane, focused_here: false },
      ],
    });
    const linked = laneList({
      lanes: [
        { ...preparedLane, focused_here: false },
        { ...focusedLane, focused_here: true },
      ],
    });

    const primaryItems = renderWorkLanes(roots(primary), diagnostics());
    const linkedItems = renderWorkLanes(roots(linked), diagnostics());

    expect(primaryItems.map((item) => item.description)).toEqual([
      "focused • prepared • Editor Proof",
      "executing • Editor Proof",
    ]);
    expect(linkedItems.map((item) => item.description)).toEqual([
      "prepared • Editor Proof",
      "focused • executing • Editor Proof",
    ]);
    expect(
      renderWorkLanes(roots(primary), diagnostics()).map(
        (item) => item.description,
      ),
    ).toEqual([
      "focused • prepared • Editor Proof",
      "executing • Editor Proof",
    ]);
  });
});

describe("Work Lanes machine-channel client", () => {
  it("lists lanes through lane.list", async () => {
    const send = vi.fn(async () => ({
      protocol_version: 1,
      id: "list",
      status: "ok" as const,
      result: laneList(),
    }));

    const data = await listWorkbenchLanes("/workspace", "list", send);
    expect(data.lanes).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: preparedLane.id }),
      ]),
    );
    expect(send).toHaveBeenCalledWith(
      "/workspace",
      expect.objectContaining({
        id: "list",
        op: {
          kind: "call",
          params: {
            address: { kind: "operation", path: ["lane", "list"] },
            input: {},
          },
        },
      }),
    );
  });

  it("focuses a lane through lane.focus and surfaces errors", async () => {
    const send = vi
      .fn()
      .mockResolvedValueOnce({
        protocol_version: 1,
        id: "focus",
        status: "ok",
        result: { kind: "lane.focus", ok: true, lane: focusedLane },
      })
      .mockResolvedValueOnce({
        protocol_version: 1,
        id: "focus-failed",
        status: "error",
        error: { code: "precondition_failed", message: "Phase is pending" },
      });

    await expect(
      focusWorkbenchLane("/workspace", focusedLane.id, "focus", send),
    ).resolves.toEqual(focusedLane);
    expect(send).toHaveBeenNthCalledWith(
      1,
      "/workspace",
      expect.objectContaining({
        op: {
          kind: "call",
          params: {
            address: { kind: "operation", path: ["lane", "focus"] },
            input: { id: focusedLane.id },
          },
        },
      }),
    );
    await expect(
      focusWorkbenchLane(
        "/workspace",
        preparedLane.id,
        "focus-failed",
        send,
      ),
    ).rejects.toThrow("Phase is pending");
  });

  it("selects unfocused or mismatched lanes whose phases are in progress", () => {
    const pending = {
      ...preparedLane,
      id: "lane-pending-phase",
      phase_status: "pending",
    };
    expect(
      focusableWorkbenchLanes(
        laneList({ lanes: [preparedLane, focusedLane, pending] }),
      ).map((lane) => lane.id),
    ).toEqual([preparedLane.id]);
    expect(
      focusableWorkbenchLanes(
        laneList({
          lanes: [preparedLane, focusedLane, pending],
          diagnostics: [
            {
              code: "lane.phase_focus_mismatch",
              message: "Focused lane and phase do not match",
              lane_id: focusedLane.id,
              lane_phase_id: focusedLane.phase_id,
              focused_phase_id: "other-phase",
            },
          ],
        }),
      ).map((lane) => lane.id),
    ).toEqual([preparedLane.id, focusedLane.id]);
    expect(workbenchLaneFocusTargetId(preparedLane.id)).toBe(
      preparedLane.id,
    );
    expect(workbenchLaneFocusTargetId({ lane: preparedLane })).toBe(
      preparedLane.id,
    );
  });

  it("distinguishes duplicate lane titles with durable IDs", () => {
    const duplicate = {
      ...preparedLane,
      id: "lane-duplicate",
    };

    expect(
      [preparedLane, duplicate]
        .map(workbenchLaneQuickPickItem)
        .map((item) => item.detail),
    ).toEqual([
      `${preparedLane.intent}\nLane ID: ${preparedLane.id}`,
      `${duplicate.intent}\nLane ID: ${duplicate.id}`,
    ]);
  });
});
