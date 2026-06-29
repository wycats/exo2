<script lang="ts">
  import { dashboardService } from "./DashboardService.svelte";
  import { useRoot } from "../services/ConsistencyService.svelte";
  import { vscode } from "./vscode";

  // Pull-Based Reactivity
  let loaded = $derived(dashboardService.isLoaded);
  let mode = $derived(dashboardService.mode);
  let phase = $derived(dashboardService.currentPhase);
  let rfcs = $derived(dashboardService.rfcs);
  let feedback = $derived(dashboardService.feedback);

  // Debug Reactivity
  let testRoot = useRoot("test.root");

  // Welcome State
  let mission = $state("");
  let selectedMode = $state("pair-programmer");
  let firstStep = $state("");

  function initProject() {
    vscode.postMessage({
      type: "INIT_PROJECT",
      payload: { mission, mode: selectedMode, firstStep },
    });
  }

  // Group RFCs by stage
  let groupedRfcs = $derived.by(() => {
    const groups: Record<string, any[]> = {};
    for (const rfc of rfcs) {
      if (!groups[rfc.stage]) {
        groups[rfc.stage] = [];
      }
      groups[rfc.stage].push(rfc);
    }
    // Sort stages reverse order (Stage 4 -> Stage 0)
    return Object.entries(groups).sort((a, b) => b[0].localeCompare(a[0]));
  });

  function formatDate(isoString: string) {
    return new Date(isoString).toLocaleDateString();
  }

  function openFile(arg: string | { path: string; uri?: string }) {
    const payload =
      typeof arg === "string"
        ? { path: arg }
        : { path: arg.path, uri: arg.uri };
    vscode.postMessage({
      type: "OPEN_FILE",
      payload,
    });
  }

  $effect(() => {
    console.log("[App] Render state:", { loaded, mode, phase: phase?.title });
  });
</script>

