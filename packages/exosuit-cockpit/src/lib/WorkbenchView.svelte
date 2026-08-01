<script lang="ts">
  import {
    Activity,
    AlertTriangle,
    ArrowDown,
    ArrowUp,
    Check,
    CheckCheck,
    CheckCircle2,
    Circle,
    CircleDashed,
    CircleDot,
    CirclePlay,
    ClipboardCheck,
    Compass,
    GitBranch,
    GitCommitHorizontal,
    Info,
    Layers3,
    ListTodo,
    LoaderCircle,
    MessageSquareText,
    PanelLeft,
    Pencil,
    Plus,
    RefreshCw,
    Route,
    Send,
    Target,
    Wifi,
    WifiOff,
    X,
    XCircle,
  } from "@lucide/svelte";
  import { onMount } from "svelte";

  import {
    workbenchPlanningBinding,
    type WorkbenchDiagnostic,
    type WorkbenchGoal,
    type WorkbenchLaneSummary,
    type WorkbenchPlanningBinding,
    type WorkbenchPlanningOperation,
    type WorkbenchSnapshot,
    type WorkbenchTask,
    type WorkbenchTaskCompletionReview,
  } from "./workbench";

  interface Props {
    snapshot: WorkbenchSnapshot;
    refreshing?: boolean;
    streamConnected?: boolean;
    sessionRecovery?: "connected" | "reconnecting" | "needs_launch";
    sessionRecoveryMessage?: string | null;
    pendingLaneId?: string | null;
    focusFailure?: string | null;
    refreshFailure?: string | null;
    planningFailure?: string | null;
    planningNotice?: string | null;
    planningSuccess?: {
      requestId: string;
      operation: WorkbenchPlanningOperation;
    } | null;
    planningEditorRebindToken?: number;
    pendingPlanningKind?: WorkbenchPlanningOperation["kind"] | null;
    completionReview?: WorkbenchTaskCompletionReview | null;
    onFocus: (laneId: string) => void;
    onRetryFocus?: (() => void) | null;
    onRetryPlanning?: (() => void) | null;
    onRetrySession?: (() => void) | null;
    onRefresh: () => void;
    onPlan?: (
      operation: WorkbenchPlanningOperation,
      binding?: WorkbenchPlanningBinding,
    ) => Promise<boolean>;
    onApproveCompletion?: () => Promise<boolean>;
    onDismissCompletionReview?: () => void;
  }

  let {
    snapshot,
    refreshing = false,
    streamConnected = false,
    sessionRecovery = "connected",
    sessionRecoveryMessage = null,
    pendingLaneId = null,
    focusFailure = null,
    refreshFailure = null,
    planningFailure = null,
    planningNotice = null,
    planningSuccess = null,
    planningEditorRebindToken = 0,
    pendingPlanningKind = null,
    completionReview = null,
    onFocus,
    onRetryFocus = null,
    onRetryPlanning = null,
    onRetrySession = null,
    onRefresh,
    onPlan = async () => false,
    onApproveCompletion = async () => false,
    onDismissCompletionReview = () => {},
  }: Props = $props();

  type PlanningEditor =
    | { kind: "add"; goalId: string }
    | { kind: "edit"; taskId: string }
    | { kind: "log"; taskId: string }
    | { kind: "review"; taskId: string };

  const TITLE_MAX_BYTES = 512;
  const MESSAGE_MAX_BYTES = 16 * 1024;
  const utf8Encoder = new TextEncoder();

  let laneRail: HTMLElement | undefined = $state();
  let completionReviewCard: HTMLElement | undefined = $state();
  let compactNavigation = $state(false);
  let planningEditor = $state<PlanningEditor | null>(null);
  let planningEditorBinding = $state<WorkbenchPlanningBinding | null>(null);
  let planningValue = $state("");
  let handledPlanningSuccessId: string | null = null;
  let handledPlanningEditorRebindToken = 0;

  let agentNextStep = $derived(snapshot.steering.next_actions[0] ?? null);
  let hasCoordination = $derived(
    agentNextStep !== null || snapshot.diagnostics.length > 0,
  );
  let planningBusy = $derived(
    pendingPlanningKind !== null || onRetryPlanning !== null,
  );
  let interactionDisabled = $derived(sessionRecovery !== "connected");
  let planningContextAvailable = $derived(
    workbenchPlanningBinding(snapshot) !== null,
  );
  let planningEditorAvailable = $derived.by(
    () =>
      planningContextAvailable &&
      editorAllowsPlanning(planningEditor, snapshot),
  );
  let planningDisabled = $derived(
    planningBusy || interactionDisabled || !planningContextAvailable,
  );
  let planningValueForOperation = $derived.by(() =>
    planningEditor?.kind === "add" || planningEditor?.kind === "edit"
      ? planningValue.trim()
      : planningValue,
  );
  let planningValueByteLimit = $derived(
    planningEditor?.kind === "add" || planningEditor?.kind === "edit"
      ? TITLE_MAX_BYTES
      : MESSAGE_MAX_BYTES,
  );
  let planningValueByteLength = $derived(
    utf8Encoder.encode(planningValueForOperation).byteLength,
  );
  let planningValueTooLarge = $derived(
    planningValueByteLength > planningValueByteLimit,
  );
  let planningSubmitDisabled = $derived(
    planningDisabled ||
      !planningEditorAvailable ||
      planningValueForOperation.trim().length === 0 ||
      planningValueTooLarge,
  );
  let connectionPresentation = $derived.by(() => {
    if (sessionRecovery === "reconnecting") {
      return {
        label: "Reconnecting",
        title: "Reconnecting to the Exo workbench host",
        kind: "reconnecting" as const,
      };
    }
    if (sessionRecovery === "needs_launch") {
      return {
        label: "Paused",
        title: "This workbench session needs a current launch",
        kind: "paused" as const,
      };
    }
    if (streamConnected) {
      return {
        label: "Live",
        title: "Live updates connected",
        kind: "connected" as const,
      };
    }
    return {
      label: "Polling",
      title: "Live updates reconnecting; polling remains active",
      kind: "polling" as const,
    };
  });
  let reviewedTaskTitle = $derived.by(() => {
    if (!completionReview || !snapshot.phase) {
      return null;
    }
    for (const goal of snapshot.phase.goals) {
      const task = goal.tasks.find(
        (candidate) => candidate.id === completionReview.task_id,
      );
      if (task) {
        return task.title;
      }
    }
    return completionReview.task_id;
  });

  onMount(() => {
    if (!window.matchMedia) {
      return;
    }
    const query = window.matchMedia("(max-width: 760px)");
    const updateNavigation = () => {
      compactNavigation = query.matches;
      if (!query.matches && laneRail?.matches(":popover-open")) {
        laneRail.hidePopover();
      }
    };
    updateNavigation();
    query.addEventListener("change", updateNavigation);
    return () => query.removeEventListener("change", updateNavigation);
  });

  $effect(() => {
    if (
      planningSuccess &&
      planningSuccess.requestId !== handledPlanningSuccessId
    ) {
      handledPlanningSuccessId = planningSuccess.requestId;
      if (
        planningEditor &&
        editorMatchesOperation(planningEditor, planningSuccess.operation)
      ) {
        closeEditor();
      }
    }
  });

  $effect(() => {
    if (
      planningEditor &&
      !planningBusy &&
      planningEditorRebindToken > handledPlanningEditorRebindToken
    ) {
      handledPlanningEditorRebindToken = planningEditorRebindToken;
      const binding = workbenchPlanningBinding(snapshot);
      if (binding && planningEditorAvailable) {
        planningEditorBinding = binding;
      }
    }
  });

  $effect(() => {
    if (
      completionReview?.review_id &&
      typeof completionReviewCard?.scrollIntoView === "function"
    ) {
      completionReviewCard.scrollIntoView({ block: "start" });
    }
  });

  const shortHead = (head: string | null): string =>
    head ? head.slice(0, 8) : "unborn";

  const displayStatus = (status: string): string =>
    status.replaceAll("-", " ");

  type StatusTone = "complete" | "active" | "pending" | "terminal";

  const statusTone = (status: string): StatusTone => {
    const normalized = status.toLowerCase();
    if (
      ["completed", "complete", "done", "closed", "green"].includes(normalized)
    ) {
      return "complete";
    }
    if (["in-progress", "executing", "active"].includes(normalized)) {
      return "active";
    }
    if (normalized === "pending" || normalized === "prepared") {
      return "pending";
    }
    return "terminal";
  };

  const statusLabel = (status: string): string => {
    const label = displayStatus(status);
    return label.charAt(0).toUpperCase() + label.slice(1);
  };

  const goalAllowsPlanning = (goal: WorkbenchGoal): boolean =>
    ["pending", "in-progress", "active"].includes(goal.status);

  const taskAllowsPlanning = (task: WorkbenchTask): boolean =>
    ["pending", "in-progress"].includes(task.status);

  const observedTime = (value: string): string => {
    const date = new Date(value);
    return Number.isNaN(date.valueOf())
      ? value
      : new Intl.DateTimeFormat(undefined, {
          hour: "numeric",
          minute: "2-digit",
          second: "2-digit",
        }).format(date);
  };

  const diagnosticIcon = (diagnostic: WorkbenchDiagnostic) =>
    diagnostic.severity;

  const lanePhaseActive = (lane: WorkbenchLaneSummary): boolean =>
    lane.phase_status === "in-progress";

  const laneTitle = (lane: WorkbenchLaneSummary): string => {
    if (pendingLaneId === lane.id) {
      return `Focusing ${lane.title}`;
    }
    if (lane.focused_here) {
      return `${lane.title}, focused`;
    }
    if (!lanePhaseActive(lane)) {
      return `${lane.title}, phase ${displayStatus(lane.phase_status)}`;
    }
    return `Focus ${lane.title}`;
  };

  const goalProgressLabel = (goal: WorkbenchGoal): string => {
    const tone = statusTone(goal.status);
    if (tone === "complete") {
      return "Complete";
    }
    if (tone === "terminal") {
      return statusLabel(goal.status);
    }
    const completed = goal.tasks.filter(
      (task) => statusTone(task.status) === "complete",
    ).length;
    const started = goal.tasks.some((task) =>
      ["active", "complete"].includes(statusTone(task.status)),
    );
    if (
      tone === "pending" &&
      goal.tasks.length > 0 &&
      completed === goal.tasks.length
    ) {
      return "Tasks complete · Goal pending";
    }
    if (tone === "active" || started) {
      return goal.tasks.length > 0
        ? `${completed} of ${goal.tasks.length} tasks complete`
        : "In progress";
    }
    return "Not started";
  };

  const goalProgressTone = (goal: WorkbenchGoal): StatusTone => {
    const tone = statusTone(goal.status);
    return tone === "pending" &&
      goal.tasks.some((task) =>
        ["active", "complete"].includes(statusTone(task.status)),
      )
      ? "active"
      : tone;
  };

  const taskIndex = (goal: WorkbenchGoal, task: WorkbenchTask): number =>
    goal.tasks.findIndex((candidate) => candidate.id === task.id);
  const progressSummary = (count: number, truncated = false): string =>
    `${count} ${truncated ? "recent " : ""}progress ${
      count === 1 ? "update" : "updates"
    }`;

  const planningLabel = (
    kind: WorkbenchPlanningOperation["kind"],
  ): string => {
    switch (kind) {
      case "task_add":
        return "Adding task";
      case "task_update":
        return "Updating task";
      case "task_reorder":
        return "Reordering task";
      case "task_start":
        return "Marking task active";
      case "task_log":
        return "Recording progress";
      case "task_complete_review":
        return "Preparing review";
      case "task_complete_approve":
        return "Recording approval";
    }
  };

  function selectLane(laneId: string): void {
    if (laneRail?.matches(":popover-open")) {
      laneRail.hidePopover();
    }
    onFocus(laneId);
  }

  function toggleNavigationFallback(event: MouseEvent): void {
    const button = event.currentTarget as HTMLButtonElement;
    if (!("commandForElement" in button)) {
      laneRail?.togglePopover();
    }
  }

  function openEditor(editor: PlanningEditor, initialValue = ""): void {
    const binding = workbenchPlanningBinding(snapshot);
    if (!binding) {
      return;
    }
    planningEditor = editor;
    planningEditorBinding = binding;
    planningValue = initialValue;
  }

  function closeEditor(): void {
    planningEditor = null;
    planningEditorBinding = null;
    planningValue = "";
  }

  function editorMatchesOperation(
    editor: PlanningEditor,
    operation: WorkbenchPlanningOperation,
  ): boolean {
    switch (editor.kind) {
      case "add":
        return (
          operation.kind === "task_add" &&
          operation.goal_id === editor.goalId
        );
      case "edit":
        return (
          operation.kind === "task_update" &&
          operation.task_id === editor.taskId
        );
      case "log":
        return (
          operation.kind === "task_log" &&
          operation.task_id === editor.taskId
        );
      case "review":
        return (
          operation.kind === "task_complete_review" &&
          operation.task_id === editor.taskId
        );
    }
  }

  function editorAllowsPlanning(
    editor: PlanningEditor | null,
    currentSnapshot: WorkbenchSnapshot,
  ): boolean {
    if (!editor || !currentSnapshot.phase) {
      return editor === null;
    }
    if (editor.kind === "add") {
      const goal = currentSnapshot.phase.goals.find(
        (candidate) => candidate.id === editor.goalId,
      );
      return goal !== undefined && goalAllowsPlanning(goal);
    }
    const task = currentSnapshot.phase.goals
      .flatMap((goal) => goal.tasks)
      .find((candidate) => candidate.id === editor.taskId);
    if (!task) {
      return false;
    }
    return editor.kind === "edit"
      ? taskAllowsPlanning(task)
      : task.status === "in-progress";
  }

  function reviseCompletionReview(): void {
    if (!completionReview) {
      return;
    }
    openEditor(
      { kind: "review", taskId: completionReview.task_id },
      completionReview.proposed_outcome,
    );
    onDismissCompletionReview();
  }

  async function submitEditor(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (
      !planningEditor ||
      !planningEditorBinding ||
      planningDisabled ||
      !planningEditorAvailable
    ) {
      return;
    }
    const value = planningValueForOperation;
    if (!value.trim() || planningValueTooLarge) {
      return;
    }
    let operation: WorkbenchPlanningOperation;
    switch (planningEditor.kind) {
      case "add":
        operation = {
          kind: "task_add",
          goal_id: planningEditor.goalId,
          title: value,
        };
        break;
      case "edit":
        operation = {
          kind: "task_update",
          task_id: planningEditor.taskId,
          title: value,
        };
        break;
      case "log":
        operation = {
          kind: "task_log",
          task_id: planningEditor.taskId,
          message: value,
        };
        break;
      case "review":
        operation = {
          kind: "task_complete_review",
          task_id: planningEditor.taskId,
          outcome: value,
        };
        break;
    }
    if (await onPlan(operation, planningEditorBinding)) {
      closeEditor();
    }
  }

  async function applyPlanning(
    operation: WorkbenchPlanningOperation,
  ): Promise<void> {
    if (!planningDisabled) {
      await onPlan(operation);
    }
  }
