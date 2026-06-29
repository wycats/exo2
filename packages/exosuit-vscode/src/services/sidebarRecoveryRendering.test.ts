import { describe, expect, it } from "vitest";

import { renderEpochContext } from "../EpochContextProvider";
import { renderPhaseDetails } from "../PhaseDetailsProvider";
import type { TraceCacheRootDiagnostic } from "./TraceCache";

function phaseDetailsRoots(value: unknown): ReadonlyMap<string, unknown> {
  return new Map([["phase-details", value]]);
}

function rootsWithStatus(
  phaseDetails: unknown,
  status: unknown,
  plan: unknown,
): ReadonlyMap<string, unknown> {
  return new Map([
    ["phase-details", phaseDetails],
    ["status", status],
    ["plan-read", plan],
  ]);
}

function diagnostics(
  diagnostic: TraceCacheRootDiagnostic,
): ReadonlyMap<string, TraceCacheRootDiagnostic | undefined> {
  return new Map([["phase-details", diagnostic]]);
}

function staleFocusDiagnostic(): TraceCacheRootDiagnostic {
  return {
    rootId: "phase-details",
    namespace: "phase",
    operation: "read-details",
    status: "empty",
    input: { id: "stale-phase" },
    explicitInput: true,
    fetchedAt: 1_779_000_000_000,
  };
}