<main>
  {#if loaded}
    {#if mode === "welcome"}
      <div class="welcome-container">
        <h1>Welcome to Exosuit</h1>
        <p class="subtitle">Let's set up your workspace.</p>

        <div class="form-group">
          <label for="mission">Mission</label>
          <input
            type="text"
            id="mission"
            bind:value={mission}
            placeholder="e.g. Build a reactive web framework"
          />
        </div>

        <div class="form-group">
          <label for="mode">Agent Mode</label>
          <select id="mode" bind:value={selectedMode}>
            <option value="pair-programmer"
              >Pair Programmer (Collaborative)</option
            >
            <option value="co-founder">Co-Founder (Strategic)</option>
            <option value="intern">Intern (Task-Focused)</option>
          </select>
        </div>

        <div class="form-group">
          <label for="firstStep">First Step</label>
          <input
            type="text"
            id="firstStep"
            bind:value={firstStep}
            placeholder="e.g. Create the initial repository structure"
          />
        </div>

        <button class="primary-button" onclick={initProject}
          >Initialize Workspace</button
        >
      </div>
    {:else}
      <!-- Current Phase Section -->
      <div class="section">
        <div class="section-header">CURRENT PHASE</div>
        {#if phase}
          <div class="phase-card">
            <div class="phase-icon codicon codicon-rocket"></div>
            <div class="phase-content">
              <div class="phase-title">{phase.title}</div>
              <div class="phase-id">{phase.phaseId}</div>
            </div>
          </div>
        {:else}
          <div class="empty-state">No active phase</div>
        {/if}
      </div>

      <!-- Feedback Section -->
      <div class="section">
        <div class="section-header">FEEDBACK</div>
        {#if feedback.length > 0}
          <div class="list">
            {#each feedback as thread}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="list-item feedback-item"
                onclick={() => openFile(thread.target_file)}
              >
                <div class="feedback-header">
                  <span class="status-dot status-{thread.status}"></span>
                  <span class="target-id"
                    >{thread.target_id || thread.target_file}</span
                  >
                  <span class="timestamp">{formatDate(thread.updated_at)}</span>
                </div>
                {#if thread.messages.length > 0}
                  <div class="message-preview">
                    {thread.messages[thread.messages.length - 1].content}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {:else}
          <div class="empty-state">No feedback threads</div>
        {/if}
      </div>

      <!-- RFCs Section -->
      <div class="section">
        <div class="section-header">RFCs</div>
        {#if rfcs.length > 0}
          <div class="list">
            {#each groupedRfcs as [stage, stageRfcs]}
              <div class="group-header">
                {stage.replace("stage-", "Stage ")}
                <span class="count-badge">{stageRfcs.length}</span>
              </div>
              {#each stageRfcs as rfc}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="list-item rfc-item" onclick={() => openFile(rfc)}>
                  <div class="rfc-main">
                    <span class="rfc-dots">{rfc.stageDots}</span>
                    <span class="rfc-id">{rfc.formattedId || rfc.number}</span>
                    <span class="rfc-title">{rfc.title}</span>
                  </div>
                </div>
              {/each}
            {/each}
          </div>
        {:else}
          <div class="empty-state">No RFCs found</div>
        {/if}
      </div>
    {/if}
  {:else}
    <div class="loading">
      <span class="codicon codicon-loading codicon-modifier-spin"></span>
      Loading context...
    </div>
  {/if}

  <!-- Debug Reactivity Section -->
  <div class="section">
    <div class="section-header">DEBUG REACTIVITY</div>
    <div class="list-item">
      <button
        onclick={() => vscode.postMessage({ type: "REGISTER_TEST_ROOT" })}
      >
        Register Test Root
      </button>
      <div id="debug-root-status">Status: {testRoot.status}</div>
      <div id="debug-root-value">Value: {JSON.stringify(testRoot.value)}</div>
    </div>
  </div>
</main>

<style>
  main {
    padding: 0;
    font-family: var(--vscode-font-family, sans-serif);
    color: var(--vscode-foreground);
    font-size: 13px;
  }

  .section {
    margin-bottom: 0;
  }

  .section-header {
    font-size: 11px;
    font-weight: bold;
    text-transform: uppercase;
    color: var(--vscode-sideBarSectionHeader-foreground);
    background-color: var(--vscode-sideBarSectionHeader-background);
    padding: 4px 16px 4px 8px;
    border-top: 1px solid var(--vscode-sideBarSectionHeader-border);
    display: flex;
    align-items: center;
  }

  .group-header {
    font-size: 11px;
    font-weight: 600;
    color: var(--vscode-descriptionForeground);
    background-color: var(
      --vscode-list-hoverBackground
    ); /* Subtle separation */
    padding: 2px 16px 2px 8px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--vscode-panel-border);
  }

  .count-badge {
    background-color: var(--vscode-badge-background);
    color: var(--vscode-badge-foreground);
    border-radius: 8px;
    padding: 0 6px;
    font-size: 10px;
  }

  .empty-state {
    padding: 12px 16px;
    color: var(--vscode-descriptionForeground);
    font-style: italic;
    font-size: 0.9em;
    border-bottom: 1px solid var(--vscode-panel-border);
  }

  /* Phase Card */
  .phase-card {
    display: flex;
    align-items: center;
    padding: 12px 16px;
    background-color: var(--vscode-editor-inactiveSelectionBackground);
    border-bottom: 1px solid var(--vscode-panel-border);
  }

  .phase-icon {
    font-size: 16px;
    margin-right: 12px;
    color: var(--vscode-textLink-foreground);
  }

  .phase-title {
    font-weight: 600;
    font-size: 1.1em;
  }

  .phase-id {
    font-size: 0.9em;
    color: var(--vscode-descriptionForeground);
    margin-top: 2px;
  }

  /* Lists */
  .list {
    display: flex;
    flex-direction: column;
  }

  .list-item {
    padding: 3px 16px 3px 8px; /* Reduced padding for density */
    border-bottom: 1px solid var(--vscode-tree-tableOddRowsBackground); /* Lighter border */
    cursor: pointer;
    transition: background-color 0.1s;
  }

  .list-item:hover {
    background-color: var(--vscode-list-hoverBackground);
  }

  /* Feedback Items */
  .feedback-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 2px;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-right: 8px;
    display: inline-block;
    flex-shrink: 0;
  }

  .status-open {
    background-color: var(--vscode-charts-blue);
  }
  .status-resolved {
    background-color: var(--vscode-charts-green);
  }
  .status-proposed-resolved {
    background-color: var(--vscode-charts-yellow);
  }

  .target-id {
    font-weight: 600;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--vscode-foreground);
  }

  .timestamp {
    font-size: 0.85em;
    color: var(--vscode-descriptionForeground);
    margin-left: 8px;
    flex-shrink: 0;
  }

  .message-preview {
    font-size: 0.9em;
    color: var(--vscode-descriptionForeground);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-left: 16px; /* Indent to align with text above */
  }

  /* RFC Items */
  .rfc-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    height: 22px; /* Reduced height */
  }

  .rfc-main {
    display: flex;
    align-items: center;
    overflow: hidden;
    flex: 1;
    margin-right: 8px;
  }

  .rfc-dots {
    font-family: var(--vscode-editor-font-family);
    color: var(--vscode-descriptionForeground);
    margin-right: 6px;
    font-size: 0.85em;
    flex-shrink: 0;
    letter-spacing: -1px; /* Tighten dot spacing */
  }

  .rfc-id {
    font-family: var(--vscode-editor-font-family);
    color: var(--vscode-textLink-foreground);
    margin-right: 8px;
    font-size: 0.9em;
    flex-shrink: 0;
    min-width: 36px; /* Align titles */
  }

  .rfc-title {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .loading {
    padding: 20px;
    text-align: center;
    color: var(--vscode-descriptionForeground);
  }

  /* Welcome UI */
  .welcome-container {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .welcome-container h1 {
    font-size: 1.5em;
    margin: 0;
    font-weight: 600;
  }

  .subtitle {
    color: var(--vscode-descriptionForeground);
    margin: 0;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .form-group label {
    font-weight: 600;
    font-size: 0.9em;
  }

  input,
  select {
    background: var(--vscode-input-background);
    color: var(--vscode-input-foreground);
    border: 1px solid var(--vscode-input-border);
    padding: 6px;
    border-radius: 2px;
  }

  .primary-button {
    background: var(--vscode-button-background);
    color: var(--vscode-button-foreground);
    border: none;
    padding: 8px 16px;
    cursor: pointer;
    font-weight: 600;
    margin-top: 8px;
  }

  .primary-button:hover {
    background: var(--vscode-button-hoverBackground);
  }
</style>