</script>

<div class="workbench">
  <header class="topbar">
    <div class="brand">
      <span class="brand-mark" aria-hidden="true"><Route size={19} /></span>
      <div>
        <span class="brand-name">Exo</span>
        <span class="brand-context">Lane workbench</span>
      </div>
    </div>

    <div class="workspace-identity" aria-label="Workspace identity">
      <strong>{snapshot.workspace.label}</strong>
      <span class="identity-item">
        <GitBranch size={14} aria-hidden="true" />
        {snapshot.workspace.branch ?? "detached"}
      </span>
      <span class="identity-item mono">
        <GitCommitHorizontal size={14} aria-hidden="true" />
        {shortHead(snapshot.workspace.head)}
      </span>
      {#if snapshot.workspace.dirty}
        <span class="dirty-indicator">
          <CircleDot size={13} aria-hidden="true" />
          Modified
        </span>
      {/if}
    </div>

    <div class="topbar-actions">
      <span
        class:connected={connectionPresentation.kind === "connected"}
        class:reconnecting={connectionPresentation.kind === "reconnecting"}
        class:paused={connectionPresentation.kind === "paused"}
        class="connection"
        title={connectionPresentation.title}
      >
        {#if connectionPresentation.kind === "connected"}
          <Wifi size={14} aria-hidden="true" />
        {:else if connectionPresentation.kind === "reconnecting"}
          <LoaderCircle class="spin" size={14} aria-hidden="true" />
        {:else}
          <WifiOff size={14} aria-hidden="true" />
        {/if}
        {connectionPresentation.label}
      </span>
      <button
        class="icon-button lane-invoker"
        type="button"
        title="Open project lanes"
        aria-label="Open project lanes"
        commandfor="lane-navigation"
        command="toggle-popover"
        onclick={toggleNavigationFallback}
      >
        <PanelLeft size={17} aria-hidden="true" />
      </button>
      <button
        class="icon-button"
        type="button"
        title="Refresh workbench"
        aria-label="Refresh workbench"
        disabled={refreshing || sessionRecovery === "reconnecting"}
        onclick={onRefresh}
      >
        <RefreshCw class={refreshing ? "spin" : undefined} size={17} aria-hidden="true" />
      </button>
    </div>
  </header>

  {#if sessionRecovery === "reconnecting"}
    <div class="recovery-banner" role="status">
      <LoaderCircle class="spin" size={17} aria-hidden="true" />
      <span><strong>Reconnecting to Exo.</strong> The current cockpit remains visible while changes are paused.</span>
    </div>
  {:else if sessionRecovery === "needs_launch"}
    <div class="recovery-banner needs-launch" role="alert">
      <Route size={17} aria-hidden="true" />
      <span>
        <strong>This session could not be restored.</strong>
        {sessionRecoveryMessage ?? "Open a current Exo workbench link for this workspace."}
      </span>
      {#if onRetrySession}
        <button type="button" onclick={onRetrySession}>Try again</button>
      {/if}
    </div>
  {/if}

  {#if focusFailure}
    <div class="failure-banner" role="alert">
      <AlertTriangle size={18} aria-hidden="true" />
      <span><strong>Lane focus failed.</strong> {focusFailure}</span>
      {#if onRetryFocus}
        <button type="button" onclick={onRetryFocus}>Retry</button>
      {/if}
    </div>
  {/if}

  {#if planningFailure}
    <div class="failure-banner" role="alert">
      <AlertTriangle size={18} aria-hidden="true" />
      <span><strong>Planning change not applied.</strong> {planningFailure}</span>
      {#if onRetryPlanning}
        <button type="button" onclick={onRetryPlanning}>Retry same request</button>
      {/if}
    </div>
  {/if}

  {#if planningNotice}
    <div class="planning-notice" role="status">
      <CircleDot size={18} aria-hidden="true" />
      <span><strong>Ready for agent handoff.</strong> {planningNotice}</span>
    </div>
  {/if}

  {#if refreshFailure}
    <div class="failure-banner refresh-failure" role="alert">
      <WifiOff size={18} aria-hidden="true" />
      <span><strong>Live refresh paused.</strong> {refreshFailure}</span>
      <button type="button" onclick={onRefresh}>Refresh</button>
    </div>
  {/if}

  <div class:has-coordination={hasCoordination} class="workspace-grid">
    <aside
      bind:this={laneRail}
      class="lane-rail"
      id="lane-navigation"
      aria-label="Project lanes"
      popover={compactNavigation ? "auto" : undefined}
    >
      <div class="rail-heading">
        <div>
          <span class="section-kicker">Project</span>
          <h2>Lanes</h2>
        </div>
        <div class="rail-heading-actions">
          <span class="count" aria-label={`${snapshot.lanes.length} lanes`}>
            {snapshot.lanes.length}
          </span>
          <button
            class="icon-button rail-close"
            type="button"
            title="Close project lanes"
            aria-label="Close project lanes"
            commandfor="lane-navigation"
            command="hide-popover"
          >
            <X size={16} aria-hidden="true" />
          </button>
        </div>
      </div>

      {#if snapshot.lanes.length === 0}
        <div class="rail-empty">
          <Layers3 size={20} aria-hidden="true" />
          <span>No lanes yet</span>
        </div>
      {:else}
        <nav class="lane-list" aria-label="Available lanes">
          {#each snapshot.lanes as lane (lane.id)}
            <button
              class:focused={lane.focused_here}
              class="lane-row"
              type="button"
              aria-current={lane.focused_here ? "page" : undefined}
              aria-label={laneTitle(lane)}
              title={!lanePhaseActive(lane)
                ? "This lane’s phase is not active"
                : undefined}
              disabled={interactionDisabled ||
                pendingLaneId !== null ||
                lane.focused_here ||
                !lanePhaseActive(lane)}
              onclick={() => selectLane(lane.id)}
            >
              <span class="lane-state" aria-hidden="true">
                {#if pendingLaneId === lane.id}
                  <LoaderCircle class="spin" size={17} />
                {:else if lane.focused_here}
                  <Target size={17} />
                {:else if statusTone(lane.phase_status) === "complete"}
                  <CheckCircle2 size={17} />
                {:else if !lanePhaseActive(lane)}
                  <CircleDashed size={17} />
                {:else if lane.state === "executing"}
                  <CirclePlay size={17} />
                {:else}
                  <CircleDashed size={17} />
                {/if}
              </span>
              <span class="lane-copy">
                <strong>{lane.title}</strong>
                <span>{lane.phase_title}</span>
              </span>
              <span
                class={`state-dot ${lane.state}`}
                title={lane.state}
                aria-hidden="true"
              ></span>
            </button>
          {/each}
        </nav>
      {/if}
    </aside>

    <main class="main-surface">
      {#if snapshot.focused_lane}
        <section class="intent-band" aria-labelledby="lane-title">
          <div class="intent-heading">
            <span class="section-kicker">Focused lane</span>
            <span class={`status-label ${snapshot.focused_lane.state}`}>
              {snapshot.focused_lane.state}
            </span>
          </div>
          <h1 id="lane-title">{snapshot.focused_lane.title}</h1>
          <p class="lane-intent">{snapshot.focused_lane.intent}</p>
          <div class="lane-context">
            <span><Activity size={15} aria-hidden="true" />{snapshot.focused_lane.phase_title}</span>
            <span><GitBranch size={15} aria-hidden="true" />{snapshot.workspace.branch ?? "Detached HEAD"}</span>
          </div>
        </section>
      {:else}
        <section class="no-focus" aria-labelledby="no-focus-title">
          <Target size={28} aria-hidden="true" />
          <h1 id="no-focus-title">No lane focused here</h1>
          <p>Select an existing lane to establish this workspace’s current stream.</p>
        </section>
      {/if}

      {#if snapshot.phase}
        <section class="plan-section" aria-labelledby="phase-title">
          <div class="section-heading">
            <div>
              <span class="section-kicker">Execution context</span>
              <h2 id="phase-title">{snapshot.phase.title}</h2>
            </div>
            <span class={`status-label ${statusTone(snapshot.phase.status)}`}>
              {displayStatus(snapshot.phase.status)}
            </span>
          </div>

          {#if !snapshot.phase.planning_available}
            <div class="planning-read-only" role="status">
              <Info size={16} aria-hidden="true" />
              Planning is read-only here because this phase is owned by another workspace.
            </div>
          {/if}

          {#if completionReview}
            <section
              class="completion-review"
              aria-labelledby="completion-review-title"
              bind:this={completionReviewCard}
            >
              <div class="review-heading">
                <span class="review-mark" aria-hidden="true">
                  <ClipboardCheck size={19} />
                </span>
                <div>
                  <span class="section-kicker">Task completion review</span>
                  <h3 id="completion-review-title">{reviewedTaskTitle}</h3>
                </div>
                <button
                  class="review-dismiss"
                  type="button"
                  title="Keep working"
                  aria-label="Keep working"
                  disabled={planningBusy || interactionDisabled}
                  onclick={onDismissCompletionReview}
                >
                  <X size={17} aria-hidden="true" />
                </button>
              </div>
              <p class="review-rationale">
                {completionReview.readiness_rationale}
              </p>
              <div class="review-outcome">
                <span>Outcome to record</span>
                <p class="review-outcome-text">{completionReview.proposed_outcome}</p>
              </div>
              <div class="review-evidence">
                {#if completionReview.approval_evidence_present}
                  <CheckCircle2 size={15} aria-hidden="true" />
                  Existing approval evidence is present.
                {:else}
                  <Info size={15} aria-hidden="true" />
                  Approving records this exact outcome.
                {/if}
              </div>
              <div class="review-actions">
                <button
                  class="secondary-button"
                  type="button"
                  disabled={planningDisabled}
                  onclick={reviseCompletionReview}
                >
                  <Pencil size={16} aria-hidden="true" />
                  Revise outcome
                </button>
                <button
                  class="primary-button"
                  type="button"
                  disabled={planningDisabled}
                  onclick={() => void onApproveCompletion()}
                >
                  {#if pendingPlanningKind === "task_complete_approve"}
                    <LoaderCircle class="spin" size={16} aria-hidden="true" />
                    Recording approval
                  {:else}
                    <CheckCheck size={16} aria-hidden="true" />
                    Approve exact outcome
                  {/if}
                </button>
              </div>
            </section>
          {/if}

          <div class="goal-list">
            {#each snapshot.phase.goals as goal (goal.id)}
              <article class="goal">
                <div class="goal-heading">
                  <span class={`status-icon ${goalProgressTone(goal)}`} aria-hidden="true">
                    {#if goalProgressTone(goal) === "complete"}
                      <CheckCircle2 size={18} />
                    {:else if goalProgressTone(goal) === "active"}
                      <CircleDot size={18} />
                    {:else if goalProgressTone(goal) === "terminal"}
                      <XCircle size={18} />
                    {:else}
                      <Circle size={18} />
                    {/if}
                  </span>
                  <div class="goal-copy">
                    <h3>{goal.title}</h3>
                    <span class="goal-progress">{goalProgressLabel(goal)}</span>
                  </div>
                  {#if goalAllowsPlanning(goal)}
                    <button
                      class="planning-icon-button"
                      type="button"
                      title="Add task"
                      aria-label={`Add task to ${goal.title}`}
                      disabled={planningDisabled}
                      onclick={() => openEditor({ kind: "add", goalId: goal.id })}
                    >
                      <Plus size={16} aria-hidden="true" />
                    </button>
                  {/if}
                </div>

                {#if planningEditor?.kind === "add" && planningEditor.goalId === goal.id}
                  <form class="planning-editor" onsubmit={submitEditor}>
                    <label for={`add-task-${goal.id}`}>New task</label>
                    <div class="editor-control">
                      <input
                        id={`add-task-${goal.id}`}
                        bind:value={planningValue}
                        maxlength="512"
                        aria-invalid={planningValueTooLarge}
                        aria-describedby={planningValueTooLarge
                          ? `add-task-limit-${goal.id}`
                          : undefined}
                        placeholder="Describe the next bounded task"
                        required
                      />
                      <button
                        class="primary-icon-button"
                        type="submit"
                        title="Add task"
                        aria-label="Add task"
                        disabled={planningSubmitDisabled}
                      >
                        {#if pendingPlanningKind === "task_add"}
                          <LoaderCircle class="spin" size={16} aria-hidden="true" />
                        {:else}
                          <Send size={16} aria-hidden="true" />
                        {/if}
                      </button>
                      <button
                        class="planning-icon-button"
                        type="button"
                        title="Cancel"
                        aria-label="Cancel adding task"
                        disabled={planningBusy || interactionDisabled}
                        onclick={closeEditor}
                      >
                        <X size={16} aria-hidden="true" />
                      </button>
                    </div>
                    {#if planningValueTooLarge}
                      <p
                        class="editor-validation"
                        id={`add-task-limit-${goal.id}`}
                        role="alert"
                      >
                        Text is too long ({planningValueByteLength} of
                        {planningValueByteLimit} bytes).
                      </p>
                    {/if}
                  </form>
                {/if}

                {#if goal.tasks.length > 0}
                  <ul class="task-list">
                    {#each goal.tasks as task, index (task.id)}
                      {@const progress = task.progress ?? []}
                      <li>
                        <div class="task-row">
                          <span class={`task-check ${statusTone(task.status)}`} aria-hidden="true">
                            {#if statusTone(task.status) === "complete"}
                              <Check size={13} />
                            {:else if statusTone(task.status) === "active"}
                              <CircleDot size={13} />
                            {:else if statusTone(task.status) === "terminal"}
                              <X size={13} />
                            {:else}
                              <Circle size={13} />
                            {/if}
                          </span>
                          <span class="task-copy">
                            <span class="task-title">{task.title}</span>
                            {#if ["pending", "complete"].includes(statusTone(task.status))}
                              <span class="sr-only">{statusLabel(task.status)}</span>
                            {:else if statusTone(task.status) === "active"}
                              <span class="task-active-label">Active</span>
                            {:else if statusTone(task.status) === "terminal"}
                              <span class="task-terminal-label">{statusLabel(task.status)}</span>
                            {/if}
                          </span>
                          {#if taskAllowsPlanning(task)}
                            <div class="task-actions" aria-label={`Actions for ${task.title}`}>
                              {#if task.status === "pending"}
                                <button
                                  class="planning-icon-button"
                                  type="button"
                                  title="Mark active in Exo"
                                  aria-label={`Mark ${task.title} active in Exo`}
                                  disabled={planningDisabled}
                                  onclick={() =>
                                    void applyPlanning({
                                      kind: "task_start",
                                      task_id: task.id,
                                    })}
                                >
                                  <CircleDot size={15} aria-hidden="true" />
                                </button>
                              {/if}
                              <button
                                class="planning-icon-button"
                                type="button"
                                title="Edit title"
                                aria-label={`Edit ${task.title}`}
                                disabled={planningDisabled}
                                onclick={() =>
                                  openEditor(
                                    { kind: "edit", taskId: task.id },
                                    task.title,
                                  )}
                              >
                                <Pencil size={14} aria-hidden="true" />
                              </button>
                              <button
                                class="planning-icon-button"
                                type="button"
                                title="Move up"
                                aria-label={`Move ${task.title} up`}
                                disabled={planningDisabled || index === 0}
                                onclick={() =>
                                  void applyPlanning({
                                    kind: "task_reorder",
                                    task_id: task.id,
                                    position: taskIndex(goal, task) - 1,
                                  })}
                              >
                                <ArrowUp size={14} aria-hidden="true" />
                              </button>
                              <button
                                class="planning-icon-button"
                                type="button"
                                title="Move down"
                                aria-label={`Move ${task.title} down`}
                                disabled={planningDisabled ||
                                  index === goal.tasks.length - 1}
                                onclick={() =>
                                  void applyPlanning({
                                    kind: "task_reorder",
                                    task_id: task.id,
                                    position: taskIndex(goal, task) + 1,
                                  })}
                              >
                                <ArrowDown size={14} aria-hidden="true" />
                              </button>
                              {#if task.status === "in-progress"}
                                <button
                                  class="planning-icon-button"
                                  type="button"
                                  title="Record progress"
                                  aria-label={`Record progress for ${task.title}`}
                                  disabled={planningDisabled}
                                  onclick={() =>
                                    openEditor({ kind: "log", taskId: task.id })}
                                >
                                  <MessageSquareText size={15} aria-hidden="true" />
                                </button>
                                <button
                                  class="planning-icon-button"
                                  type="button"
                                  title="Review completion"
                                  aria-label={`Review completion of ${task.title}`}
                                  disabled={planningDisabled}
                                  onclick={() =>
                                    openEditor({ kind: "review", taskId: task.id })}
                                >
                                  <ClipboardCheck size={15} aria-hidden="true" />
                                </button>
                              {/if}
                            </div>
                          {/if}
                        </div>

                        {#if progress.length > 0}
                          <details class="task-progress">
                            <summary>
                              {progressSummary(progress.length, task.progress_truncated)}
                            </summary>
                            <ol>
                              {#each progress as entry}
                                <li>
                                  <p>{entry.message}</p>
                                  <time datetime={entry.created_at}>
                                    {observedTime(entry.created_at)}
                                  </time>
                                </li>
                              {/each}
                            </ol>
                            {#if task.progress_truncated}
                              <p class="task-progress-note">
                                Earlier or longer updates remain available in Exo.
                              </p>
                            {/if}
                          </details>
                        {/if}

                        {#if planningEditor?.kind === "edit" && planningEditor.taskId === task.id}
                          <form class="planning-editor task-editor" onsubmit={submitEditor}>
                            <label for={`edit-task-${task.id}`}>Task title</label>
                            <div class="editor-control">
                              <input
                                id={`edit-task-${task.id}`}
                                bind:value={planningValue}
                                maxlength="512"
                                readonly={!planningEditorAvailable}
                                aria-invalid={planningValueTooLarge}
                                aria-describedby={planningValueTooLarge
                                  ? `edit-task-limit-${task.id}`
                                  : undefined}
                                required
                              />
                              <button
                                class="primary-icon-button"
                                type="submit"
                                title="Save title"
                                aria-label="Save task title"
                                disabled={planningSubmitDisabled}
                              >
                                {#if pendingPlanningKind === "task_update"}
                                  <LoaderCircle class="spin" size={16} aria-hidden="true" />
                                {:else}
                                  <Check size={16} aria-hidden="true" />
                                {/if}
                              </button>
                              <button
                                class="planning-icon-button"
                                type="button"
                                title="Cancel"
                                aria-label="Cancel editing task"
                                disabled={planningBusy || interactionDisabled}
                                onclick={closeEditor}
                              >
                                <X size={16} aria-hidden="true" />
                              </button>
                            </div>
                            {#if planningValueTooLarge}
                              <p
                                class="editor-validation"
                                id={`edit-task-limit-${task.id}`}
                                role="alert"
                              >
                                Text is too long ({planningValueByteLength} of
                                {planningValueByteLimit} bytes).
                              </p>
                            {/if}
                            {#if !planningEditorAvailable}
                              <p class="editor-availability" role="status">
                                This task changed and can no longer accept this action. Your draft is preserved.
                              </p>
                            {/if}
                          </form>
                        {:else if planningEditor?.kind === "log" && planningEditor.taskId === task.id}
                          <form class="planning-editor task-editor" onsubmit={submitEditor}>
                            <label for={`log-task-${task.id}`}>Progress update</label>
                            <textarea
                              id={`log-task-${task.id}`}
                              bind:value={planningValue}
                              maxlength="16384"
                              readonly={!planningEditorAvailable}
                              aria-invalid={planningValueTooLarge}
                              aria-describedby={planningValueTooLarge
                                ? `log-task-limit-${task.id}`
                                : undefined}
                              rows="3"
                              placeholder="Record evidence, a decision, or the next concrete boundary"
                              required
                            ></textarea>
                            {#if planningValueTooLarge}
                              <p
                                class="editor-validation"
                                id={`log-task-limit-${task.id}`}
                                role="alert"
                              >
                                Text is too long ({planningValueByteLength} of
                                {planningValueByteLimit} bytes).
                              </p>
                            {/if}
                            {#if !planningEditorAvailable}
                              <p class="editor-availability" role="status">
                                This task changed and can no longer accept this action. Your draft is preserved.
                              </p>
                            {/if}
                            <div class="editor-actions">
                              <button
                                class="secondary-button"
                                type="button"
                                disabled={planningBusy || interactionDisabled}
                                onclick={closeEditor}
                              >
                                Cancel
                              </button>
                              <button
                                class="primary-button"
                                type="submit"
                                disabled={planningSubmitDisabled}
                              >
                                {#if pendingPlanningKind === "task_log"}
                                  <LoaderCircle class="spin" size={16} aria-hidden="true" />
                                  Recording
                                {:else}
                                  <MessageSquareText size={16} aria-hidden="true" />
                                  Record progress
                                {/if}
                              </button>
                            </div>
                          </form>
                        {:else if planningEditor?.kind === "review" && planningEditor.taskId === task.id}
                          <form class="planning-editor task-editor" onsubmit={submitEditor}>
                            <label for={`review-task-${task.id}`}>Proposed completion outcome</label>
                            <textarea
                              id={`review-task-${task.id}`}
                              bind:value={planningValue}
                              maxlength="16384"
                              readonly={!planningEditorAvailable}
                              aria-invalid={planningValueTooLarge}
                              aria-describedby={planningValueTooLarge
                                ? `review-task-limit-${task.id}`
                                : undefined}
                              rows="5"
                              placeholder="State the exact verified outcome to review"
                              required
                            ></textarea>
                            {#if planningValueTooLarge}
                              <p
                                class="editor-validation"
                                id={`review-task-limit-${task.id}`}
                                role="alert"
                              >
                                Text is too long ({planningValueByteLength} of
                                {planningValueByteLimit} bytes).
                              </p>
                            {/if}
                            {#if !planningEditorAvailable}
                              <p class="editor-availability" role="status">
                                This task changed and can no longer accept this action. Your draft is preserved.
                              </p>
                            {/if}
                            <div class="editor-actions">
                              <button
                                class="secondary-button"
                                type="button"
                                disabled={planningBusy || interactionDisabled}
                                onclick={closeEditor}
                              >
                                Cancel
                              </button>
                              <button
                                class="primary-button"
                                type="submit"
                                disabled={planningSubmitDisabled}
                              >
                                {#if pendingPlanningKind === "task_complete_review"}
                                  <LoaderCircle class="spin" size={16} aria-hidden="true" />
                                  Preparing
                                {:else}
                                  <ClipboardCheck size={16} aria-hidden="true" />
                                  Review completion
                                {/if}
                              </button>
                            </div>
                          </form>
                        {/if}
                      </li>
                    {/each}
                  </ul>
                {/if}
              </article>
            {/each}
          </div>

          {#if pendingPlanningKind}
            <div class="planning-progress" role="status">
              <LoaderCircle class="spin" size={15} aria-hidden="true" />
              {planningLabel(pendingPlanningKind)}
            </div>
          {/if}
        </section>
      {/if}
    </main>

    {#if hasCoordination}
      <aside class="context-rail" aria-label="Coordination">
        {#if agentNextStep}
          <section class="context-section" aria-labelledby="coordination-title">
            <div class="context-heading">
              <Compass size={17} aria-hidden="true" />
              <h2 id="coordination-title">Coordination</h2>
            </div>
            <div class="agent-next-step">
              <span class="section-kicker">Agent next step</span>
              <strong>{agentNextStep.label}</strong>
              <p>{agentNextStep.rationale}</p>
            </div>
            <details class="agent-details">
              <summary>Agent details</summary>
              <p class="situation">{snapshot.steering.situation}</p>
              <code>{agentNextStep.command}</code>
            </details>
          </section>
        {/if}

        {#if snapshot.diagnostics.length > 0}
          <section class="context-section diagnostics" aria-labelledby="diagnostics-title">
            <div class="context-heading">
              <ListTodo size={17} aria-hidden="true" />
              <h2 id="diagnostics-title">Diagnostics</h2>
              <span class="count">{snapshot.diagnostics.length}</span>
            </div>

            <ul class="diagnostic-list">
              {#each snapshot.diagnostics as diagnostic (`${diagnostic.code}-${diagnostic.message}`)}
                <li class={diagnostic.severity}>
                  <span aria-hidden="true">
                    {#if diagnosticIcon(diagnostic) === "error"}
                      <XCircle size={17} />
                    {:else if diagnosticIcon(diagnostic) === "warning"}
                      <AlertTriangle size={17} />
                    {:else}
                      <Info size={17} />
                    {/if}
                  </span>
                  <div>
                    <strong>{diagnostic.code}</strong>
                    <p>{diagnostic.message}</p>
                  </div>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </aside>
    {/if}
  </div>

  <footer>
    <span>Revision {snapshot.revision}</span>
    <span>Observed {observedTime(snapshot.observed_at)}</span>
    <span class="project-id">{snapshot.project.id}</span>
  </footer>
</div>

<style>
  .workbench {
    --ink: #17201f;
    --muted: #65706e;
    --line: #d9dfdd;
    --line-strong: #c3ccca;
    --surface: #ffffff;
    --surface-soft: #f4f7f6;
    --surface-quiet: #e9efed;
    --teal: #176f61;
    --teal-soft: #dceee9;
    --blue: #315f96;
    --blue-soft: #e6eef8;
    --amber: #9a6717;
    --amber-soft: #f7edd8;
    --red: #b43e46;
    --red-soft: #f8e4e5;

    min-height: 100vh;
    background: var(--surface-soft);
    color: var(--ink);
    display: flex;
    flex-direction: column;
  }

  .topbar {
    min-height: 58px;
    display: grid;
    grid-template-columns: minmax(220px, 280px) minmax(0, 1fr) auto;
    align-items: center;
    gap: 20px;
    padding: 0 18px;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }

  .brand,
  .workspace-identity,
  .topbar-actions,
  .identity-item,
  .dirty-indicator,
  .connection,
  .lane-context span,
  .context-heading {
    display: flex;
    align-items: center;
  }

  .brand {
    gap: 10px;
    min-width: 0;
  }

  .brand-mark {
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: 7px;
    background: var(--ink);
    color: white;
  }

  .brand > div {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .brand-name {
    font-size: 0.95rem;
    font-weight: 750;
    line-height: 1.05;
  }

  .brand-context {
    margin-top: 2px;
    color: var(--muted);
    font-size: 0.72rem;
  }

  .workspace-identity {
    min-width: 0;
    gap: 12px;
    color: var(--muted);
    font-size: 0.78rem;
  }

  .workspace-identity strong {
    min-width: 0;
    overflow: hidden;
    color: var(--ink);
    font-size: 0.88rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .identity-item,
  .dirty-indicator,
  .connection,
  .lane-context span {
    gap: 5px;
  }

  .identity-item,
  .dirty-indicator {
    white-space: nowrap;
  }

  .mono,
  code,
  .project-id {
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", monospace;
  }

  .dirty-indicator {
    color: var(--amber);
    font-weight: 650;
  }

  .topbar-actions {
    gap: 8px;
  }

  .connection {
    gap: 5px;
    padding: 4px 7px;
    border-radius: 4px;
    background: var(--amber-soft);
    color: var(--amber);
    font-size: 0.72rem;
    font-weight: 700;
  }

  .connection.connected {
    background: var(--teal-soft);
    color: var(--teal);
  }

  .connection.reconnecting {
    background: var(--amber-soft);
    color: var(--amber);
  }

  .connection.paused {
    background: var(--red-soft);
    color: #812c33;
  }

  .icon-button {
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--surface);
    color: var(--ink);
    cursor: pointer;
  }

  .icon-button:hover:not(:disabled) {
    border-color: var(--line-strong);
    background: var(--surface-soft);
  }

  .icon-button:disabled {
    cursor: wait;
    opacity: 0.58;
  }

  .lane-invoker {
    display: none;
    anchor-name: --lane-navigation-trigger;
  }

  .recovery-banner {
    min-height: 40px;
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 0 18px;
    border-bottom: 1px solid #d8c28e;
    background: var(--amber-soft);
    color: #765014;
    font-size: 0.78rem;
  }

  .recovery-banner span {
    flex: 1;
  }

  .recovery-banner.needs-launch {
    border-bottom-color: #e9bdc0;
    background: var(--red-soft);
    color: #812c33;
  }

  .recovery-banner button {
    padding: 5px 9px;
    border: 1px solid currentColor;
    border-radius: 5px;
    background: transparent;
    color: inherit;
    font-weight: 700;
    cursor: pointer;
  }

  .failure-banner {
    min-height: 42px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 18px;
    border-bottom: 1px solid #e9bdc0;
    background: var(--red-soft);
    color: #812c33;
    font-size: 0.82rem;
  }

  .failure-banner span {
    flex: 1;
  }

  .failure-banner strong {
    font-weight: 800;
  }

  .failure-banner.refresh-failure {
    border-bottom-color: #e5c995;
    background: var(--amber-soft);
    color: #7a5213;
  }

  .failure-banner button {
    border: 1px solid #ce858a;
    border-radius: 5px;
    background: #fff7f7;
    color: #812c33;
    padding: 5px 10px;
    font: inherit;
    font-weight: 700;
    cursor: pointer;
  }

  .planning-notice {
    min-height: 42px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 18px;
    border-bottom: 1px solid #a8d1c8;
    background: var(--teal-soft);
    color: var(--teal);
    font-size: 0.78rem;
  }

  .planning-notice strong {
    font-weight: 800;
  }

  .workspace-grid {
    min-height: 0;
    flex: 1;
    display: grid;
    grid-template-columns: minmax(230px, 280px) minmax(420px, 1fr);
  }

  .workspace-grid.has-coordination {
    grid-template-columns:
      minmax(230px, 280px)
      minmax(420px, 1fr)
      minmax(280px, 340px);
  }

  .lane-rail,
  .context-rail {
    min-width: 0;
    background: var(--surface);
  }

  .lane-rail {
    border-right: 1px solid var(--line);
    padding: 18px 12px;
    box-shadow: 10px 0 24px -24px rgba(23, 32, 31, 0.7);
  }

  .rail-heading,
  .section-heading,
  .intent-heading,
  .goal-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }

  .rail-heading {
    padding: 0 7px 13px;
    align-items: center;
  }

  .rail-heading-actions {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .rail-close {
    display: none;
  }

  .section-kicker {
    display: block;
    color: var(--muted);
    font-size: 0.66rem;
    font-weight: 750;
    text-transform: uppercase;
  }

  h1,
  h2,
  h3,
  p {
    margin: 0;
  }

  h1,
  h2,
  h3 {
    letter-spacing: 0;
  }

  .rail-heading h2 {
    margin-top: 2px;
    font-size: 1rem;
  }

  .count {
    min-width: 22px;
    height: 22px;
    display: inline-grid;
    place-items: center;
    border-radius: 4px;
    background: var(--surface-quiet);
    color: var(--muted);
    font-size: 0.68rem;
    font-weight: 750;
  }

  .lane-list {
    display: grid;
    gap: 4px;
  }

  .lane-row {
    width: 100%;
    min-height: 58px;
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) 8px;
    align-items: center;
    gap: 9px;
    padding: 8px 9px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    color: var(--ink);
    text-align: left;
    cursor: pointer;
  }

  .lane-row:hover:not(:disabled) {
    border-color: var(--line);
    background: var(--surface-soft);
  }

  .lane-row.focused {
    border-color: #a9c9c2;
    background: var(--teal-soft);
  }

  .lane-row:disabled {
    cursor: default;
  }

  .lane-row:disabled:not(.focused) {
    opacity: 0.64;
  }

  .lane-state {
    color: var(--muted);
  }

  .lane-row.focused .lane-state {
    color: var(--teal);
  }

  .lane-copy {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .lane-copy strong,
  .lane-copy span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .lane-copy strong {
    font-size: 0.8rem;
  }

  .lane-copy span {
    color: var(--muted);
    font-size: 0.68rem;
  }

  .state-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #a8b1af;
  }

  .state-dot.executing {
    background: var(--teal);
  }

  .rail-empty {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 14px 8px;
    color: var(--muted);
    font-size: 0.78rem;
  }

  .main-surface {
    min-width: 0;
    background: var(--surface-soft);
  }

  .intent-band {
    padding: 30px clamp(24px, 4vw, 54px);
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }

  .intent-heading {
    align-items: center;
    justify-content: flex-start;
  }

  .status-label {
    display: inline-flex;
    align-items: center;
    border-radius: 4px;
    padding: 3px 6px;
    background: var(--surface-quiet);
    color: var(--muted);
    font-size: 0.64rem;
    font-weight: 750;
    text-transform: capitalize;
  }

  .status-label.executing,
  .status-label.active {
    background: var(--teal-soft);
    color: var(--teal);
  }

  .status-label.complete {
    background: var(--blue-soft);
    color: var(--blue);
  }

  .status-label.pending,
  .status-label.prepared {
    background: var(--amber-soft);
    color: var(--amber);
  }

  .intent-band h1 {
    margin-top: 12px;
    max-width: 820px;
    font-size: clamp(1.55rem, 2.3vw, 2.15rem);
    line-height: 1.12;
    text-wrap: balance;
  }

  .lane-intent {
    max-width: 820px;
    margin-top: 12px;
    color: #34413f;
    font-size: 1rem;
    line-height: 1.55;
    text-wrap: pretty;
  }

  .lane-context {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    margin-top: 18px;
    color: var(--muted);
    font-size: 0.76rem;
  }

  .plan-section {
    padding: 26px clamp(24px, 4vw, 54px) 48px;
  }

  .section-heading {
    align-items: center;
    padding-bottom: 13px;
    border-bottom: 1px solid var(--line-strong);
  }

  .section-heading h2 {
    margin-top: 3px;
    font-size: 1.12rem;
  }

  .goal-list {
    display: grid;
  }

  .goal {
    padding: 22px 0;
    border-bottom: 1px solid var(--line);
  }

  .goal-heading {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    align-items: start;
    column-gap: 12px;
  }

  .status-icon {
    min-height: 28px;
    display: grid;
    place-items: center;
    color: var(--muted);
  }

  .status-icon.complete,
  .task-check.complete {
    color: var(--blue);
  }

  .status-icon.active,
  .task-check.active {
    color: var(--teal);
  }

  .status-icon.terminal,
  .task-check.terminal {
    color: var(--muted);
  }

  .goal-copy {
    min-width: 0;
    flex: 1;
  }

  .goal-heading h3 {
    font-size: 0.9rem;
    line-height: 1.35;
    text-wrap: balance;
  }

  .goal-progress {
    display: block;
    margin-top: 4px;
    color: var(--muted);
    font-size: 0.68rem;
    font-weight: 600;
    line-height: 1.35;
  }

  .task-list {
    margin: 16px 0 0 32px;
    padding: 0;
    list-style: none;
  }

  .task-list li {
    border-top: 1px solid #e3e8e6;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .task-row {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    align-items: start;
    column-gap: 10px;
    min-height: 44px;
    padding: 8px 0;
    font-size: 0.77rem;
  }

  .task-check {
    min-height: 28px;
    display: grid;
    place-items: center;
  }

  .task-copy {
    min-width: 0;
    display: grid;
    gap: 3px;
    padding: 4px 0;
  }

  .task-title {
    line-height: 1.4;
    text-wrap: pretty;
  }

  .task-active-label,
  .task-terminal-label {
    width: fit-content;
    font-size: 0.63rem;
    font-weight: 750;
    line-height: 1.25;
  }

  .task-active-label {
    color: var(--teal);
  }

  .task-terminal-label {
    color: var(--muted);
  }

  .task-progress {
    margin: -2px 0 10px 25px;
    color: var(--muted);
    font-size: 0.68rem;
  }

  .task-progress summary {
    width: fit-content;
    cursor: pointer;
    font-weight: 700;
  }

  .task-progress ol {
    display: grid;
    gap: 7px;
    margin: 7px 0 0;
    padding: 0;
    list-style: none;
  }

  .task-progress li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    padding: 7px 9px;
    border: 1px solid var(--line);
    border-radius: 5px;
    background: var(--surface-soft);
  }

  .task-progress p {
    color: var(--ink-soft);
    line-height: 1.45;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .task-progress time {
    white-space: nowrap;
  }

  .task-progress .task-progress-note {
    margin-top: 7px;
    color: var(--muted);
    font-size: 0.63rem;
  }

  .task-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 28px;
  }

  .planning-icon-button,
  .primary-icon-button {
    width: 28px;
    height: 28px;
    display: inline-grid;
    flex: 0 0 auto;
    place-items: center;
    border: 1px solid transparent;
    border-radius: 5px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }

  .planning-icon-button:hover:not(:disabled) {
    border-color: var(--line);
    background: var(--surface);
    color: var(--ink);
  }

  .planning-icon-button:disabled,
  .primary-icon-button:disabled {
    cursor: default;
    opacity: 0.38;
  }

  .primary-icon-button {
    border-color: var(--teal);
    background: var(--teal);
    color: white;
  }

  .planning-editor {
    margin: 13px 0 2px 30px;
    padding: 12px;
    border-left: 3px solid #9fc5bd;
    background: #eef5f3;
  }

  .planning-editor.task-editor {
    margin: 0 0 10px 25px;
  }

  .planning-editor label {
    display: block;
    margin-bottom: 7px;
    color: #40504d;
    font-size: 0.7rem;
    font-weight: 750;
  }

  .planning-editor input,
  .planning-editor textarea {
    width: 100%;
    border: 1px solid var(--line-strong);
    border-radius: 5px;
    background: var(--surface);
    color: var(--ink);
    padding: 8px 9px;
    font-size: 0.77rem;
    line-height: 1.45;
  }

  .planning-editor textarea {
    display: block;
    resize: vertical;
  }

  .planning-editor input:read-only,
  .planning-editor textarea:read-only {
    background: #f5f7f6;
    color: var(--muted);
  }

  .editor-validation {
    margin-top: 6px;
    color: var(--danger);
    font-size: 0.68rem;
    font-weight: 650;
  }

  .editor-availability {
    margin-top: 7px;
    color: var(--muted);
    font-size: 0.68rem;
    line-height: 1.45;
  }

  .planning-read-only {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0 0 18px;
    padding: 9px 10px;
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--muted);
    font-size: 0.72rem;
  }

  .editor-control {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 28px 28px;
    align-items: center;
    gap: 6px;
  }

  .editor-actions,
  .review-actions {
    display: flex;
    justify-content: flex-end;
    gap: 7px;
    margin-top: 9px;
  }

  .primary-button,
  .secondary-button {
    min-height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    border: 1px solid var(--line-strong);
    border-radius: 5px;
    padding: 6px 10px;
    font-size: 0.72rem;
    font-weight: 750;
    cursor: pointer;
  }

  .primary-button {
    border-color: var(--teal);
    background: var(--teal);
    color: white;
  }

  .secondary-button {
    background: var(--surface);
    color: var(--ink);
  }

  .primary-button:disabled,
  .secondary-button:disabled {
    cursor: wait;
    opacity: 0.55;
  }

  .completion-review {
    margin-top: 18px;
    padding: 16px;
    border: 1px solid #a9c9c2;
    border-radius: 7px;
    background: var(--surface);
    box-shadow: 0 10px 28px -24px rgba(23, 32, 31, 0.8);
  }

  .review-heading {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .review-dismiss {
    width: 30px;
    height: 30px;
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    margin-left: auto;
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }

  .review-dismiss:hover:not(:disabled) {
    border-color: var(--line);
    background: var(--surface-soft);
    color: var(--ink);
  }

  .review-dismiss:disabled {
    cursor: wait;
    opacity: 0.55;
  }

  .review-heading h3 {
    margin-top: 2px;
    font-size: 0.9rem;
    text-wrap: balance;
  }

  .review-mark {
    width: 34px;
    height: 34px;
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 6px;
    background: var(--teal-soft);
    color: var(--teal);
  }

  .review-rationale {
    margin-top: 13px;
    color: #40504d;
    font-size: 0.78rem;
    line-height: 1.5;
    text-wrap: pretty;
  }

  .review-outcome {
    margin-top: 13px;
    padding: 11px 12px;
    border-left: 3px solid #9fc5bd;
    background: var(--surface-soft);
  }

  .review-outcome > span {
    color: var(--muted);
    font-size: 0.64rem;
    font-weight: 750;
    text-transform: uppercase;
  }

  .review-outcome-text {
    margin-top: 5px;
    font-size: 0.78rem;
    line-height: 1.5;
    overflow-wrap: anywhere;
    text-wrap: pretty;
    white-space: pre-wrap;
  }

  .review-evidence {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 10px;
    color: var(--muted);
    font-size: 0.68rem;
  }

  .planning-progress {
    position: sticky;
    bottom: 12px;
    width: fit-content;
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 18px 0 0 auto;
    padding: 7px 10px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.95);
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 700;
    box-shadow: 0 8px 20px -18px rgba(23, 32, 31, 0.8);
  }

  .no-focus {
    min-height: 300px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 28px;
    color: var(--muted);
    text-align: center;
  }

  .no-focus h1 {
    margin-top: 12px;
    color: var(--ink);
    font-size: 1.3rem;
  }

  .no-focus p {
    max-width: 420px;
    margin-top: 8px;
    line-height: 1.5;
  }

  .context-rail {
    border-left: 1px solid var(--line);
  }

  .context-section {
    padding: 21px 18px;
    border-bottom: 1px solid var(--line);
  }

  .context-section:last-child {
    border-bottom: 0;
  }

  .context-heading {
    gap: 7px;
  }

  .context-heading h2 {
    flex: 1;
    font-size: 0.84rem;
  }

  .situation {
    margin-top: 13px;
    color: #3d4846;
    font-size: 0.8rem;
    line-height: 1.5;
  }

  .diagnostic-list {
    margin: 15px 0 0;
    padding: 0;
    list-style: none;
  }

  .agent-next-step {
    margin-top: 16px;
  }

  .agent-next-step strong {
    display: block;
    margin-top: 5px;
    font-size: 0.8rem;
    line-height: 1.35;
  }

  .agent-next-step p {
    margin-top: 5px;
    color: var(--muted);
    font-size: 0.74rem;
    line-height: 1.45;
  }

  .agent-details {
    margin-top: 14px;
    color: var(--muted);
    font-size: 0.7rem;
  }

  .agent-details summary {
    width: fit-content;
    color: var(--blue);
    font-weight: 700;
    cursor: pointer;
  }

  .agent-details .situation {
    font-size: 0.72rem;
  }

  .agent-details code {
    display: block;
    overflow: hidden;
    margin-top: 9px;
    padding: 6px 7px;
    border: 1px solid var(--line);
    border-radius: 4px;
    background: var(--surface-soft);
    color: #394542;
    font-size: 0.62rem;
    line-height: 1.35;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .diagnostics {
    border-bottom: 0;
  }

  .diagnostic-list {
    display: grid;
    gap: 10px;
  }

  .diagnostic-list li {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr);
    gap: 8px;
    padding: 9px;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--surface-soft);
    color: var(--blue);
  }

  .diagnostic-list li.warning {
    border-color: #e5c995;
    background: var(--amber-soft);
    color: var(--amber);
  }

  .diagnostic-list li.error {
    border-color: #e9bdc0;
    background: var(--red-soft);
    color: var(--red);
  }

  .diagnostic-list strong {
    font-family: "SFMono-Regular", Consolas, "Liberation Mono", monospace;
    font-size: 0.65rem;
  }

  .diagnostic-list p {
    margin-top: 3px;
    color: #4b5553;
    font-size: 0.7rem;
    line-height: 1.4;
  }

  footer {
    min-height: 30px;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 18px;
    border-top: 1px solid var(--line);
    background: var(--surface);
    color: var(--muted);
    font-size: 0.63rem;
  }

  .project-id {
    min-width: 0;
    overflow: hidden;
    margin-left: auto;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spin {
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 1120px) {
    .workspace-grid,
    .workspace-grid.has-coordination {
      grid-template-columns: minmax(220px, 260px) minmax(0, 1fr);
    }

    .context-rail {
      grid-column: 1 / -1;
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
      border-top: 1px solid var(--line);
      border-left: 0;
    }

    .context-section {
      border-right: 1px solid var(--line);
      border-bottom: 0;
    }

    .context-section:last-child {
      border-right: 0;
    }
  }

  @media (max-width: 760px) {
    .topbar {
      grid-template-columns: minmax(0, 1fr) auto;
      gap: 10px;
      padding: 8px 12px;
    }

    .workspace-identity {
      grid-column: 1 / -1;
      grid-row: 2;
      overflow: hidden;
      padding-bottom: 2px;
    }

    .workspace-identity strong {
      flex: 1;
    }

    .workspace-identity .identity-item {
      display: none;
    }

    .topbar-actions {
      grid-column: 2;
      grid-row: 1;
    }

    .lane-invoker {
      display: grid;
    }

    .workspace-grid {
      display: block;
    }

    .lane-rail {
      display: none;
    }

    .lane-rail[popover]:popover-open {
      position: fixed;
      inset: 66px 12px auto auto;
      width: min(320px, calc(100vw - 24px));
      max-height: calc(100dvh - 78px);
      display: block;
      overflow: auto;
      margin: 0;
      padding: 14px 12px;
      border: 1px solid var(--line-strong);
      border-radius: 8px;
      background: var(--surface);
      box-shadow:
        0 24px 56px -30px rgba(23, 32, 31, 0.72),
        0 8px 22px -18px rgba(23, 32, 31, 0.5);
    }

    .lane-rail[popover]::backdrop {
      background: rgba(23, 32, 31, 0.08);
    }

    @supports (top: anchor(bottom)) {
      .lane-rail[popover]:popover-open {
        position-anchor: --lane-navigation-trigger;
        top: calc(anchor(bottom) + 8px);
        right: 12px;
        bottom: auto;
        left: auto;
        position-try-fallbacks: flip-block;
      }
    }

    .rail-close {
      display: grid;
    }

    .task-row {
      grid-template-columns: 18px minmax(0, 1fr);
      row-gap: 3px;
      padding: 9px 0;
    }

    .task-actions {
      grid-column: 2;
      justify-content: flex-start;
    }

    .planning-editor,
    .planning-editor.task-editor {
      margin-left: 0;
    }

    .intent-band,
    .plan-section {
      padding-right: 20px;
      padding-left: 20px;
    }

    .context-rail {
      display: block;
    }

    .context-section {
      border-right: 0;
      border-bottom: 1px solid var(--line);
    }

    footer {
      flex-wrap: wrap;
      gap: 8px 14px;
      padding: 7px 12px;
    }

    .project-id {
      width: 100%;
      margin-left: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
</style>
