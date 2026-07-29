<script lang="ts">
  import {
    Activity,
    AlertTriangle,
    Check,
    CheckCircle2,
    Circle,
    CircleDashed,
    CircleDot,
    CirclePlay,
    Compass,
    GitBranch,
    GitCommitHorizontal,
    Info,
    Layers3,
    ListTodo,
    LoaderCircle,
    RefreshCw,
    Route,
    Target,
    Wifi,
    WifiOff,
    XCircle,
  } from "@lucide/svelte";

  import type {
    WorkbenchDiagnostic,
    WorkbenchLaneSummary,
    WorkbenchSnapshot,
  } from "./workbench";

  interface Props {
    snapshot: WorkbenchSnapshot;
    refreshing?: boolean;
    streamConnected?: boolean;
    pendingLaneId?: string | null;
    focusFailure?: string | null;
    onFocus: (laneId: string) => void;
    onRetryFocus?: (() => void) | null;
    onRefresh: () => void;
  }

  let {
    snapshot,
    refreshing = false,
    streamConnected = false,
    pendingLaneId = null,
    focusFailure = null,
    onFocus,
    onRetryFocus = null,
    onRefresh,
  }: Props = $props();

  let agentNextStep = $derived(snapshot.steering.next_actions[0] ?? null);
  let hasCoordination = $derived(
    agentNextStep !== null || snapshot.diagnostics.length > 0,
  );

  const shortHead = (head: string | null): string =>
    head ? head.slice(0, 8) : "unborn";

  const displayStatus = (status: string): string =>
    status.replaceAll("-", " ");

  const statusTone = (status: string): "complete" | "active" | "pending" => {
    const normalized = status.toLowerCase();
    if (
      normalized.includes("complete") ||
      normalized.includes("done") ||
      normalized.includes("closed")
    ) {
      return "complete";
    }
    if (
      normalized.includes("progress") ||
      normalized.includes("execut") ||
      normalized.includes("active")
    ) {
      return "active";
    }
    return "pending";
  };

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
        class:connected={streamConnected}
        class="connection"
        title={streamConnected
          ? "Live updates connected"
          : "Live updates reconnecting; polling remains active"}
      >
        {#if streamConnected}
          <Wifi size={14} aria-hidden="true" />
          Live
        {:else}
          <WifiOff size={14} aria-hidden="true" />
          Polling
        {/if}
      </span>
      <button
        class="icon-button"
        type="button"
        title="Refresh workbench"
        aria-label="Refresh workbench"
        disabled={refreshing}
        onclick={onRefresh}
      >
        <RefreshCw class={refreshing ? "spin" : undefined} size={17} aria-hidden="true" />
      </button>
    </div>
  </header>

  {#if focusFailure}
    <div class="failure-banner" role="alert">
      <AlertTriangle size={18} aria-hidden="true" />
      <span>{focusFailure}</span>
      {#if onRetryFocus}
        <button type="button" onclick={onRetryFocus}>Retry</button>
      {/if}
    </div>
  {/if}

  <div class:has-coordination={hasCoordination} class="workspace-grid">
    <aside class="lane-rail" aria-label="Project lanes">
      <div class="rail-heading">
        <div>
          <span class="section-kicker">Project</span>
          <h2>Lanes</h2>
        </div>
        <span class="count" aria-label={`${snapshot.lanes.length} lanes`}>
          {snapshot.lanes.length}
        </span>
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
              disabled={pendingLaneId !== null ||
                lane.focused_here ||
                !lanePhaseActive(lane)}
              onclick={() => onFocus(lane.id)}
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

          <div class="goal-list">
            {#each snapshot.phase.goals as goal (goal.id)}
              <article class="goal">
                <div class="goal-heading">
                  <span class={`status-icon ${statusTone(goal.status)}`} aria-hidden="true">
                    {#if statusTone(goal.status) === "complete"}
                      <CheckCircle2 size={18} />
                    {:else if statusTone(goal.status) === "active"}
                      <CircleDot size={18} />
                    {:else}
                      <Circle size={18} />
                    {/if}
                  </span>
                  <div>
                    <h3>{goal.title}</h3>
                    <span>{displayStatus(goal.status)}</span>
                  </div>
                </div>

                {#if goal.tasks.length > 0}
                  <ul class="task-list">
                    {#each goal.tasks as task (task.id)}
                      <li>
                        <span class={`task-check ${statusTone(task.status)}`} aria-hidden="true">
                          {#if statusTone(task.status) === "complete"}
                            <Check size={13} />
                          {:else if statusTone(task.status) === "active"}
                            <CircleDot size={13} />
                          {:else}
                            <Circle size={13} />
                          {/if}
                        </span>
                        <span>{task.title}</span>
                        <small>{displayStatus(task.status)}</small>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </article>
            {/each}
          </div>
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
  }

  .lane-intent {
    max-width: 820px;
    margin-top: 12px;
    color: #34413f;
    font-size: 1rem;
    line-height: 1.55;
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
    padding: 18px 0;
    border-bottom: 1px solid var(--line);
  }

  .goal-heading {
    justify-content: flex-start;
  }

  .status-icon {
    margin-top: 1px;
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

  .goal-heading h3 {
    font-size: 0.86rem;
    line-height: 1.3;
  }

  .goal-heading span:last-child {
    display: block;
    margin-top: 3px;
    color: var(--muted);
    font-size: 0.68rem;
    text-transform: capitalize;
  }

  .task-list {
    margin: 13px 0 0 30px;
    padding: 0;
    list-style: none;
  }

  .task-list li {
    min-height: 34px;
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    align-items: center;
    gap: 7px;
    border-top: 1px solid #e3e8e6;
    font-size: 0.77rem;
  }

  .task-list small {
    color: var(--muted);
    font-size: 0.63rem;
    text-transform: capitalize;
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
      overflow-x: auto;
      padding-bottom: 2px;
    }

    .topbar-actions {
      grid-column: 2;
      grid-row: 1;
    }

    .workspace-grid {
      display: flex;
      flex-direction: column;
    }

    .lane-rail {
      border-right: 0;
      border-bottom: 1px solid var(--line);
      padding: 12px;
    }

    .rail-heading {
      padding-bottom: 9px;
    }

    .lane-list {
      display: flex;
      gap: 7px;
      overflow-x: auto;
      padding-bottom: 2px;
      scroll-snap-type: x proximity;
    }

    .lane-row {
      min-width: min(260px, 78vw);
      scroll-snap-align: start;
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