describe("sidebar stale-focus recovery rendering", () => {
  it("renders Phase Details stale-focus recovery state from explicit empty diagnostics", () => {
    const [item] = renderPhaseDetails(
      phaseDetailsRoots(null),
      diagnostics(staleFocusDiagnostic()),
    );

    expect(item?.label).toBe("Focused phase not found");
    expect(item?.id).toBe("phase-details-stale-focus");
    expect(item?.contextValue).toBe("phase-details-stale-focus");
    expect(item?.description).toBe("cleared stale-phase");
    expect(item?.tooltip).toContain("selected phase no longer exists");
    expect(item?.tooltip).toContain('{"id":"stale-phase"}');
  });

  it("renders Epoch Context stale-focus recovery state from the same diagnostic", () => {
    const [item] = renderEpochContext(
      phaseDetailsRoots(null),
      diagnostics(staleFocusDiagnostic()),
    );

    expect(item?.label).toBe("Focused phase not found");
    expect(item?.id).toBe("epoch-stale-focus");
    expect(item?.contextValue).toBe("epoch-stale-focus");
    expect(item?.description).toBe("cleared stale-phase");
    expect(item?.tooltip).toContain("selected phase no longer exists");
    expect(item?.tooltip).toContain('{"id":"stale-phase"}');
  });

  it("keeps active-phase empty states distinct from stale focused phase", () => {
    const emptyActiveDiagnostic: TraceCacheRootDiagnostic = {
      ...staleFocusDiagnostic(),
      input: {},
      explicitInput: false,
    };

    const [phaseItem] = renderPhaseDetails(
      phaseDetailsRoots(null),
      diagnostics(emptyActiveDiagnostic),
    );
    const [epochItem] = renderEpochContext(
      phaseDetailsRoots(null),
      diagnostics(emptyActiveDiagnostic),
    );

    expect(phaseItem?.label).toBe("No active phase");
    expect(phaseItem?.contextValue).toBe("no-phase-message");
    expect(phaseItem?.description).toBeUndefined();
    expect(epochItem?.label).toBe("No active epoch");
    expect(epochItem?.contextValue).toBe("epoch-empty");
    expect(epochItem?.description).toBeUndefined();
  });

  it("renders active epoch context while between phases", () => {
    const [epochItem] = renderEpochContext(
      rootsWithStatus(
        null,
        {
          progress_mode: "between-phases",
          between_phases_context: {
            epoch_id: "epoch-1",
            epoch_title: "Sidecar Dogfooding & Ambient Guidance",
            completed_phase: {
              phase_id: "phase-done",
              phase_title: "Migration-Aware Upgrade Guidance",
              goal_count: 4,
              completed_goals: 4,
            },
            next_phase: {
              id: "phase-next",
              title: "GitHub Profile Sidecar Discovery",
              goal_count: 0,
              rfcs: [],
            },
            is_epoch_finale: false,
          },
        },
        {
          epochs: [
            {
              id: "epoch-1",
              title: "Sidecar Dogfooding & Ambient Guidance",
              status: "in-progress",
              phases: [
                {
                  id: "phase-done",
                  title: "Migration-Aware Upgrade Guidance",
                  status: "completed",
                  goalCount: 4,
                  completedGoals: 4,
                },
                {
                  id: "phase-next",
                  title: "GitHub Profile Sidecar Discovery",
                  status: "pending",
                  goalCount: 0,
                  completedGoals: 0,
                },
              ],
            },
          ],
        },
      ),
      diagnostics({
        ...staleFocusDiagnostic(),
        input: {},
        explicitInput: false,
      }),
    );

    expect(epochItem?.label).toBe("Migration-Aware Upgrade Guidance");
    expect(epochItem?.id).toBe("epoch-phase:phase-done");
    expect(epochItem?.contextValue).toBe("epoch-sibling-phase");
    expect(epochItem?.description).toBe("4 goals completed");

    const [, nextItem] = renderEpochContext(
      rootsWithStatus(
        null,
        {
          progress_mode: "between-phases",
          between_phases_context: {
            epoch_id: "epoch-1",
            epoch_title: "Sidecar Dogfooding & Ambient Guidance",
            next_phase: {
              id: "phase-next",
              title: "GitHub Profile Sidecar Discovery",
              goal_count: 0,
              rfcs: [],
            },
            is_epoch_finale: false,
          },
        },
        {
          epochs: [
            {
              id: "epoch-1",
              title: "Sidecar Dogfooding & Ambient Guidance",
              status: "in-progress",
              phases: [
                {
                  id: "phase-done",
                  title: "Migration-Aware Upgrade Guidance",
                  status: "completed",
                  goalCount: 4,
                  completedGoals: 4,
                },
                {
                  id: "phase-next",
                  title: "GitHub Profile Sidecar Discovery",
                  status: "pending",
                  goalCount: 0,
                  completedGoals: 0,
                },
              ],
            },
          ],
        },
      ),
      diagnostics({
        ...staleFocusDiagnostic(),
        input: {},
        explicitInput: false,
      }),
    );
    expect(nextItem?.label).toBe("GitHub Profile Sidecar Discovery");
    expect(nextItem?.contextValue).toBe("epoch-next-phase");
    expect(nextItem?.description).toBe("next");
  });

  it("renders Phase Details from status and plan while between phases", () => {
    const items = renderPhaseDetails(
      rootsWithStatus(
        null,
        {
          progress_mode: "between-phases",
          between_phases_context: {
            epoch_id: "epoch-1",
            epoch_title: "Sidecar Dogfooding & Ambient Guidance",
            completed_phase: {
              phase_id: "phase-done",
              phase_title: "Migration-Aware Upgrade Guidance",
              goal_count: 4,
              completed_goals: 4,
            },
            next_phase: {
              id: "phase-next",
              title: "GitHub Profile Sidecar Discovery",
              goal_count: 2,
              rfcs: [],
            },
            is_epoch_finale: false,
          },
        },
        {
          epochs: [
            {
              id: "epoch-1",
              title: "Sidecar Dogfooding & Ambient Guidance",
              status: "in-progress",
              phases: [
                {
                  id: "phase-done",
                  title: "Migration-Aware Upgrade Guidance",
                  status: "completed",
                  goalCount: 4,
                  completedGoals: 4,
                },
                {
                  id: "phase-next",
                  title: "GitHub Profile Sidecar Discovery",
                  status: "pending",
                  goalCount: 2,
                  completedGoals: 0,
                },
              ],
            },
          ],
        },
      ),
      diagnostics({
        ...staleFocusDiagnostic(),
        input: {},
        explicitInput: false,
      }),
    );

    expect(items.map((item) => item.label)).toEqual([
      "Between phases",
      "Migration-Aware Upgrade Guidance",
      "GitHub Profile Sidecar Discovery",
    ]);
    expect(items[0]?.contextValue).toBe("phase-between-phases");
    expect(items[0]?.description).toBe(
      "next: GitHub Profile Sidecar Discovery",
    );
    expect(items[1]?.contextValue).toBe("phase-between-completed");
    expect(items[1]?.description).toBe("4 goals completed");
    expect(items[2]?.contextValue).toBe("phase-between-next");
    expect(items[2]?.description).toBe("2 goals planned");
  });
});
