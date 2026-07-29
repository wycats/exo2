import { describe, expect, it } from "vitest";

import snapshotFixture from "./workbench-snapshot.v1.json";
import { decodeWorkbenchSnapshot } from "./workbench";

describe("workbench snapshot contract", () => {
  it("decodes the Rust-owned version-one fixture", () => {
    const snapshot = decodeWorkbenchSnapshot(snapshotFixture);

    expect(snapshot.kind).toBe("workbench.snapshot");
    expect(snapshot.focused_lane?.id).toBe("lane-fixture");
    expect(snapshot.phase?.goals[0]?.tasks[0]?.id).toBe("implement-host");
  });

  it("rejects malformed nested lane state", () => {
    const malformed = structuredClone(snapshotFixture);
    malformed.lanes[0]!.state = "completed";

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: lane.state",
    );
  });

  it("rejects non-finite steering confidence", () => {
    const malformed = structuredClone(snapshotFixture);
    malformed.steering.next_actions[0]!.confidence = Number.NaN;

    expect(() => decodeWorkbenchSnapshot(malformed)).toThrow(
      "Invalid workbench snapshot field: suggested_action.confidence",
    );
  });
});
