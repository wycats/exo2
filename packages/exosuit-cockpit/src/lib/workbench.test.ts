import { describe, expect, it } from "vitest";

import snapshotFixture from "./workbench-snapshot.v1.json";
import { decodeWorkbenchSnapshot } from "./workbench";

describe("workbench snapshot contract", () => {
  it("decodes the Rust-owned version-one fixture", () => {
    const snapshot = decodeWorkbenchSnapshot(snapshotFixture);

    expect(snapshot.kind).toBe("workbench.snapshot");
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
