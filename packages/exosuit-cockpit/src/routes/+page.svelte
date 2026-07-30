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
  import type { WorkbenchSnapshot } from "$lib/workbench";
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
    | "workspace_unavailable"
    | "transport_error";

  interface FocusRequest {
    laneId: string;
    requestId: string;
  }

  let screen = $state<ScreenState>("loading");
  let snapshot = $state<WorkbenchSnapshot | null>(null);
  let refreshing = $state(false);
  let streamConnected = $state(false);
  let pendingFocus = $state<FocusRequest | null>(null);
  let retryFocus = $state<FocusRequest | null>(null);
  let focusFailure = $state<string | null>(null);
  let refreshFailure = $state<string | null>(null);
  let screenMessage = $state<string | null>(null);
  let retryBootstrap = $state<(() => void) | null>(null);

  let client: WorkbenchClient | null = null;
  let ambiguousFocus: FocusRequest | null = null;
  let refreshQueued = false;
  let startLiveUpdates: (() => void) | null = null;
  let stopLiveUpdates: (() => void) | null = null;

  onMount(() => {
    let events: EventSource | null = null;
    let pollTimer: number | null = null;
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
      document.removeEventListener("visibilitychange", refreshForVisibility);
      window.removeEventListener("focus", refreshForFocus);
      liveUpdatesStarted = false;
      streamConnected = false;
    };
    startLiveUpdates = startUpdates;
    stopLiveUpdates = stopUpdates;

    const bootstrap = async (retryTicket?: string) => {
      const generation = ++bootstrapGeneration;
      const isCurrent = () => generation === bootstrapGeneration;
      const ticket = retryTicket ?? launchTicketFromHash(location.hash);
      screen = "loading";
      screenMessage = null;
      retryBootstrap = null;
      try {
        let sessionKey = sessionKeyFromHistory(history.state);
        if (ticket) {
          stopUpdates();
          client = null;
          snapshot = null;
          pendingFocus = null;
          retryFocus = null;
          ambiguousFocus = null;
          focusFailure = null;
          refreshFailure = null;
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
    window.addEventListener("hashchange", bootstrapFreshTicket);
    void bootstrap();

    return () => {
      bootstrapGeneration += 1;
      retryBootstrap = null;
      startLiveUpdates = null;
      window.removeEventListener("hashchange", bootstrapFreshTicket);
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
      snapshot = nextSnapshot;
      screen = "ready";
      screenMessage = null;
      refreshFailure = null;
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
    } catch (error) {
      if (client !== activeClient) {
        return;
      }
      if (terminalFailure(error)) {
        applyTerminalFailure(error);
      } else if (snapshot === null) {
        screen = "transport_error";
        screenMessage = messageFrom(error);
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

  async function focusLane(
    laneId: string,
    requestId = createWorkbenchRequestId(),
  ): Promise<void> {
    if (!client || pendingFocus) {
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

  function applyTerminalFailure(error: unknown): void {
    const kind =
      error instanceof WorkbenchClientError ? error.kind : "transport_error";
    screen = screenForFailure(kind);
    screenMessage = messageFrom(error);
    stopLiveUpdates?.();
  }

  function retryPendingFocus(): void {
    if (retryFocus) {
      void focusLane(retryFocus.laneId, retryFocus.requestId);
    }
  }

  function retryTransport(): void {
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
    pendingLaneId={pendingFocus?.laneId ?? null}
    focusFailure={focusFailure ?? refreshFailure}
    onFocus={(laneId) => void focusLane(laneId)}
    onRetryFocus={retryFocus ? retryPendingFocus : null}
    onRefresh={() => void refreshSnapshot(false)}
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
        {:else if screen === "workspace_unavailable"}
          <FolderX size={24} />
        {:else}
          <WifiOff size={24} />
        {/if}
      </div>
      <h1 id="state-title">{content.title}</h1>
      <p>{content.body}</p>
      {#if screen === "transport_error" || screen === "workbench_busy"}
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
