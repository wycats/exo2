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
  import {
    pushState as pushPageState,
    replaceState as replacePageState,
  } from "$app/navigation";
  import { onMount } from "svelte";

  import WorkbenchView from "$lib/WorkbenchView.svelte";
  import {
    workbenchPlanningBinding,
    type WorkbenchPlanningBinding,
    type WorkbenchLaneInspection,
    type WorkbenchPlanningOperation,
    type WorkbenchPlanningRequest,
    type WorkbenchSnapshot,
    type WorkbenchTaskCompletionReview,
  } from "$lib/workbench";
  import {
    clearPairingResumeRequestId,
    createWorkbenchPairingResumeRequestId,
    createWorkbenchRequestId,
    exchangeWorkbenchTicket,
    launchTicketFromHash,
    pairingResumeRequestIdFromHistory,
    prepareWorkbenchTicketExchange,
    resumeWorkbenchPairing,
    retainPairingResumeRequestId,
    retainSessionSelector,
    sessionKeyFromHistory,
    usesPublishedWorkbenchEntry,
    workbenchHistoryState,
    WorkbenchClient,
    WorkbenchClientError,
    type WorkbenchFailureKind,
    type WorkbenchPairingSummary,
  } from "$lib/workbench-client";

  type ScreenState =
    | "loading"
    | "ready"
    | "session_required"
    | "session_expired"
    | "client_update_required"
    | "workbench_busy"
    | "request_rejected"
    | "workspace_unavailable"
    | "transport_error";

  type SessionRecoveryState =
    | "connected"
    | "reconnecting"
    | "needs_launch"
    | "reload_required";

  const SESSION_RENEW_INTERVAL_MS = 5 * 60_000;
  const INSPECTED_LANE_HISTORY_KEY = "exoWorkbenchInspectedLaneId";
  const PROJECT_OVERVIEW_HISTORY_KEY = "exoWorkbenchProjectOverview";
  const TAB_RESUME_STATE_KEY = "exoWorkbenchResumeState";
  const PAIRING_EVENTS_CHANNEL = "exo-workbench-pairing-events-v1";
  const PAIRING_AUTH_LOCK = "exo-workbench-pairing-auth-v1";
  const PAIRING_ENROLLED_NOTICE = {
    kind: "pairing-enrolled",
    version: 1,
  } as const;

  interface FocusRequest {
    laneId: string;
    requestId: string;
  }

  interface ConfirmedLocalFocus {
    laneId: string;
    daemonInstanceId: string;
    priorRevision: number;
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

  function readTabResumeState(): Record<string, unknown> {
    try {
      const value = JSON.parse(sessionStorage.getItem(TAB_RESUME_STATE_KEY) ?? "null");
      return typeof value === "object" && value !== null && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : {};
    } catch {
      return {};
    }
  }

  function retainTabResumeState(state: Record<string, unknown>): void {
    try {
      sessionStorage.setItem(TAB_RESUME_STATE_KEY, JSON.stringify(state));
    } catch {
      // History state remains the in-session navigation source of truth.
    }
  }

  function clearTabResumeState(): void {
    try {
      sessionStorage.removeItem(TAB_RESUME_STATE_KEY);
    } catch {
      // A fresh launch still clears the retained selector from history state.
    }
  }

  let screen = $state<ScreenState>("loading");
  let snapshot = $state<WorkbenchSnapshot | null>(null);
  let inspection = $state<WorkbenchLaneInspection | null>(null);
  let inspectionLoading = $state(false);
  let inspectionFailure = $state<string | null>(null);
  let inspectionRetryLaneId = $state<string | null>(null);
  let inspectionRetryHistoryMode = $state<InspectionHistoryMode>("none");
  let projectOverview = $state(false);
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
  let planningEditorRebindToken = $state(0);
  let planningEditorRebindPending = false;
  let completionReview = $state<BoundCompletionReview | null>(null);
  let screenMessage = $state<string | null>(null);
  let screenRetryable = $state(false);
  let retryBootstrap = $state<(() => void) | null>(null);
  let sessionRecovery = $state<SessionRecoveryState>("connected");
  let sessionRecoveryMessage = $state<string | null>(null);
  let pairingAvailable = $state(false);
  let pairings = $state<WorkbenchPairingSummary[] | null>(null);
  let pairingsLoading = $state(false);
  let pairingFailure = $state<string | null>(null);
  let pendingPairingSelector = $state<string | null>(null);

  let client: WorkbenchClient | null = null;
  let ambiguousFocus: FocusRequest | null = null;
  let refreshQueued = false;
  let refreshIdleWaiters: Array<(applied: boolean) => void> = [];
  let startLiveUpdates: (() => void) | null = null;
  let stopLiveUpdates: (() => void) | null = null;
  let beginSessionRecovery: (() => void) | null = null;
  let snapshotRefreshGeneration = 0;
  let inspectionRequestGeneration = 0;
  let inspectionRequestedLaneId = $state<string | null>(null);
  let inspectionRequestedHistoryMode: InspectionHistoryMode = "none";
  let pendingRestoredLaneId: string | null = null;
  let pendingRestoredLaneHistoryMode: InspectionHistoryMode = "none";
  let bootstrapInspectionLoading = false;
  let bootstrapInspectionRequestGeneration: number | null = null;
  let confirmedLocalFocus: ConfirmedLocalFocus | null = null;

  onMount(() => {
    const publishedEntry = usesPublishedWorkbenchEntry(location.protocol);
    pairingAvailable = publishedEntry;
    let events: EventSource | null = null;
    let pollTimer: number | null = null;
    let renewalTimer: number | null = null;
    let recoveryTimer: number | null = null;
    let eventRetryTimer: number | null = null;
    let recoveryAttempt = 0;
    let eventRetryAttempt = 0;
    let recoveryInFlight = false;
    let liveUpdatesStarted = false;
    let bootstrapGeneration = 0;
    let pairingEnrollmentRecoveryQueued = false;
    let pairingEnrollmentRecoveryPending = false;
    const pairingEvents = publishedEntry
      ? new BroadcastChannel(PAIRING_EVENTS_CHANNEL)
      : null;
    const withPairingAuthLock = <T,>(operation: () => Promise<T>): Promise<T> =>
      navigator.locks
        .request(PAIRING_AUTH_LOCK, { mode: "exclusive" }, operation)
        .then((result) => result);

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

    const clearEventRetryTimer = () => {
      if (eventRetryTimer !== null) {
        window.clearTimeout(eventRetryTimer);
        eventRetryTimer = null;
      }
    };
    function scheduleEventStreamRetry(): void {
      if (!client || !liveUpdatesStarted || eventRetryTimer !== null) {
        return;
      }
      eventRetryAttempt += 1;
      const delay = Math.min(
        15_000,
        1_000 * 2 ** Math.min(eventRetryAttempt - 1, 4),
      );
      eventRetryTimer = window.setTimeout(() => {
        eventRetryTimer = null;
        openEventStream();
      }, delay);
    }
    function openEventStream(): void {
      const activeClient = client;
      if (!activeClient || !liveUpdatesStarted || events !== null) {
        return;
      }
      try {
        const nextEvents = new EventSource(activeClient.eventSourceUrl());
        events = nextEvents;
        nextEvents.onopen = () => {
          if (events !== nextEvents || client !== activeClient) {
            return;
          }
          streamConnected = true;
        };
        nextEvents.onerror = () => {
          if (events !== nextEvents || client !== activeClient) {
            return;
          }
          nextEvents.close();
          events = null;
          streamConnected = false;
          scheduleEventStreamRetry();
        };
        nextEvents.addEventListener("ready", () => {
          if (events !== nextEvents || client !== activeClient) {
            return;
          }
          clearEventRetryTimer();
          eventRetryAttempt = 0;
          streamConnected = true;
        });
        nextEvents.addEventListener("invalidate", () => {
          if (events !== nextEvents || client !== activeClient) {
            return;
          }
          void refreshSnapshot(true);
        });
      } catch {
        events = null;
        streamConnected = false;
        scheduleEventStreamRetry();
      }
    }
    const startUpdates = () => {
      if (!client || liveUpdatesStarted) {
        return;
      }
      liveUpdatesStarted = true;
      openEventStream();
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
      clearEventRetryTimer();
      eventRetryAttempt = 0;
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
    const resumePublishedSession = async (
      isCurrent: () => boolean = () => true,
    ): Promise<WorkbenchClient> => {
      const resumeState = readTabResumeState();
      const requestId =
        pairingResumeRequestIdFromHistory(history.state) ??
        pairingResumeRequestIdFromHistory(resumeState) ??
        createWorkbenchPairingResumeRequestId();
      const pendingState = retainPairingResumeRequestId(
        {
          ...resumeState,
          ...workbenchHistoryState(history.state),
        },
        requestId,
      );
      replacePageState(
        `${location.pathname}${location.search}`,
        pendingState,
      );
      retainTabResumeState(pendingState);
      const session = await withPairingAuthLock(() =>
        resumeWorkbenchPairing(requestId),
      );
      if (!isCurrent()) {
        throw new Error("A newer workbench bootstrap replaced this pairing resume");
      }
      const resumedState = retainSessionSelector(
        clearPairingResumeRequestId(history.state),
        session.session_key,
      );
      replacePageState(
        `${location.pathname}${location.search}`,
        resumedState,
      );
      retainTabResumeState(resumedState);
      return new WorkbenchClient(session.session_key);
    };
    const recoverSession = async () => {
      let activeClient = client;
      if (!activeClient || snapshot === null || recoveryInFlight) {
        return;
      }
      recoveryInFlight = true;
      snapshotRefreshGeneration += 1;
      clearRecoveryTimer();
      stopUpdates();
      sessionRecovery = "reconnecting";
      sessionRecoveryMessage = null;
      try {
        try {
          await activeClient.renewSession();
        } catch (error) {
          if (
            publishedEntry &&
            error instanceof WorkbenchClientError &&
            error.kind === "session_expired"
          ) {
            activeClient = await resumePublishedSession();
            client = activeClient;
          } else {
            throw error;
          }
        }
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
          if (bootstrapInspectionLoading && pendingRestoredLaneId) {
            finishBootstrapInspection();
          }
          return;
        }
        if (
          error instanceof WorkbenchClientError &&
          error.kind === "client_update_required"
        ) {
          sessionRecovery = "reload_required";
          sessionRecoveryMessage = messageFrom(error);
          refreshFailure = null;
          if (bootstrapInspectionLoading && pendingRestoredLaneId) {
            finishBootstrapInspection();
          }
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
      inspection = null;
      inspectionLoading = false;
      inspectionFailure = null;
      inspectionRetryLaneId = null;
      inspectionRetryHistoryMode = "none";
      projectOverview = false;
      inspectionRequestGeneration += 1;
      inspectionRequestedLaneId = null;
      inspectionRequestedHistoryMode = "none";
      pendingRestoredLaneId = null;
      pendingRestoredLaneHistoryMode = "none";
      bootstrapInspectionLoading = false;
      bootstrapInspectionRequestGeneration = null;
      confirmedLocalFocus = null;
      pendingFocus = null;
      retryFocus = null;
      ambiguousFocus = null;
      focusFailure = null;
      refreshFailure = null;
      pendingPlanning = null;
      retryPlanning = null;
      planningFailure = null;
      planningNotice = null;
      planningSuccess = null;
      planningEditorRebindToken = 0;
      planningEditorRebindPending = false;
      completionReview = null;
      pairings = null;
      pairingsLoading = false;
      pairingFailure = null;
      pendingPairingSelector = null;
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
        const resumeState =
          !ticket && restoredSessionKey === undefined
            ? readTabResumeState()
            : {};
        let sessionKey =
          restoredSessionKey === undefined
            ? sessionKeyFromHistory(history.state) ??
              sessionKeyFromHistory(resumeState)
            : restoredSessionKey;
        if (ticket || sessionKey !== client?.sessionKey) {
          resetClientState();
        }
        if (ticket) {
          clearTabResumeState();
          replacePageState(
            `${location.pathname}${location.search}`,
            prepareWorkbenchTicketExchange(history.state),
          );
          const session = await (publishedEntry
            ? withPairingAuthLock(() => exchangeWorkbenchTicket(ticket))
            : exchangeWorkbenchTicket(ticket));
          if (!isCurrent()) {
            return;
          }
          sessionKey = session.session_key;
          replacePageState(
            `${location.pathname}${location.search}`,
            retainSessionSelector(history.state, sessionKey),
          );
          retainTabResumeState(workbenchHistoryState(history.state));
          if (publishedEntry) {
            pairingEvents?.postMessage(PAIRING_ENROLLED_NOTICE);
          }
        }
        if (!isCurrent()) {
          return;
        }
        if (!sessionKey) {
          if (!publishedEntry) {
            screen = "session_required";
            return;
          }
          const resumedClient = await resumePublishedSession(isCurrent);
          if (!isCurrent()) {
            return;
          }
          client = resumedClient;
          sessionKey = resumedClient.sessionKey;
        }
        if (
          !ticket &&
          !sessionKeyFromHistory(history.state) &&
          sessionKeyFromHistory(resumeState) === sessionKey
        ) {
          const restoredState = retainSessionSelector(
            {
              ...resumeState,
              ...workbenchHistoryState(history.state),
            },
            sessionKey,
          );
          replacePageState(
            `${location.pathname}${location.search}`,
            restoredState,
          );
          retainTabResumeState(restoredState);
        }

        client ??= new WorkbenchClient(sessionKey);
        if (!ticket) {
          try {
            await client.renewSession();
          } catch (error) {
            if (
              publishedEntry &&
              error instanceof WorkbenchClientError &&
              error.kind === "session_expired"
            ) {
              client = await resumePublishedSession(isCurrent);
              sessionKey = client.sessionKey;
            } else {
              throw error;
            }
          }
        }
        const restoredLaneId =
          inspectedLaneFromHistory(history.state) ??
          inspectedLaneFromHistory(resumeState);
        pendingRestoredLaneId = restoredLaneId;
        pendingRestoredLaneHistoryMode = "none";
        bootstrapInspectionLoading = restoredLaneId !== null;
        projectOverview =
          !restoredLaneId &&
          (projectOverviewFromHistory(history.state) ||
            projectOverviewFromHistory(resumeState));
        await refreshSnapshot(false);
      } catch (error) {
        if (isCurrent()) {
          if (
            !ticket &&
            publishedEntry &&
            error instanceof WorkbenchClientError &&
            error.retryable
          ) {
            retryBootstrap = () => void bootstrap();
          } else if (
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
      const restoredState = workbenchHistoryState(event.state);
      if (sessionKeyFromHistory(restoredState)) {
        retainTabResumeState(restoredState);
      }
      const restoredSessionKey = sessionKeyFromHistory(event.state);
      if (restoredSessionKey !== client?.sessionKey) {
        void bootstrap(undefined, restoredSessionKey);
      } else {
        const laneId = inspectedLaneFromHistory(event.state);
        if (laneId) {
          if (sessionRecovery === "connected") {
            void inspectLane(
              laneId,
              "none",
              true,
              true,
              bootstrapInspectionLoading,
            );
          } else {
            pendingRestoredLaneId = laneId;
            pendingRestoredLaneHistoryMode = "none";
            projectOverview = false;
          }
        } else if (projectOverviewFromHistory(event.state)) {
          openProjectOverview("none");
        } else {
          clearInspection("none");
        }
      }
    };
    const queuePairingEnrollmentRecovery = () => {
      if (pairingEnrollmentRecoveryQueued) {
        pairingEnrollmentRecoveryPending = true;
        return;
      }

      pairingEnrollmentRecoveryQueued = true;
      clearTabResumeState();
      replacePageState(
        `${location.pathname}${location.search}`,
        prepareWorkbenchTicketExchange(history.state),
      );
      const recovery = bootstrap(undefined, null);
      const recoveryGeneration = bootstrapGeneration;
      void recovery.finally(() => {
        const recoveryIsCurrent = recoveryGeneration === bootstrapGeneration;
        pairingEnrollmentRecoveryQueued = false;
        if (pairingEnrollmentRecoveryPending && recoveryIsCurrent) {
          pairingEnrollmentRecoveryPending = false;
          queuePairingEnrollmentRecovery();
        } else if (!recoveryIsCurrent) {
          pairingEnrollmentRecoveryPending = false;
        }
      });
    };
    const resumeAfterPairingEnrollment = (event: MessageEvent<unknown>) => {
      if (
        !publishedEntry ||
        typeof event.data !== "object" ||
        event.data === null ||
        (event.data as Record<string, unknown>).kind !==
          PAIRING_ENROLLED_NOTICE.kind ||
        (event.data as Record<string, unknown>).version !==
          PAIRING_ENROLLED_NOTICE.version ||
        (!pairingEnrollmentRecoveryQueued &&
          screen !== "loading" &&
          screen !== "session_required" &&
          screen !== "session_expired" &&
          sessionRecovery !== "needs_launch" &&
          sessionRecovery !== "reconnecting")
      ) {
        return;
      }

      queuePairingEnrollmentRecovery();
    };
    if (pairingEvents) {
      pairingEvents.onmessage = resumeAfterPairingEnrollment;
    }
    window.addEventListener("hashchange", bootstrapFreshTicket);
    window.addEventListener("popstate", bootstrapRestoredSession);
    queueMicrotask(() => void bootstrap());

    return () => {
      bootstrapGeneration += 1;
      retryBootstrap = null;
      startLiveUpdates = null;
      beginSessionRecovery = null;
      window.removeEventListener("hashchange", bootstrapFreshTicket);
      window.removeEventListener("popstate", bootstrapRestoredSession);
      pairingEvents?.close();
      clearRecoveryTimer();
      stopUpdates();
      stopLiveUpdates = null;
    };
  });

  async function refreshSnapshot(quiet: boolean): Promise<boolean> {
    if (!client || sessionRecovery !== "connected") {
      return false;
    }
    if (refreshing) {
      refreshQueued = true;
      return new Promise<boolean>((resolve) => {
        refreshIdleWaiters.push(resolve);
      });
    }

    refreshing = true;
    let applied = false;
    const refreshGeneration = snapshotRefreshGeneration;
    if (!quiet && snapshot === null) {
      screen = "loading";
    }
    const activeClient = client;
    try {
      const nextSnapshot = await activeClient.snapshot();
      if (
        client !== activeClient ||
        refreshGeneration !== snapshotRefreshGeneration
      ) {
        return false;
      }
      applySnapshot(nextSnapshot);
      applied = true;
    } catch (error) {
      if (
        client !== activeClient ||
        refreshGeneration !== snapshotRefreshGeneration
      ) {
        return false;
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
        if (
          error instanceof WorkbenchClientError &&
          error.kind === "client_update_required"
        ) {
          sessionRecovery = "reload_required";
          sessionRecoveryMessage = messageFrom(error);
          refreshFailure = null;
          stopLiveUpdates?.();
        } else {
          refreshFailure = messageFrom(error);
        }
        if (
          error instanceof WorkbenchClientError &&
          error.kind === "transport_error"
        ) {
          beginSessionRecovery?.();
        }
      }
    } finally {
      refreshing = false;
      if (refreshQueued) {
        refreshQueued = false;
        applied = await refreshSnapshot(true);
      }
      if (!refreshing && !refreshQueued) {
        const waiters = refreshIdleWaiters;
        refreshIdleWaiters = [];
        for (const resolve of waiters) {
          resolve(applied);
        }
      }
    }
    return applied;
  }

  function applySnapshot(nextSnapshot: WorkbenchSnapshot): void {
    const previousSnapshot = snapshot;
    const previousInspection = inspection;
    const requestedLaneId = inspectionRequestedLaneId;
    const requestedHistoryMode = inspectionRequestedHistoryMode;
    const retryLaneId = inspectionRetryLaneId;
    const retryHistoryMode = inspectionRetryHistoryMode;
    const snapshotChanged =
      previousSnapshot === null ||
      previousSnapshot.daemon.instance_id !== nextSnapshot.daemon.instance_id ||
      previousSnapshot.revision !== nextSnapshot.revision;
    snapshot = nextSnapshot;
    if (!bootstrapInspectionLoading) {
      screen = "ready";
    }
    screenMessage = null;
    screenRetryable = false;
    refreshFailure = null;
    if (
      completionReview &&
      (nextSnapshot.daemon.instance_id !==
        completionReview.expectedDaemonInstanceId ||
        nextSnapshot.revision !== completionReview.expectedRevision ||
        nextSnapshot.phase?.id !== completionReview.expectedPhaseId) &&
      !preparedPlanningApprovesReview(
        pendingPlanning,
        completionReview.review.review_id,
      ) &&
      !preparedPlanningApprovesReview(
        retryPlanning,
        completionReview.review.review_id,
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
    if (planningEditorRebindPending) {
      planningEditorRebindPending = false;
      planningEditorRebindToken += 1;
    }
    const confirmedFocusObserved =
      confirmedLocalFocus !== null &&
      nextSnapshot.focused_lane?.id === confirmedLocalFocus.laneId;
    if (confirmedFocusObserved) {
      confirmedLocalFocus = null;
      clearInspection("replace");
    } else {
      if (
        confirmedLocalFocus !== null &&
        (nextSnapshot.daemon.instance_id !==
          confirmedLocalFocus.daemonInstanceId ||
          nextSnapshot.revision > confirmedLocalFocus.priorRevision)
      ) {
        confirmedLocalFocus = null;
      }
      if (pendingRestoredLaneId && sessionRecovery === "connected") {
        const restoredLaneId = pendingRestoredLaneId;
        const restoredHistoryMode = pendingRestoredLaneHistoryMode;
        pendingRestoredLaneId = null;
        pendingRestoredLaneHistoryMode = "none";
        void inspectLane(
          restoredLaneId,
          restoredHistoryMode,
          true,
          true,
          bootstrapInspectionLoading,
        );
      } else if (requestedLaneId) {
        if (snapshotChanged) {
          void inspectLane(
            requestedLaneId,
            requestedHistoryMode,
            true,
            true,
            bootstrapInspectionLoading,
          );
        }
      } else if (retryLaneId && snapshotChanged) {
        void inspectLane(retryLaneId, retryHistoryMode, true, true);
      } else if (
        previousInspection &&
        (previousInspection.daemon.instance_id !==
          nextSnapshot.daemon.instance_id ||
          previousInspection.revision !== nextSnapshot.revision)
      ) {
        void inspectLane(previousInspection.lane.id, "none", true, true);
      }
    }
    startLiveUpdates?.();
  }

  type InspectionHistoryMode = "push" | "replace" | "none";

  function inspectedLaneFromHistory(state: unknown): string | null {
    const laneId = workbenchHistoryState(state)[INSPECTED_LANE_HISTORY_KEY];
    return typeof laneId === "string" && laneId.length > 0 ? laneId : null;
  }

  function projectOverviewFromHistory(state: unknown): boolean {
    return workbenchHistoryState(state)[PROJECT_OVERVIEW_HISTORY_KEY] === true;
  }

  function writeInspectionHistory(
    laneId: string | null,
    mode: Exclude<InspectionHistoryMode, "none">,
  ): void {
    const nextState = { ...workbenchHistoryState(history.state) };
    if (laneId) {
      nextState[INSPECTED_LANE_HISTORY_KEY] = laneId;
    } else {
      delete nextState[INSPECTED_LANE_HISTORY_KEY];
    }
    delete nextState[PROJECT_OVERVIEW_HISTORY_KEY];
    if (mode === "push") {
      pushPageState("", nextState);
    } else {
      replacePageState("", nextState);
    }
    retainTabResumeState(nextState);
  }

  function finishBootstrapInspection(
    requestGeneration: number | null = null,
  ): void {
    if (
      !bootstrapInspectionLoading ||
      (requestGeneration !== null &&
        bootstrapInspectionRequestGeneration !== requestGeneration)
    ) {
      return;
    }
    bootstrapInspectionRequestGeneration = null;
    bootstrapInspectionLoading = false;
    if (screen !== "loading" || !snapshot) {
      return;
    }
    screen =
      sessionRecovery === "reload_required"
        ? "client_update_required"
        : "ready";
  }

  function clearInspection(mode: InspectionHistoryMode = "push"): void {
    const currentHistory = workbenchHistoryState(history.state);
    const historyAlreadyShowsCurrentWork =
      inspectedLaneFromHistory(currentHistory) === null &&
      !projectOverviewFromHistory(currentHistory);
    inspectionRequestGeneration += 1;
    inspection = null;
    inspectionLoading = false;
    inspectionFailure = null;
    inspectionRetryLaneId = null;
    inspectionRetryHistoryMode = "none";
    inspectionRequestedLaneId = null;
    inspectionRequestedHistoryMode = "none";
    pendingRestoredLaneId = null;
    pendingRestoredLaneHistoryMode = "none";
    projectOverview = false;
    finishBootstrapInspection();
    if (
      mode !== "none" &&
      !(mode === "push" && historyAlreadyShowsCurrentWork)
    ) {
      writeInspectionHistory(null, mode);
    }
  }

  function openProjectOverview(
    mode: InspectionHistoryMode = "push",
  ): void {
    inspectionRequestGeneration += 1;
    inspection = null;
    inspectionLoading = false;
    inspectionFailure = null;
    inspectionRetryLaneId = null;
    inspectionRetryHistoryMode = "none";
    inspectionRequestedLaneId = null;
    inspectionRequestedHistoryMode = "none";
    pendingRestoredLaneId = null;
    pendingRestoredLaneHistoryMode = "none";
    projectOverview = true;
    finishBootstrapInspection();
    if (mode === "none") {
      return;
    }
    const nextState = { ...workbenchHistoryState(history.state) };
    delete nextState[INSPECTED_LANE_HISTORY_KEY];
    nextState[PROJECT_OVERVIEW_HISTORY_KEY] = true;
    if (mode === "push") {
      pushPageState("", nextState);
    } else {
      replacePageState("", nextState);
    }
    retainTabResumeState(nextState);
  }

  async function inspectLane(
    laneId: string,
    historyMode: InspectionHistoryMode = "push",
    retryOnStale = true,
    preserveFocusedSelection = false,
    bootstrapLoading = false,
  ): Promise<void> {
    if (!client || !snapshot || sessionRecovery !== "connected") {
      return;
    }
    if (laneId === snapshot.focused_lane?.id && !preserveFocusedSelection) {
      clearInspection(historyMode);
      return;
    }

    const activeClient = client;
    const requestGeneration = ++inspectionRequestGeneration;
    if (bootstrapLoading) {
      bootstrapInspectionRequestGeneration = requestGeneration;
    }
    inspectionRequestedLaneId = laneId;
    inspectionRequestedHistoryMode = historyMode;
    inspectionLoading = true;
    inspectionFailure = null;
    inspectionRetryLaneId = null;
    inspectionRetryHistoryMode = "none";
    try {
      const nextInspection = await activeClient.inspectLane(laneId);
      if (
        client !== activeClient ||
        requestGeneration !== inspectionRequestGeneration
      ) {
        return;
      }
      if (
        nextInspection.daemon.instance_id !== snapshot.daemon.instance_id ||
        nextInspection.revision !== snapshot.revision
      ) {
        if (retryOnStale) {
          await refreshSnapshot(true);
          if (requestGeneration === inspectionRequestGeneration) {
            await inspectLane(
              laneId,
              historyMode,
              false,
              true,
              bootstrapLoading,
            );
          }
        } else {
          inspectionFailure =
            "This lane changed while it was opening. Retry from the current project view.";
          inspectionRetryLaneId = laneId;
          inspectionRetryHistoryMode = historyMode;
        }
        return;
      }
      inspection = nextInspection;
      inspectionRetryLaneId = null;
      inspectionRetryHistoryMode = "none";
      projectOverview = false;
      if (historyMode !== "none") {
        writeInspectionHistory(laneId, historyMode);
      }
    } catch (error) {
      if (
        client !== activeClient ||
        requestGeneration !== inspectionRequestGeneration
      ) {
        return;
      }
      if (
        error instanceof WorkbenchClientError &&
        error.detailKind === "workbench.lane_not_found"
      ) {
        openProjectOverview("replace");
        inspectionFailure =
          "That lane is no longer part of the current project plan.";
        inspectionRetryLaneId = null;
        inspectionRetryHistoryMode = "none";
      } else if (
        error instanceof WorkbenchClientError &&
        error.kind === "client_update_required"
      ) {
        inspectionFailure = null;
        inspectionRetryLaneId = null;
        inspectionRetryHistoryMode = "none";
        sessionRecovery = "reload_required";
        sessionRecoveryMessage = messageFrom(error);
        refreshFailure = null;
        stopLiveUpdates?.();
        if (bootstrapLoading || screen === "loading") {
          screen = "client_update_required";
          screenMessage = sessionRecoveryMessage;
        }
      } else if (terminalFailure(error)) {
        pendingRestoredLaneId = laneId;
        pendingRestoredLaneHistoryMode = historyMode;
        projectOverview = false;
        applyTerminalFailure(error);
        if (bootstrapLoading) {
          screen = "loading";
        }
      } else {
        inspectionFailure = messageFrom(error);
        inspectionRetryLaneId = laneId;
        inspectionRetryHistoryMode = historyMode;
      }
    } finally {
      if (requestGeneration === inspectionRequestGeneration) {
        inspectionRequestedLaneId = null;
        inspectionRequestedHistoryMode = "none";
        inspectionLoading = false;
      }
      if (
        bootstrapLoading &&
        !(
          pendingRestoredLaneId === laneId &&
          sessionRecovery !== "connected"
        )
      ) {
        finishBootstrapInspection(requestGeneration);
      }
    }
  }

  function selectLane(laneId: string): void {
    void inspectLane(laneId);
  }

  async function focusInspectedLane(laneId: string): Promise<void> {
    await focusLane(laneId);
  }

  function retryInspection(): void {
    if (inspectionRetryLaneId) {
      const laneId = inspectionRetryLaneId;
      const historyMode = inspectionRetryHistoryMode;
      void inspectLane(laneId, historyMode, true, true);
    }
  }

  function preparedPlanningApprovesReview(
    prepared: PreparedPlanningRequest | null,
    reviewId: string,
  ): boolean {
    return (
      prepared?.request.operation.kind === "task_complete_approve" &&
      prepared.request.operation.review_id === reviewId
    );
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
      if (client !== activeClient) {
        return;
      }
      if (snapshot) {
        confirmedLocalFocus = {
          laneId,
          daemonInstanceId: snapshot.daemon.instance_id,
          priorRevision: snapshot.revision,
        };
      }
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
    binding?: WorkbenchPlanningBinding,
  ): PreparedPlanningRequest | null {
    const resolvedBinding =
      binding ?? (snapshot ? workbenchPlanningBinding(snapshot) : null);
    if (!client || !resolvedBinding || sessionRecovery !== "connected") {
      return null;
    }
    return {
      request: {
        protocol_version: 2,
        id: createWorkbenchRequestId(),
        session_key: client.sessionKey,
        expected_daemon_instance_id:
          resolvedBinding.expected_daemon_instance_id,
        expected_revision: resolvedBinding.expected_revision,
        expected_phase_id: resolvedBinding.expected_phase_id,
        operation,
      },
    };
  }

  async function submitPlanning(
    operation: WorkbenchPlanningOperation,
    binding?: WorkbenchPlanningBinding,
  ): Promise<boolean> {
    const prepared = preparePlanningRequest(operation, binding);
    return prepared ? executePlanning(prepared) : false;
  }

  async function executePlanning(
    prepared: PreparedPlanningRequest,
  ): Promise<boolean> {
    if (!client || pendingPlanning) {
      return false;
    }
    if (
      retryPlanning !== null &&
      retryPlanning.request.id !== prepared.request.id
    ) {
      return false;
    }

    const activeClient = client;
    pendingPlanning = prepared;
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
      retryPlanning = null;
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
        } else {
          retryPlanning = null;
        }
        const shouldRefreshPlanningContext =
          error instanceof WorkbenchClientError &&
          (error.detailKind === "workbench.stale_snapshot" ||
            error.detailKind === "workbench.phase_mismatch" ||
            error.detailKind === "workbench.review_invalid");
        if (shouldRefreshPlanningContext) {
          completionReview = null;
          const applied = await refreshSnapshot(true);
          if (
            error instanceof WorkbenchClientError &&
            error.detailKind === "workbench.stale_snapshot"
          ) {
            const recoveredBinding =
              snapshot !== null &&
              (snapshot.daemon.instance_id !==
                prepared.request.expected_daemon_instance_id ||
                snapshot.revision !== prepared.request.expected_revision ||
                snapshot.phase?.id !== prepared.request.expected_phase_id);
            if (applied || recoveredBinding) {
              planningEditorRebindToken += 1;
            } else {
              planningEditorRebindPending = true;
            }
          }
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
        task_id: completionReview.review.task_id,
        outcome: completionReview.review.proposed_outcome,
      },
      {
        expected_daemon_instance_id:
          completionReview.expectedDaemonInstanceId,
        expected_revision: completionReview.expectedRevision,
        expected_phase_id: completionReview.expectedPhaseId,
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
    if (retryBootstrap) {
      retryBootstrap?.();
    } else if (client) {
      void refreshSnapshot(false);
    }
  }

  function reloadWorkbench(): void {
    window.location.reload();
  }

  async function loadPairings(): Promise<void> {
    const activeClient = client;
    if (!pairingAvailable || !activeClient || pairingsLoading) {
      return;
    }
    pairingsLoading = true;
    pairingFailure = null;
    try {
      const result = await activeClient.pairings();
      if (client === activeClient) {
        pairings = result.pairings;
      }
    } catch (error) {
      if (client !== activeClient) {
        return;
      }
      pairingFailure = messageFrom(error);
      if (
        error instanceof WorkbenchClientError &&
        error.kind === "session_expired"
      ) {
        beginSessionRecovery?.();
      }
    } finally {
      if (client === activeClient) {
        pairingsLoading = false;
      }
    }
  }

  async function revokePairing(selector: string): Promise<void> {
    const activeClient = client;
    if (!activeClient || pendingPairingSelector !== null) {
      return;
    }
    pendingPairingSelector = selector;
    pairingFailure = null;
    try {
      await activeClient.revokePairing(selector);
      if (client === activeClient) {
        pairings = null;
        await loadPairings();
      }
    } catch (error) {
      if (client === activeClient) {
        pairingFailure = messageFrom(error);
      }
    } finally {
      if (client === activeClient) {
        pendingPairingSelector = null;
      }
    }
  }

  async function renamePairing(selector: string, nickname: string): Promise<void> {
    const activeClient = client;
    if (!activeClient || pendingPairingSelector !== null) {
      return;
    }
    pendingPairingSelector = selector;
    pairingFailure = null;
    try {
      await activeClient.renamePairing(selector, nickname);
      if (client === activeClient) {
        pairings = pairings?.map((pairing) =>
          pairing.selector === selector ? { ...pairing, nickname } : pairing,
        ) ?? null;
      }
    } catch (error) {
      if (client === activeClient) {
        pairingFailure = messageFrom(error);
      }
    } finally {
      if (client === activeClient && pendingPairingSelector === selector) {
        pendingPairingSelector = null;
      }
    }
  }

  async function forgetCurrentPairing(): Promise<void> {
    const activeClient = client;
    if (!activeClient || pendingPairingSelector !== null) {
      return;
    }
    const currentSelector =
      pairings?.find((pairing) => pairing.current)?.selector ?? "current";
    pendingPairingSelector = currentSelector;
    pairingFailure = null;
    try {
      await activeClient.forgetPairing();
      if (client !== activeClient) {
        return;
      }
      stopLiveUpdates?.();
      client = null;
      snapshot = null;
      pairings = null;
      clearTabResumeState();
      replacePageState(`${location.pathname}${location.search}`, {});
      screen = "session_expired";
      screenMessage =
        "This browser no longer has access to this workspace. Open a current enrollment link to pair it again.";
      screenRetryable = false;
    } catch (error) {
      if (client === activeClient) {
        pairingFailure = messageFrom(error);
      }
    } finally {
      if (
        client === activeClient &&
        pendingPairingSelector === currentSelector
      ) {
        pendingPairingSelector = null;
      }
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
      case "client_update_required":
        return "client_update_required";
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
          body:
            screenMessage ?? "Open a fresh Exo workbench link to continue.",
        };
      case "client_update_required":
        return {
          title: "Workbench update available",
          body:
            screenMessage ??
            "Reload this page to use the current Exo workbench version.",
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
    {inspection}
    {inspectionLoading}
    inspectionLaneId={inspectionRequestedLaneId}
    {inspectionFailure}
    {projectOverview}
    {refreshing}
    {streamConnected}
    {sessionRecovery}
    {sessionRecoveryMessage}
    {pairingAvailable}
    {pairings}
    {pairingsLoading}
    {pairingFailure}
    {pendingPairingSelector}
    pendingLaneId={pendingFocus?.laneId ?? null}
    {focusFailure}
    {refreshFailure}
    planningFailure={planningFailure}
    {planningNotice}
    {planningSuccess}
    {planningEditorRebindToken}
    pendingPlanningKind={pendingPlanning?.request.operation.kind ?? null}
    completionReview={completionReview?.review ?? null}
    onInspect={selectLane}
    onOpenProject={() => openProjectOverview()}
    onCloseProject={() => clearInspection()}
    onCloseInspection={() => clearInspection()}
    onRetryInspection={inspectionFailure && inspectionRetryLaneId
      ? retryInspection
      : null}
    onFocus={(laneId) => void focusInspectedLane(laneId)}
    onRetryFocus={retryFocus ? retryPendingFocus : null}
    onRetryPlanning={retryPlanning ? retryPendingPlanning : null}
    onRetrySession={sessionRecovery === "reconnecting"
      ? () => beginSessionRecovery?.()
      : sessionRecovery === "reload_required"
        ? reloadWorkbench
        : null}
    onRefresh={() => void refreshSnapshot(false)}
    onOpenPairings={() => void loadPairings()}
    onRetryPairings={() => void loadPairings()}
    onRevokePairing={(selector) => void revokePairing(selector)}
    onRenamePairing={(selector, nickname) => void renamePairing(selector, nickname)}
    onForgetPairing={() => void forgetCurrentPairing()}
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
        {:else if screen === "client_update_required"}
          <RefreshCw size={24} />
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
      {#if screen === "client_update_required"}
        <button type="button" onclick={reloadWorkbench}>
          <RefreshCw size={16} aria-hidden="true" />
          Reload
        </button>
      {:else if screenRetryable}
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

  :global(svg.spin) {
    transform-box: fill-box;
    transform-origin: center;
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(svg.spin) {
      animation: none;
    }
  }
</style>
