import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import snapshotFixture from "./workbench-snapshot.v1.json";
import { decodeWorkbenchSnapshot } from "./workbench";
import WorkbenchView from "./WorkbenchView.svelte";

afterEach(cleanup);

const fixture = () => {
  const value = structuredClone(snapshotFixture);
  value.lanes.push({
    id: "lane-next",
    title: "Focus-only lane workspace",
    state: "prepared",
    phase_id: "phase-next",
    phase_title: "Lane workspace delivery",
    phase_status: "pending",
    focused_here: false,
  });
  return decodeWorkbenchSnapshot(value);
};

describe("focus-only lane workbench", () => {
  it("gives lane intent, plan, agent context, and workspace identity clear hierarchy", () => {
    render(WorkbenchView, {
      snapshot: fixture(),
      streamConnected: true,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(
      screen.getByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(screen.getByText("Build the host and launch substrate")).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Workbench foundation" }),
    ).toBeTruthy();
    expect(screen.getByText("Implement host")).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Coordination" }),
    ).toBeTruthy();
    expect(screen.getByText("Agent next step")).toBeTruthy();
    expect(screen.getByText("Continue implementation")).toBeTruthy();
    expect(screen.getAllByText("main").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Modified")).toBeTruthy();
    expect(
      screen.queryByRole("heading", { name: "Diagnostics" }),
    ).toBeNull();
    expect(screen.queryByText("No active diagnostics")).toBeNull();

    const details = screen.getByText("Agent details").closest("details");
    expect(details).toHaveProperty("open", false);
    expect(
      screen.getByText("The local host implementation is active."),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "task log host-goal::implement-host --message <message>",
      ),
    ).toBeTruthy();
  });

  it("shows only the first agent action as quiet coordination context", () => {
    const snapshot = fixture();
    snapshot.steering.next_actions.push({
      label: "Package the work",
      command: "task complete host-goal::implement-host",
      rationale: "Move to the next boundary.",
      intent: "complete",
      confidence: 0.8,
    });

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(screen.getByText("Continue implementation")).toBeTruthy();
    expect(screen.queryByText("Package the work")).toBeNull();
  });

  it("hides coordination when there is no agent guidance or diagnostic", () => {
    const snapshot = fixture();
    snapshot.steering.next_actions = [];
    snapshot.diagnostics = [];

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(
      screen.queryByRole("complementary", { name: "Coordination" }),
    ).toBeNull();
  });

  it("shows diagnostics only when Exo reports them", () => {
    const snapshot = fixture();
    snapshot.steering.next_actions = [];
    snapshot.diagnostics = [
      {
        code: "workbench.focus_mismatch",
        severity: "warning",
        message: "The focused lane no longer matches this workspace phase.",
      },
    ];

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(
      screen.getByRole("complementary", { name: "Coordination" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Diagnostics" }),
    ).toBeTruthy();
    expect(screen.getByText("workbench.focus_mismatch")).toBeTruthy();
    expect(
      screen.getByText(
        "The focused lane no longer matches this workspace phase.",
      ),
    ).toBeTruthy();
  });

  it("focuses another existing lane through the only mutable control", async () => {
    const onFocus = vi.fn();
    render(WorkbenchView, {
      snapshot: fixture(),
      onFocus,
      onRefresh: vi.fn(),
    });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Focus Focus-only lane workspace",
      }),
    );

    expect(onFocus).toHaveBeenCalledWith("lane-next");
  });

  it("renders pending focus and retryable failure without changing local state", async () => {
    const onRetryFocus = vi.fn();
    render(WorkbenchView, {
      snapshot: fixture(),
      pendingLaneId: "lane-next",
      focusFailure: "The lane focus request could not be completed",
      onFocus: vi.fn(),
      onRetryFocus,
      onRefresh: vi.fn(),
    });

    expect(
      screen.getByRole("button", {
        name: "Focusing Focus-only lane workspace",
      }),
    ).toHaveProperty("disabled", true);
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetryFocus).toHaveBeenCalledOnce();
  });

  it("keeps the lane rail useful when no lane is focused", () => {
    const snapshot = fixture();
    snapshot.focused_lane = null;
    snapshot.phase = null;
    snapshot.lanes = snapshot.lanes.map((lane) => ({
      ...lane,
      focused_here: false,
    }));

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(
      screen.getByRole("heading", { name: "No lane focused here" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Focus Local workbench host" }),
    ).toBeTruthy();
  });

  it("keeps completed-phase lanes visible without making them focusable", async () => {
    const snapshot = fixture();
    snapshot.lanes.push({
      id: "lane-history",
      title: "Completed lane",
      state: "prepared",
      phase_id: "phase-history",
      phase_title: "Completed phase",
      phase_status: "completed",
      focused_here: false,
    });
    const onFocus = vi.fn();

    render(WorkbenchView, {
      snapshot,
      onFocus,
      onRefresh: vi.fn(),
    });

    const completedLane = screen.getByRole("button", {
      name: "Completed lane, phase completed",
    });
    expect(completedLane).toHaveProperty("disabled", true);
    expect(completedLane.getAttribute("title")).toBe(
      "This lane’s phase is complete",
    );
    await fireEvent.click(completedLane);
    expect(onFocus).not.toHaveBeenCalled();
  });
});
