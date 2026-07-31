<script lang="ts">
  import {
    AlertTriangle,
    FolderX,
    Link2Off,
    LoaderCircle,
    RefreshCw,
    Route,
    WifiOff,
  } from "@lucide/svelte";
  import { onMount } from "svelte";

  import WorkbenchView from "$lib/WorkbenchView.svelte";
  import type {
    WorkbenchPlanningOperation,
    WorkbenchPlanningRequest,
    WorkbenchSnapshot,
    WorkbenchTaskCompletionReview,
  } from "$lib/workbench";
  import {
    createWorkbenchRequestId,
    exchangeWorkbenchTicket,
    launchTicketFromHash,
    prepareWorkbenchTicketExchange,
    retainSessionSelector,
    sessionKeyFromHistory,
    WorkbenchClient,
    WorkbenchClientError,
    type WorkbenchFailureKind,
  } from "$lib/workbench-client";

  type ScreenState =
    | "loading"
    | "ready"
    | "session_required"
    | "session_expired"
    | "workbench_busy"
    | "request_rejected"
    | "workspace_unavailable"
    | "transport_error";

  type SessionRecoveryState = "connected" | "reconnecting" | "needs_launch";

  const SESSION_RENEW_INTERVAL_MS = 5 * 60_000;

  interface FocusRequest {
    laneId: string;
    requestId: string;
  }

  interface PreparedPlanningRequest {
    request: WorkbenchPlanningRequest;
  }

  interface PlanningSuccess {
    requestId: string;
    operation: WorkbenchPlanningOperation;
  }

  interface BoundCompletionReview {
    review: WorkbenchTaskCompletionReview;
    expectedDaemonInstanceId: string;
    expectedRevision: number;
    expectedPhaseId: string;
  }

  let screen = $state<ScreenState>("loading");
  let snapshot = $state<WorkbenchSnapshot | null>(null);
  let refreshing = $state(false);
  let streamConnected = $state(false);
  let pendingFocus = $state<FocusRequest | null>(null);
  let retryFocus = $state<FocusRequest | null>(null);
  let focusFailure = $state<string | null>(null);
  let refreshFailure = $state<string | null>(null);
  let pendingPlanning = $state<PreparedPlanningRequest | null>(null);
  let retryPlanning = $state<PreparedPlanningRequest | null>(null);
  let planningFailure = $state<string | null>(null);
  let planningNotice = $state<string | null>(null);
  let planningSuccess = $state<PlanningSuccess | null>(null);
  let completionReview = $state<BoundCompletionReview | null>(null);
  let screenMessage = $state<string | null>(null);
  let screenRetryable = $state(false);
  let retryBootstrap = $state<(() => void) | null>(null);
  let sessionRecovery = $state<SessionRecoveryState>("connected");
  let sessionRecoveryMessage = $state<string | null>(null);

  let client: WorkbenchClient | null = null;
  let ambiguousFocus: FocusRequest | null = null;
  let refreshQueued = false;
  let startLiveUpdates: (() => void) | null = null;
  let stopLiveUpdates: (() => void) | null = null;
  let beginSessionRecovery: (() => void) | null = null;

  onMount(() => {
    let events: EventSource | null = null;
    let pollTimer: number | null = null;
    let renewalTimer: number | null = null;
    let recoveryTimer: number | null = null;
    let recoveryAttempt = 0;
    let recoveryInFlight = false;
    let liveUpdatesStarted = false;
    let bootstrapGeneration = 0;

    const refreshForVisibility = () => {
      if (document.visibilityState === "visible") {
        void refreshSnapshot(true);
      }
    };

    const refreshForFocus = () => {
      if (document.visibilityState === "visible") {
        void refreshSnapshot(true);
      }
    };

    const startUpdates = () => {
      if (!client || liveUpdatesStarted) {
        return;
      }
      liveUpdatesStarted = true;
      try {
        events = new EventSource(client.eventSourceUrl());
        events.onopen = () => {
          streamConnected = true;
        };
        events.onerror = () => {
          streamConnected = false;
        };
        events.addEventListener("ready", () => {
          streamConnected = true;
        });
        events.addEventListener("invalidate", () => {
          void refreshSnapshot(true);
        });
      } catch {
        streamConnected = false;
      }
      pollTimer = window.setInterval(() => {
        if (document.visibilityState === "visible") {
          void refreshSnapshot(true);
        }
      }, 5_000);
      renewalTimer = window.setInterval(() => {
        if (document.visibilityState === "visible") {
          void renewCurrentSession();
        }
      }, SESSION_RENEW_INTERVAL_MS);
      document.addEventListener("visibilitychange", refreshForVisibility);
      window.addEventListener("focus", refreshForFocus);
    };
    const stopUpdates = () => {
      events?.close();
      events = null;
      if (pollTimer !== null) {
        window.clearInterval(pollTimer);
        pollTimer = null;
      }
      if (renewalTimer !== null) {
        window.clearInterval(renewalTimer);
        renewalTimer = null;
      }
      document.removeEventListener("visibilitychange", refreshForVisibility);
      window.removeEventListener("focus", refreshForFocus);
      liveUpdatesStarted = false;
      streamConnected = false;
    };
    const clearRecoveryTimer = () => {
      if (recoveryTimer !== null) {
        window.clearTimeout(recoveryTimer);
        recoveryTimer = null;
      }
    };
    const recoverSession = async () => {
      const activeClient = client;
      if (!activeClient || snapshot === null || recoveryInFlight) {
        return;
      }
      recoveryInFlight = true;
      clearRecoveryTimer();
      stopUpdates();
      sessionRecovery = "reconnecting";
      sessionRecoveryMessage = null;
      try {
        await activeClient.renewSession();
        if (client !== activeClient) {
          return;
        }
        const nextSnapshot = await activeClient.snapshot();
        if (client !== activeClient) {
          return;
        }
        recoveryAttempt = 0;
        sessionRecovery = "connected";
        sessionRecoveryMessage = null;
        applySnapshot(nextSnapshot);
      } catch (error) {
        if (client !== activeClient) {
          return;
        }
        if (
          error instanceof WorkbenchClientError &&
          (error.kind === "session_expired" ||
            error.kind === "workspace_unavailable")
        ) {
          sessionRecovery = "needs_launch";
          sessionRecoveryMessage = messageFrom(error);
          return;
        }
        recoveryAttempt += 1;
        const delay = Math.min(15_000, 1_000 * 2 ** Math.min(recoveryAttempt, 4));
        recoveryTimer = window.setTimeout(() => {
          recoveryTimer = null;
          void recoverSession();
        }, delay);
      } finally {
        recoveryInFlight = false;
      }
    };
    const renewCurrentSession = async () => {
      const activeClient = client;
      if (!activeClient || sessionRecovery !== "connected") {
        return;
      }
      try {
        await activeClient.renewSession();
      } catch {
        if (client === activeClient) {
          void recoverSession();
        }
      }
    };
    beginSessionRecovery = () => void recoverSession();
    startLiveUpdates = startUpdates;
    stopLiveUpdates = stopUpdates;

    const resetClientState = () => {
      clearRecoveryTimer();
      stopUpdates();
      client = null;
      snapshot = null;
      pendingFocus = null;
      retryFocus = null;
      ambiguousFocus = null;
      focusFailure = null;
      refreshFailure = null;
      pendingPlanning = null;
      retryPlanning = null;
      planningFailure = null;
      planningSuccess = null;
      completionReview = null;
      recoveryAttempt = 0;
      sessionRecovery = "connected";
      sessionRecoveryMessage = null;
    };

    const bootstrap = async (
      retryTicket?: string,
      restoredSessionKey?: string | null,
    ) => {
      const generation = ++bootstrapGeneration;
      const isCurrent = () => generation === bootstrapGeneration;
      const ticket = retryTicket ?? launchTicketFromHash(location.hash);
      screen = "loading";
      screenMessage = null;
      screenRetryable = false;
      retryBootstrap = null;
      try {
        let sessionKey =
          restoredSessionKey === undefined
            ? sessionKeyFromHistory(history.state)
            : restoredSessionKey;
        if (ticket || sessionKey !== client?.sessionKey) {
          resetClientState();
        }
        if (ticket) {
          prepareWorkbenchTicketExchange(history, location);
          const session = await exchangeWorkbenchTicket(ticket);
          if (!isCurrent()) {
            return;
          }
          sessionKey = session.session_key;
          retainSessionSelector(history, location, sessionKey);
        }
        if (!isCurrent()) {
          return;
        }
        if (!sessionKey) {
          screen = "session_required";
          return;
        }

        client = new WorkbenchClient(sessionKey);
        if (!ticket) {
          await client.renewSession();
        }
        await refreshSnapshot(false);
      } catch (error) {
        if (isCurrent()) {
          if (
            ticket &&
            error instanceof WorkbenchClientError &&
            error.kind === "server_busy"
          ) {
            retryBootstrap = () => void bootstrap(ticket);
          }
          applyTerminalFailure(error);
        }
      }
    };

    const bootstrapFreshTicket = () => {
      if (launchTicketFromHash(location.hash)) {
        void bootstrap();
      }
    };
    const bootstrapRestoredSession = (event: PopStateEvent) => {
      const restoredSessionKey = sessionKeyFromHistory(event.state);
      if (restoredSessionKey !== client?.sessionKey) {
        void bootstrap(undefined, restoredSessionKey);
      }
    };
    window.addEventListener("hashchange", bootstrapFreshTicket);
    window.addEventListener("popstate", bootstrapRestoredSession);
    void bootstrap();

    return () => {
      bootstrapGeneration += 1;
      retryBootstrap = null;
      startLiveUpdates = null;
      beginSessionRecovery = null;
      window.removeEventListener("hashchange", bootstrapFreshTicket);
      window.removeEventListener("popstate", bootstrapRestoredSession);
      clearRecoveryTimer();
      stopUpdates();
      stopLiveUpdates = null;
    };
  });

  async function refreshSnapshot(quiet: boolean): Promise<void> {
    if (!client) {
      return;
    }
    if (refreshing) {
      refreshQueued = true;
      return;
    }

    refreshing = true;
    if (!quiet && snapshot === null) {
      screen = "loading";
    }
    const activeClient = client;
    try {
      const nextSnapshot = await activeClient.snapshot();
      if (client !== activeClient) {
        return;
      }
      applySnapshot(nextSnapshot);
    } catch (error) {
      if (client !== activeClient) {
        return;
      }
      if (terminalFailure(error)) {
        applyTerminalFailure(error);
      } else if (snapshot === null) {
        const kind =
          error instanceof WorkbenchClientError
            ? error.kind
            : "transport_error";
        screen = screenForFailure(kind);
        screenMessage = messageFrom(error);
        screenRetryable =
          error instanceof WorkbenchClientError && error.retryable;
      } else {
        refreshFailure = messageFrom(error);
      }
    } finally {
      refreshing = false;
      if (refreshQueued) {
        refreshQueued = false;
        void refreshSnapshot(true);
      }
    }
  }

  function applySnapshot(nextSnapshot: WorkbenchSnapshot): void {
    snapshot = nextSnapshot;
    screen = "ready";
    screenMessage = null;
    screenRetryable = false;
    refreshFailure = null;
    if (
      completionReview &&
      (nextSnapshot.daemon.instance_id !==
        completionReview.expectedDaemonInstanceId ||
        nextSnapshot.revision !== completionReview.expectedRevision ||
        nextSnapshot.phase?.id !== completionReview.expectedPhaseId) &&
      !(
        pendingPlanning?.request.operation.kind ===
          "task_complete_approve" &&
        pendingPlanning.request.operation.review_id ===
          completionReview.review.review_id
      )
    ) {
      completionReview = null;
      retryPlanning = null;
      planningFailure =
        "The plan changed. Review task completion again from the current plan.";
    }
    if (
      ambiguousFocus &&
      nextSnapshot.focused_lane?.id === ambiguousFocus.laneId
    ) {
      ambiguousFocus = null;
      focusFailure = null;
      retryFocus = null;
    }
    retryBootstrap = null;
    startLiveUpdates?.();
  }

  async function focusLane(
    laneId: string,
    requestId = createWorkbenchRequestId(),
  ): Promise<void> {
    if (!client || pendingFocus || sessionRecovery !== "connected") {
      return;
    }

    const activeClient = client;
    const request = { laneId, requestId };
    pendingFocus = request;
    retryFocus = null;
    ambiguousFocus = null;
    focusFailure = null;
    try {
      await activeClient.focusLane(laneId, requestId);
    } catch (error) {
      if (client !== activeClient) {
        return;
      }
      if (terminalFailure(error)) {
        applyTerminalFailure(error);
      } else {
        focusFailure = messageFrom(error);
        if (error instanceof WorkbenchClientError && error.retryable) {
          if (error.retryWithSameRequestId) {
            ambiguousFocus = request;
          }
          retryFocus = {
            laneId,
            requestId: error.retryWithSameRequestId
              ? requestId
              : createWorkbenchRequestId(),
          };
        }
      }
    } finally {
      if (client === activeClient) {
        pendingFocus = null;
        await refreshSnapshot(true);
      }
    }
  }

  function preparePlanningRequest(
    operation: WorkbenchPlanningOperation,
    binding?: {
      daemonInstanceId: string;
      revision: number;
      phaseId: string;
    },
  ): PreparedPlanningRequest | null {
    if (
      !client ||
      !snapshot?.phase ||
      sessionRecovery !== "connected"
    ) {
      return null;
    }
    return {
      request: {
        protocol_version: 2,
        id: createWorkbenchRequestId(),
        session_key: client.sessionKey,
        expected_daemon_instance_id:
          binding?.daemonInstanceId ?? snapshot.daemon.instance_id,
        expected_revision: binding?.revision ?? snapshot.revision,
        expected_phase_id: binding?.phaseId ?? snapshot.phase.id,
        operation,
      },
    };
  }

  async function submitPlanning(
    operation: WorkbenchPlanningOperation,
  ): Promise<boolean> {
    const prepared = preparePlanningRequest(operation);
    return prepared ? executePlanning(prepared) : false;
  }

  async function executePlanning(
    prepared: PreparedPlanningRequest,
  ): Promise<boolean> {
    if (!client || pendingPlanning) {
      return false;
    }

    const activeClient = client;
    pendingPlanning = prepared;
    retryPlanning = null;
    planningFailure = null;
    planningNotice = null;
    try {
      const result = await activeClient.planning(prepared.request);
      if (client !== activeClient) {
        return false;
      }
      if (result.kind === "workbench.task_completion_review") {
        completionReview = {
          review: result,
          expectedDaemonInstanceId:
            prepared.request.expected_daemon_instance_id,
          expectedRevision: prepared.request.expected_revision,
          expectedPhaseId: prepared.request.expected_phase_id,
        };
      } else {
        completionReview = null;
      }
      if (prepared.request.operation.kind === "task_start") {
        planningNotice =
          "Exo marked the task active; the workbench did not start an agent.";
      }
      planningFailure = null;
      planningSuccess = {
        requestId: prepared.request.id,
        operation: prepared.request.operation,
      };
      await refreshSnapshot(true);
      return true;
    } catch (error) {
      if (client !== activeClient) {
        return false;
      }
      if (terminalFailure(error)) {
        applyTerminalFailure(error);
      } else {
        planningFailure = messageFrom(error);
        if (
          error instanceof WorkbenchClientError &&
          error.retryable &&
          error.retryWithSameRequestId
        ) {
          retryPlanning = prepared;
        }
        if (
          error instanceof WorkbenchClientError &&
          (error.detailKind === "workbench.stale_snapshot" ||
            error.detailKind === "workbench.phase_mismatch" ||
            error.detailKind === "workbench.review_invalid")
        ) {
          completionReview = null;
          await refreshSnapshot(true);
        }
      }
      return false;
    } finally {
      if (client === activeClient) {
        pendingPlanning = null;
      }
    }
  }

  async function approveCompletionReview(): Promise<boolean> {
    if (!completionReview) {
      return false;
    }
    const prepared = preparePlanningRequest(
      {
        kind: "task_complete_approve",
        review_id: completionReview.review.review_id,
      },
      {
        daemonInstanceId: completionReview.expectedDaemonInstanceId,
        revision: completionReview.expectedRevision,
        phaseId: completionReview.expectedPhaseId,
      },
    );
    return prepared ? executePlanning(prepared) : false;
  }

  function applyTerminalFailure(error: unknown): void {
    const kind =
      error instanceof WorkbenchClientError ? error.kind : "transport_error";
    if (
      snapshot !== null &&
      (kind === "session_expired" || kind === "workspace_unavailable")
    ) {
      screen = "ready";
      beginSessionRecovery?.();
      return;
    }
    screen = screenForFailure(kind);
    screenMessage = messageFrom(error);
    screenRetryable =
      error instanceof WorkbenchClientError && error.retryable;
    stopLiveUpdates?.();
  }

  function retryPendingFocus(): void {
    if (retryFocus) {
      void focusLane(retryFocus.laneId, retryFocus.requestId);
    }
  }

  function retryPendingPlanning(): void {
    if (retryPlanning) {
      void executePlanning(retryPlanning);
    }
  }

  function retryTransport(): void {
    if (snapshot !== null && sessionRecovery !== "connected") {
      beginSessionRecovery?.();
      return;
    }
    if (client) {
      void refreshSnapshot(false);
    } else {
      retryBootstrap?.();
    }
  }

  function terminalFailure(error: unknown): boolean {
    return (
      error instanceof WorkbenchClientError &&
      (error.kind === "session_expired" ||
        error.kind === "workspace_unavailable")
    );
  }

  function screenForFailure(kind: WorkbenchFailureKind): ScreenState {
    switch (kind) {
      case "session_required":
        return "session_required";
      case "session_expired":
        return "session_expired";
      case "server_busy":
        return "workbench_busy";
      case "command_failed":
        return "request_rejected";
      case "workspace_unavailable":
        return "workspace_unavailable";
      default:
        return "transport_error";
    }
  }

  function messageFrom(error: unknown): string {
    return error instanceof Error
      ? error.message
      : "The workbench request could not be completed";
  }

  const stateContent = (state: ScreenState) => {
    switch (state) {
      case "session_required":
        return {
          title: "Launch link required",
          body:
            screenMessage ??
            "Open a current Exo workbench link for this workspace.",
        };
      case "session_expired":
        return {
          title: "Session expired",
          body: "Open a fresh Exo workbench link to continue.",
        };
      case "workbench_busy":
        return {
          title: "Workbench is busy",
          body:
            screenMessage ??
            "Wait for capacity, then retry this launch ticket.",
        };
      case "request_rejected":
        return {
          title: "Workbench request rejected",
          body:
            screenMessage ??
            "Exo rejected this workbench request.",
        };
      case "workspace_unavailable":
        return {
          title: "Workspace unavailable",
          body: "The workspace bound to this session can no longer be verified.",
        };
      case "transport_error":
        return {
          title: "Exo is not responding",
          body:
            screenMessage ??
            "The workbench could not reach the project-authority daemon.",
        };
      default:
        return {
          title: "Opening lane workspace",
          body: "Connecting to the project-authority daemon.",
        };
    }
  };
