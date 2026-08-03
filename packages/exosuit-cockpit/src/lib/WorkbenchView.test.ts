import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import snapshotFixture from "./workbench-snapshot.v2.json";
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
    phase_status: "in-progress",
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
    const activeTaskCopy = screen
      .getByText("Implement host")
      .closest<HTMLElement>(".task-copy");
    expect(activeTaskCopy).toBeTruthy();
    expect(within(activeTaskCopy!).getByText("Active")).toBeTruthy();
    expect(
      screen.getByText("0 of 1 tasks complete", {
        selector: ".goal-progress",
      }),
    ).toBeTruthy();
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

  it("uses one command-invoked lane navigator for persistent and popover layouts", () => {
    render(WorkbenchView, {
      snapshot: fixture(),
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    const invoker = screen.getByRole("button", {
      name: "Open project lanes",
    });
    const navigator = screen.getByRole("complementary", {
      name: "Project lanes",
    });
    expect(invoker.getAttribute("commandfor")).toBe("lane-navigation");
    expect(invoker.getAttribute("command")).toBe("toggle-popover");
    expect(navigator.id).toBe("lane-navigation");
  });

  it("distinguishes underway goals from goals that have not started", () => {
    const snapshot = fixture();
    snapshot.phase!.goals = [
      {
        id: "underway",
        title: "Underway goal",
        status: "pending",
        tasks: [
          { id: "done", title: "Completed evidence", status: "completed" },
          { id: "next", title: "Pending follow-up", status: "pending" },
        ],
      },
      {
        id: "not-started",
        title: "Not started goal",
        status: "pending",
        tasks: [
          { id: "future", title: "Future work", status: "pending" },
        ],
      },
      {
        id: "tasks-complete",
        title: "Tasks complete goal",
        status: "pending",
        tasks: [
          { id: "done-one", title: "First result", status: "completed" },
          { id: "done-two", title: "Second result", status: "completed" },
        ],
      },
    ];

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    const underway = screen
      .getByRole("heading", { name: "Underway goal" })
      .closest("article");
    const notStarted = screen
      .getByRole("heading", { name: "Not started goal" })
      .closest("article");
    const tasksComplete = screen
      .getByRole("heading", { name: "Tasks complete goal" })
      .closest("article");
    expect(within(underway!).getByText("1 of 2 tasks complete")).toBeTruthy();
    expect(within(notStarted!).getByText("Not started")).toBeTruthy();
    expect(
      within(tasksComplete!).getByText("Tasks complete · Goal pending"),
    ).toBeTruthy();
    expect(
      within(underway!).queryByText("Pending", {
        selector: ".goal-copy > span",
      }),
    ).toBeNull();
  });

  it("keeps icon-only task status accessible and exposes recorded progress", async () => {
    const snapshot = fixture();
    snapshot.phase!.goals[0]!.tasks.push(
      {
        id: "future-proof",
        title: "Future proof",
        status: "pending",
      },
      {
        id: "captured-proof",
        title: "Captured proof",
        status: "completed",
      },
    );
    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(screen.getByText("Pending", { selector: ".sr-only" })).toBeTruthy();
    expect(screen.getByText("Completed", { selector: ".sr-only" })).toBeTruthy();
    await fireEvent.click(screen.getByText("1 progress update"));
    expect(screen.getByText("Captured browser evidence.")).toBeTruthy();
    expect(
      document.querySelector('time[datetime="2026-07-28T19:45:00Z"]'),
    ).toBeTruthy();
  });

  it("identifies a bounded recent progress window", async () => {
    const snapshot = fixture();
    snapshot.phase!.goals[0]!.tasks[0]!.progress_truncated = true;
    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    await fireEvent.click(screen.getByText("1 recent progress update"));
    expect(
      screen.getByText("Earlier or longer updates remain available in Exo."),
    ).toBeTruthy();
  });

  it("shows terminal planning entities without mutation controls", () => {
    const snapshot = fixture();
    snapshot.phase!.goals = [
      {
        id: "abandoned-goal",
        title: "Abandoned direction",
        status: "abandoned",
        tasks: [
          {
            id: "abandoned-task",
            title: "Abandoned task",
            status: "abandoned",
          },
        ],
      },
      {
        id: "active-goal",
        title: "Active direction",
        status: "in-progress",
        tasks: [
          {
            id: "skipped-task",
            title: "Skipped task",
            status: "skipped",
          },
        ],
      },
    ];

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan: vi.fn(),
    });

    const abandonedGoal = screen
      .getByRole("heading", { name: "Abandoned direction" })
      .closest("article");
    const activeGoal = screen
      .getByRole("heading", { name: "Active direction" })
      .closest("article");

    expect(
      within(abandonedGoal!)
        .getAllByText("Abandoned")
        .filter((element) => !element.classList.contains("sr-only")),
    ).toHaveLength(2);
    expect(
      within(abandonedGoal!).queryByRole("button", {
        name: "Add task to Abandoned direction",
      }),
    ).toBeNull();
    expect(
      within(abandonedGoal!).queryByLabelText("Actions for Abandoned task"),
    ).toBeNull();
    expect(
      within(activeGoal!).getByText("Skipped", {
        selector: ".task-terminal-label",
      }),
    ).toBeTruthy();
    expect(
      within(activeGoal!).queryByLabelText("Actions for Skipped task"),
    ).toBeNull();
  });

  it("submits bounded task planning from inline controls", async () => {
    const onPlan = vi.fn().mockResolvedValue(true);
    render(WorkbenchView, {
      snapshot: fixture(),
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan,
    });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Add task to Establish local host and launch",
      }),
    );
    await fireEvent.input(screen.getByLabelText("New task"), {
      target: { value: "Validate the browser review card" },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Add task" }));

    await waitFor(() => {
      expect(onPlan).toHaveBeenCalledWith(
        {
          kind: "task_add",
          goal_id: "host-goal",
          title: "Validate the browser review card",
        },
        {
          expected_daemon_instance_id: "daemon-fixture",
          expected_revision: 7,
          expected_phase_id: "phase-fixture",
        },
      );
    });
  });

  it("marks a task active in Exo without implying that an agent started", async () => {
    const snapshot = fixture();
    snapshot.phase!.goals[0]!.tasks.push({
      id: "agent-handoff",
      title: "Prepare the agent handoff",
      status: "pending",
    });
    const onPlan = vi.fn().mockResolvedValue(true);
    render(WorkbenchView, {
      snapshot,
      planningNotice:
        "Exo marked the task active; the workbench did not start an agent.",
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan,
    });

    expect(screen.getByText("Ready for agent handoff.")).toBeTruthy();
    expect(
      screen.getByText(
        "Exo marked the task active; the workbench did not start an agent.",
      ),
    ).toBeTruthy();
    await fireEvent.click(
      screen.getByRole("button", {
        name: "Mark Prepare the agent handoff active in Exo",
      }),
    );
    expect(onPlan).toHaveBeenCalledWith({
      kind: "task_start",
      task_id: "agent-handoff",
    });
    expect(screen.queryByTitle("Start task")).toBeNull();
  });

  it("renders a human completion review with deliberate approval and revision", async () => {
    const onApproveCompletion = vi.fn().mockResolvedValue(true);
    const onDismissCompletionReview = vi.fn();
    const exactOutcome =
      "Implemented the local host.\n  Preserved indented verification.";
    render(WorkbenchView, {
      snapshot: fixture(),
      completionReview: {
        kind: "workbench.task_completion_review",
        ok: true,
        schema_version: 1,
        review_id: "review-selector",
        task_id: "implement-host",
        readiness_rationale: "The exact focused checks pass.",
        proposed_outcome: exactOutcome,
        approval_evidence_present: false,
      },
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onApproveCompletion,
      onDismissCompletionReview,
    });

    expect(
      screen.getByRole("heading", { name: "Implement host" }),
    ).toBeTruthy();
    const reviewedOutcome = document.querySelector(".review-outcome-text");
    expect(reviewedOutcome?.textContent).toBe(exactOutcome);
    expect(reviewedOutcome?.classList.contains("review-outcome-text")).toBe(true);
    await fireEvent.click(
      screen.getByRole("button", { name: "Approve exact outcome" }),
    );
    expect(onApproveCompletion).toHaveBeenCalledOnce();
    await fireEvent.click(
      screen.getByRole("button", { name: "Revise outcome" }),
    );
    expect(onDismissCompletionReview).toHaveBeenCalledOnce();
    expect(
      (screen.getByLabelText("Proposed completion outcome") as HTMLTextAreaElement)
        .value,
    ).toBe(exactOutcome);
    await fireEvent.click(screen.getByRole("button", { name: "Keep working" }));
    expect(onDismissCompletionReview).toHaveBeenCalledTimes(2);
  });

  it("enforces server UTF-8 byte limits before planning submission", async () => {
    const onPlan = vi.fn().mockResolvedValue(true);
    render(WorkbenchView, {
      snapshot: fixture(),
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan,
    });

    await fireEvent.click(
      screen.getByRole("button", { name: "Edit Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Task title"), {
      target: { value: "🙂".repeat(129) },
    });
    expect(screen.getByText("Text is too long (516 of 512 bytes).")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Save task title" }),
    ).toHaveProperty("disabled", true);

    await fireEvent.click(
      screen.getByRole("button", { name: "Cancel editing task" }),
    );
    await fireEvent.click(
      screen.getByRole("button", {
        name: "Review completion of Implement host",
      }),
    );
    await fireEvent.input(screen.getByLabelText("Proposed completion outcome"), {
      target: { value: "🙂".repeat(4097) },
    });
    expect(
      screen.getByText("Text is too long (16388 of 16384 bytes)."),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Review completion" }),
    ).toHaveProperty("disabled", true);
    expect(onPlan).not.toHaveBeenCalled();
  });

  it("preserves an open draft after an unrelated planning success", async () => {
    const props = {
      snapshot: fixture(),
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan: vi.fn().mockResolvedValue(true),
    };
    const view = render(WorkbenchView, props);

    await fireEvent.click(
      screen.getByRole("button", { name: "Edit Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Task title"), {
      target: { value: "A title still being drafted" },
    });
    await view.rerender({
      ...props,
      planningSuccess: {
        requestId: "unrelated-start",
        operation: { kind: "task_start", task_id: "another-task" },
      },
    });

    expect((screen.getByLabelText("Task title") as HTMLInputElement).value).toBe(
      "A title still being drafted",
    );

    await view.rerender({
      ...props,
      planningSuccess: {
        requestId: "matching-update",
        operation: {
          kind: "task_update",
          task_id: "implement-host",
          title: "A title still being drafted",
        },
      },
    });
    expect(screen.queryByLabelText("Task title")).toBeNull();
  });

  it("rebinds a preserved draft only after an explicit stale rejection", async () => {
    const snapshot = fixture();
    const onPlan = vi.fn().mockResolvedValue(false);
    const props = {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan,
    };
    const view = render(WorkbenchView, props);

    await fireEvent.click(
      screen.getByRole("button", { name: "Edit Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Task title"), {
      target: { value: "A title from the observed plan" },
    });

    const refreshed = structuredClone(snapshot);
    refreshed.revision = 8;
    refreshed.phase!.goals[0]!.tasks[0]!.title = "A collaborator's title";
    await view.rerender({ ...props, snapshot: refreshed });
    await fireEvent.click(
      screen.getByRole("button", { name: "Save task title" }),
    );

    expect(onPlan).toHaveBeenNthCalledWith(
      1,
      {
        kind: "task_update",
        task_id: "implement-host",
        title: "A title from the observed plan",
      },
      {
        expected_daemon_instance_id: "daemon-fixture",
        expected_revision: 7,
        expected_phase_id: "phase-fixture",
      },
    );
    expect((screen.getByLabelText("Task title") as HTMLInputElement).value).toBe(
      "A title from the observed plan",
    );

    await view.rerender({
      ...props,
      snapshot: refreshed,
      planningEditorRebindToken: 1,
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Save task title" }),
    );

    expect(onPlan).toHaveBeenNthCalledWith(
      2,
      {
        kind: "task_update",
        task_id: "implement-host",
        title: "A title from the observed plan",
      },
      {
        expected_daemon_instance_id: "daemon-fixture",
        expected_revision: 8,
        expected_phase_id: "phase-fixture",
      },
    );
    expect((screen.getByLabelText("Task title") as HTMLInputElement).value).toBe(
      "A title from the observed plan",
    );
  });

  it("preserves but disables a draft when the refreshed task is immutable", async () => {
    const snapshot = fixture();
    const onPlan = vi.fn().mockResolvedValue(false);
    const props = {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan,
    };
    const view = render(WorkbenchView, props);

    await fireEvent.click(
      screen.getByRole("button", { name: "Edit Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Task title"), {
      target: { value: "A preserved title draft" },
    });

    const completed = structuredClone(snapshot);
    completed.revision = 8;
    completed.phase!.goals[0]!.tasks[0]!.status = "completed";
    await view.rerender({
      ...props,
      snapshot: completed,
      planningEditorRebindToken: 1,
    });

    const editor = screen.getByLabelText("Task title") as HTMLInputElement;
    expect(editor.value).toBe("A preserved title draft");
    expect(editor.readOnly).toBe(true);
    expect(
      screen.getByText(
        "This task changed and can no longer accept this action. Your draft is preserved.",
      ),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Save task title" }),
    ).toHaveProperty("disabled", true);
    await fireEvent.click(
      screen.getByRole("button", { name: "Save task title" }),
    );
    expect(onPlan).not.toHaveBeenCalled();
  });

  it("preserves exact progress and completion text while validating non-whitespace", async () => {
    const onPlan = vi.fn().mockResolvedValue(true);
    render(WorkbenchView, {
      snapshot: fixture(),
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
      onPlan,
    });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Record progress for Implement host",
      }),
    );
    await fireEvent.input(screen.getByLabelText("Progress update"), {
      target: { value: "  indented evidence\n" },
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Record progress" }),
    );

    expect(onPlan).toHaveBeenLastCalledWith(
      {
        kind: "task_log",
        task_id: "implement-host",
        message: "  indented evidence\n",
      },
      {
        expected_daemon_instance_id: "daemon-fixture",
        expected_revision: 7,
        expected_phase_id: "phase-fixture",
      },
    );

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Review completion of Implement host",
      }),
    );
    await fireEvent.input(screen.getByLabelText("Proposed completion outcome"), {
      target: { value: "  exact outcome\n" },
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Review completion" }),
    );
    expect(onPlan).toHaveBeenLastCalledWith(
      {
        kind: "task_complete_review",
        task_id: "implement-host",
        outcome: "  exact outcome\n",
      },
      {
        expected_daemon_instance_id: "daemon-fixture",
        expected_revision: 7,
        expected_phase_id: "phase-fixture",
      },
    );
  });

  it("keeps refresh failure distinct from focus and planning failures", async () => {
    const onRefresh = vi.fn();
    render(WorkbenchView, {
      snapshot: fixture(),
      refreshFailure:
        "Exo returned an unreadable workbench response (HTTP 502, text/html)",
      onFocus: vi.fn(),
      onRefresh,
    });

    expect(screen.getByText("Live refresh paused.")).toBeTruthy();
    expect(screen.queryByText("Lane focus failed.")).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(onRefresh).toHaveBeenCalledOnce();
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

  it("renders the completed phase, next phase, and its prepared lanes between phases", () => {
    const snapshot = fixture();
    snapshot.focused_lane = null;
    snapshot.phase = null;
    snapshot.lanes = snapshot.lanes.map((lane) =>
      lane.id === "lane-next"
        ? {
            ...lane,
            phase_title: "Project trajectory and workspace faces",
            phase_status: "pending",
            focused_here: false,
          }
        : {
            ...lane,
            state: "prepared",
            phase_title: "Interactive lane planning",
            phase_status: "completed",
            focused_here: false,
          },
    );
    snapshot.between_phases_context = {
      epoch_id: "epoch-fixture",
      epoch_title: "exo Everywhere",
      completed_phase: {
        id: "phase-fixture",
        title: "Interactive lane planning",
        completed_at: "2026-08-01T20:00:00Z",
        goal_count: 2,
        completed_goals: 2,
      },
      next_phase: {
        id: "phase-next",
        title: "Project trajectory and workspace faces",
        goal_count: 2,
        rfc_count: 1,
      },
      pending_phases: 2,
    };

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(
      screen.getByRole("heading", { name: "Ready for what comes next" }),
    ).toBeTruthy();
    const trajectorySummary = document.querySelector(".trajectory-intro p");
    expect(trajectorySummary?.textContent).toContain("exo Everywhere");
    expect(trajectorySummary?.textContent).toContain("2 phases remaining");
    expect(screen.getByText("Just finished")).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Interactive lane planning" }),
    ).toBeTruthy();
    expect(screen.getByText("2 of 2 goals complete")).toBeTruthy();
    expect(
      document.querySelector('time[datetime="2026-08-01T20:00:00Z"]'),
    ).toBeTruthy();
    expect(screen.getByText("Up next")).toBeTruthy();
    const nextPhaseHeading = screen.getByRole("heading", {
      name: "Project trajectory and workspace faces",
    });
    expect(nextPhaseHeading).toBeTruthy();
    const nextPhaseSummary = nextPhaseHeading
      .closest("section")
      ?.querySelector(".trajectory-meta");
    expect(nextPhaseSummary?.textContent).toContain("2 goals");
    expect(nextPhaseSummary?.textContent).toContain("1 RFC");
    expect(
      screen.getByRole("heading", { name: "Prepared lanes" }),
    ).toBeTruthy();
    expect(screen.getAllByText("Focus-only lane workspace")).toHaveLength(2);
    expect(screen.getByText("Ready when this phase starts")).toBeTruthy();
    expect(
      screen.queryByRole("heading", { name: "No lane focused here" }),
    ).toBeNull();
  });

  it("states when the next phase does not have a prepared lane yet", () => {
    const snapshot = fixture();
    snapshot.focused_lane = null;
    snapshot.phase = null;
    snapshot.lanes = snapshot.lanes.map((lane) => ({
      ...lane,
      focused_here: false,
    }));
    snapshot.between_phases_context = {
      epoch_id: "epoch-fixture",
      epoch_title: "exo Everywhere",
      completed_phase: null,
      next_phase: {
        id: "phase-unprepared",
        title: "Unprepared phase",
        goal_count: 0,
        rfc_count: 0,
      },
      pending_phases: 1,
    };

    render(WorkbenchView, {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    });

    expect(
      screen.getByText("No lane is prepared for this phase yet."),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Completion history unavailable" }),
    ).toBeTruthy();
  });

  it("disables planning when phase state has no coherent focused lane", () => {
    const snapshot = fixture();
    snapshot.focused_lane = null;
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
      screen.getByRole("button", {
        name: "Add task to Establish local host and launch",
      }),
    ).toHaveProperty("disabled", true);
    expect(
      screen.getByRole("button", { name: "Edit Implement host" }),
    ).toHaveProperty("disabled", true);
  });

  it("keeps a foreign-owned focused phase and its open draft read-only", async () => {
    const snapshot = fixture();
    const props = {
      snapshot,
      onFocus: vi.fn(),
      onRefresh: vi.fn(),
    };
    const view = render(WorkbenchView, props);
    await fireEvent.click(
      screen.getByRole("button", { name: "Edit Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Task title"), {
      target: { value: "Preserved while ownership changes" },
    });

    const foreignOwned = structuredClone(snapshot);
    foreignOwned.phase!.planning_available = false;
    await view.rerender({
      ...props,
      snapshot: foreignOwned,
    });

    expect(
      screen.getByText(
        "Planning is read-only here because this phase is owned by another workspace.",
      ),
    ).toBeTruthy();
    const editor = screen.getByLabelText("Task title") as HTMLInputElement;
    expect(editor.value).toBe("Preserved while ownership changes");
    expect(editor.readOnly).toBe(true);
    expect(
      screen.getByRole("button", {
        name: "Add task to Establish local host and launch",
      }),
    ).toHaveProperty("disabled", true);
    expect(
      screen.getByRole("button", { name: "Edit Implement host" }),
    ).toHaveProperty("disabled", true);
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
      "This lane’s phase is not active",
    );
    await fireEvent.click(completedLane);
    expect(onFocus).not.toHaveBeenCalled();
  });

  it("keeps pending-phase lanes visible without making them focusable", async () => {
    const snapshot = fixture();
    snapshot.lanes.push({
      id: "lane-future",
      title: "Future lane",
      state: "prepared",
      phase_id: "phase-future",
      phase_title: "Future phase",
      phase_status: "pending",
      focused_here: false,
    });
    const onFocus = vi.fn();

    render(WorkbenchView, {
      snapshot,
      onFocus,
      onRefresh: vi.fn(),
    });

    const futureLane = screen.getByRole("button", {
      name: "Future lane, phase pending",
    });
    expect(futureLane).toHaveProperty("disabled", true);
    expect(futureLane.getAttribute("title")).toBe(
      "This lane’s phase is not active",
    );
    await fireEvent.click(futureLane);
    expect(onFocus).not.toHaveBeenCalled();
  });
});
