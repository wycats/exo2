import { describe, expect, it } from "vitest";

import inspectionFixture from "./workbench-lane-inspection.v1.json";
import snapshotFixture from "./workbench-snapshot.v3.json";
import {
  decodeWorkbenchLaneInspection,
  decodeWorkbenchSnapshot,
} from "./workbench";

describe("workbench snapshot contract", () => {
  it("decodes the Rust-owned version-three fixture", () => {
    const snapshot = decodeWorkbenchSnapshot(snapshotFixture);

    expect(snapshot.kind).toBe("workbench.snapshot");
    expect(snapshot.project_workspaces).toHaveLength(2);
    expect(snapshot.project_workspaces[0]?.current).toBe(true);
    expect(snapshot.project_workspaces[1]?.availability).toBe("stale");
    expect(snapshot.project_workspaces[1]?.dirty).toBeNull();
    expect(snapshot.focused_lane?.id).toBe("lane-fixture");
    expect(snapshot.phase?.planning_available).toBe(true);
    expect(snapshot.phase?.goals[0]?.tasks[0]?.id).toBe("implement-host");
    expect(snapshot.phase?.goals[0]?.tasks[0]?.progress).toEqual([
      {
        message: "Captured browser evidence.",
        created_at: "2026-07-28T19:45:00Z",
      },
    ]);
  });

  it("requires one project workspace to match the session workspace", () => {
    const malformed = structuredClone(snapshotFixture);
    malformed.project_workspaces[0]!.current = false;

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: project_workspaces.current",
    );
  });

  it("rejects unknown project workspace availability", () => {
    const malformed = structuredClone(snapshotFixture);
    (
      malformed.project_workspaces[1] as {
        availability: string;
      }
    ).availability = "probably-live";

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: project_workspace.availability",
    );
  });

  it("decodes browser-safe between-phase trajectory context", () => {
    const betweenPhases = structuredClone(snapshotFixture);
    const mutable = betweenPhases as unknown as {
      focused_lane: unknown;
      phase: unknown;
      between_phases_context: unknown;
    };
    mutable.focused_lane = null;
    mutable.phase = null;
    mutable.between_phases_context = {
      epoch_id: "epoch-fixture",
      epoch_title: "Workbench epoch",
      completed_phase: {
        id: "phase-fixture",
        title: "Workbench foundation",
        completed_at: "2026-08-01T20:00:00+00:00",
        goal_count: 2,
        completed_goals: 2,
      },
      next_phase: {
        id: "phase-next",
        title: "Next workbench slice",
        goal_count: 1,
        rfc_count: 1,
      },
      pending_phases: 2,
    };

    const decoded = decodeWorkbenchSnapshot(betweenPhases);
    expect(decoded.between_phases_context?.completed_phase?.id).toBe(
      "phase-fixture",
    );
    expect(decoded.between_phases_context?.next_phase?.id).toBe("phase-next");
    expect(decoded.between_phases_context?.pending_phases).toBe(2);
  });

  it("rejects invalid between-phase counts", () => {
    const malformed = structuredClone(snapshotFixture);
    (malformed as { between_phases_context: unknown }).between_phases_context = {
      epoch_id: "epoch-fixture",
      epoch_title: "Workbench epoch",
      completed_phase: null,
      next_phase: null,
      pending_phases: -1,
    };

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: between_phases_context.pending_phases",
    );
  });

  it("requires explicit planning availability for a focused phase", () => {
    const malformed = structuredClone(snapshotFixture);
    delete (malformed.phase as { planning_available?: boolean })
      .planning_available;

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: phase.planning_available",
    );
  });

  it("rejects malformed nested lane state", () => {
    const malformed = structuredClone(snapshotFixture);
    malformed.lanes[0]!.state = "completed";

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: lane.state",
    );
  });

  it("accepts a bounded progress-history marker", () => {
    const bounded = structuredClone(snapshotFixture);
    (
      bounded.phase.goals[0]!.tasks[0] as {
        progress_truncated?: boolean;
      }
    ).progress_truncated = true;

    expect(
      decodeWorkbenchSnapshot(bounded).phase?.goals[0]?.tasks[0]
        ?.progress_truncated,
    ).toBe(true);
  });

  it("rejects non-finite steering confidence", () => {
    const malformed = structuredClone(snapshotFixture);
    malformed.steering.next_actions[0]!.confidence = Number.NaN;

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: suggested_action.confidence",
    );
  });
});

describe("workbench lane inspection contract", () => {
  it("decodes the Rust-owned version-one fixture", () => {
    const inspection = decodeWorkbenchLaneInspection(inspectionFixture);

    expect(inspection.kind).toBe("workbench.lane_inspection");
    expect(inspection.relationship).toBe("historical");
    expect(inspection.can_focus_here).toBe(false);
    expect(inspection.lane.id).toBe("lane-history");
    expect(inspection.phase.goals[0]?.outcome).toBe(
      "The first lane-centered cockpit is available for dogfood.",
    );
    expect(inspection.phase.goals[0]?.tasks[0]?.outcome).toBe(
      "The reviewed foundation landed cleanly.",
    );
  });

  it("rejects an unknown inspection relationship", () => {
    const malformed = structuredClone(inspectionFixture);
    (malformed as { relationship: string }).relationship = "active_elsewhere";

    expect(() => decodeWorkbenchLaneInspection(malformed)).toThrow(
      "Invalid workbench snapshot field: inspection.relationship",
    );
  });
});