</script>

<svelte:head>
  <title>Exo Lane Workbench</title>
  <meta
    name="description"
    content="Focused lane workspace for the current Exo project"
  />
</svelte:head>

{#if screen === "ready" && snapshot}
  <WorkbenchView
    {snapshot}
    {refreshing}
    {streamConnected}
    {sessionRecovery}
    {sessionRecoveryMessage}
    pendingLaneId={pendingFocus?.laneId ?? null}
    {focusFailure}
    {refreshFailure}
    planningFailure={planningFailure}
    {planningNotice}
    {planningSuccess}
    pendingPlanningKind={pendingPlanning?.request.operation.kind ?? null}
    completionReview={completionReview?.review ?? null}
    onFocus={(laneId) => void focusLane(laneId)}
    onRetryFocus={retryFocus ? retryPendingFocus : null}
    onRetryPlanning={retryPlanning ? retryPendingPlanning : null}
    onRetrySession={() => beginSessionRecovery?.()}
    onRefresh={() => void refreshSnapshot(false)}
    onPlan={submitPlanning}
    onApproveCompletion={approveCompletionReview}
    onDismissCompletionReview={() => {
      completionReview = null;
      planningFailure = null;
    }}
  />
{:else}
  {@const content = stateContent(screen)}
  <main class="state-shell" aria-labelledby="state-title">
    <section
      class={`state-panel ${screen}`}
      role={screen === "loading" ? "status" : undefined}
    >
      <div class="state-mark" aria-hidden="true">
        {#if screen === "loading"}
          <LoaderCircle class="spin" size={24} />
        {:else if screen === "session_required"}
          <Link2Off size={24} />
        {:else if screen === "session_expired"}
          <Route size={24} />
        {:else if screen === "workbench_busy"}
          <AlertTriangle size={24} />
        {:else if screen === "request_rejected"}
          <AlertTriangle size={24} />
        {:else if screen === "workspace_unavailable"}
          <FolderX size={24} />
        {:else}
          <WifiOff size={24} />
        {/if}
      </div>
      <h1 id="state-title">{content.title}</h1>
      <p>{content.body}</p>
      {#if screenRetryable}
        <button type="button" onclick={retryTransport}>
          <RefreshCw size={16} aria-hidden="true" />
          Retry
        </button>
      {:else if screen === "workspace_unavailable"}
        <div class="state-note">
          <AlertTriangle size={15} aria-hidden="true" />
          This session will not fall back to another worktree.
        </div>
      {/if}
    </section>
  </main>
{/if}

<style>
  :global(*) {
    box-sizing: border-box;
  }

  :global(html) {
    color-scheme: light;
    background: #f4f7f6;
  }

  :global(body) {
    min-width: 320px;
    margin: 0;
    background: #f4f7f6;
    color: #17201f;
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
      "Segoe UI", sans-serif;
  }

  :global(button),
  :global(input),
  :global(select),
  :global(textarea) {
    font: inherit;
  }

  :global(button:focus-visible),
  :global(a:focus-visible) {
    outline: 2px solid #176f61;
    outline-offset: 2px;
  }

  .state-shell {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
    background: #f4f7f6;
  }

  .state-panel {
    width: min(440px, 100%);
    padding: 32px;
    border: 1px solid #d9dfdd;
    border-radius: 8px;
    background: #ffffff;
    text-align: center;
  }

  .state-mark {
    width: 46px;
    height: 46px;
    display: grid;
    place-items: center;
    margin: 0 auto;
    border-radius: 8px;
    background: #e9efed;
    color: #176f61;
  }

  .state-panel.session_expired .state-mark,
  .state-panel.workbench_busy .state-mark,
  .state-panel.request_rejected .state-mark,
  .state-panel.workspace_unavailable .state-mark {
    background: #f7edd8;
    color: #946314;
  }

  .state-panel.transport_error .state-mark {
    background: #f8e4e5;
    color: #b43e46;
  }

  h1 {
    margin: 18px 0 0;
    font-size: 1.28rem;
    letter-spacing: 0;
  }

  p {
    margin: 9px auto 0;
    color: #65706e;
    font-size: 0.86rem;
    line-height: 1.5;
  }

  button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    margin-top: 19px;
    padding: 8px 12px;
    border: 1px solid #b7c4c1;
    border-radius: 6px;
    background: #ffffff;
    color: #17201f;
    font-weight: 700;
    cursor: pointer;
  }

  button:hover {
    background: #f4f7f6;
  }

  .state-note {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    margin-top: 18px;
    color: #946314;
    font-size: 0.72rem;
  }

  .spin {
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
</style>
