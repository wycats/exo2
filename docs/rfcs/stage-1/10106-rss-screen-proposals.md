<!-- exo:10106 ulid:01kmzxey29za76rg3vrpp43sqd -->


# RFC 10106: RSS Screen Proposals

This document proposes the layout and styling for the Axiom, Decision, and Plan editors using **Svelte + RSS (RTD Surface Syntax)** styled with **RSL (RTD Style Language)**.

## Shared RSL Tokens

We assume the following semantic tokens are available in our registry (extending the default set):

| Token    | Property  | Meaning                                                  |
| :------- | :-------- | :------------------------------------------------------- |
| `page`   | `layout`  | `mode stack, gap lg` (Vertical stack with large gaps)    |
| `card`   | `surface` | `base surface-1` (Primary background)                    |
| `card`   | `border`  | `style subtle, radius md` (Subtle border, medium radius) |
| `card`   | `spacing` | `all lg` (Large internal padding)                        |
| `header` | `text`    | `size xl, weight bold`                                   |
| `meta`   | `text`    | `size sm` (Small text for metadata)                      |
| `meta`   | `surface` | `base transparent`                                       |
| `badge`  | `surface` | `base surface-2`                                         |
| `badge`  | `border`  | `radius sm`                                              |
| `badge`  | `spacing` | `y xs, x sm` (Tiny vertical, small horizontal)           |
| `badge`  | `text`    | `size xs, weight bold`                                   |

## 1. Axiom Editor

The Axiom Editor manages the fundamental truths of the project.

### Layout Strategy

- **Header**: Title and "Add Axiom" action.
- **List**: Vertical stack of Axiom Cards.
- **Card**: Displays the axiom title, description, and tags.

### Svelte + RSS Proposal

```svelte
<script lang="ts">
  interface Axiom {
    id: string;
    title: string;
    description: string;
    tags: string[];
  }
  export let axioms: Axiom[];
</script>

<rtd-container variant="axiom-editor">
  <rtd-container variant="header">
    <h1>Axioms</h1>
    <p><rtd-command id="add-axiom">Add Axiom</rtd-command></p>
  </rtd-container>

  <rtd-container variant="axiom-list">
    {#each axioms as axiom}
      <rtd-container variant="axiom-card">
        <rtd-container variant="card-header">
          <h2>{axiom.title}</h2>
          <rtd-container variant="tags">
            {#each axiom.tags as tag}
              <rtd-container variant="badge">
                <p>{tag}</p>
              </rtd-container>
            {/each}
          </rtd-container>
        </rtd-container>
        <p>{axiom.description}</p>
      </rtd-container>
    {/each}
  </rtd-container>
</rtd-container>

<style>
  .axiom-editor {
    layout: mode stack, gap xl;
    spacing: all xl;
  }

  .header {
    layout: mode row;
    border: style subtle;
    spacing: y md;
  }

  h1 {
    text: size xl, weight bold;
  }

  .axiom-list {
    layout: mode stack, gap md;
  }

  .axiom-card {
    layout: mode stack, gap sm;
    surface: base surface-1;
    border: style subtle, radius md;
    spacing: all lg;
  }

  .card-header {
    layout: mode row;
  }

  h2 {
    text: size lg, weight bold;
  }

  .tags {
    layout: mode row, gap sm;
  }

  .badge {
    surface: base surface-2;
    border: radius sm;
    spacing: y xs, x sm;
    text: size xs, weight bold;
  }

  .badge p {
    /* Reset paragraph margins for badge text */
    margin: 0;
  }

  p {
    text: size md;
    surface: base transparent;
  }
</style>
```

## 2. Decision Editor

The Decision Editor tracks architectural decisions (ADRs).

### Layout Strategy

- **Header**: Title and filters.
- **Timeline/List**: Chronological list of decisions.
- **Card**: Status indicator (Proposed/Accepted/Deprecated), title, date, and summary.

### Svelte + RSS Proposal

```svelte
<script lang="ts">
  interface Decision {
    id: string;
    title: string;
    status: "proposed" | "accepted" | "deprecated";
    date: string;
    summary: string;
  }
  export let decisions: Decision[];
</script>

<rtd-container variant="decision-editor">
  <rtd-container variant="header">
    <h1>Decisions</h1>
    <rtd-container variant="filters">
      <!-- Filter controls -->
    </rtd-container>
  </rtd-container>

  <rtd-container variant="decision-list">
    {#each decisions as decision}
      <rtd-container variant="decision-card-{decision.status}">
        <rtd-container variant="meta-row">
          <rtd-container variant="status-badge-{decision.status}">
            <p>{decision.status}</p>
          </rtd-container>
          <p>{decision.date}</p>
        </rtd-container>
        <h2>{decision.title}</h2>
        <p>{decision.summary}</p>
      </rtd-container>
    {/each}
  </rtd-container>
</rtd-container>

<style>
  .decision-editor {
    layout: mode stack, gap xl;
    spacing: all xl;
  }

  .decision-list {
    layout: mode stack, gap md;
  }

  /* Base card style */
  [class^="decision-card-"] {
    layout: mode stack, gap sm;
    surface: base surface-1;
    border: style subtle, radius md;
    spacing: all lg;
    border-left-width: 4px;
  }

  .decision-card-accepted {
    border-color: var(--rtd-color-success);
  }

  .meta-row {
    layout: mode row;
    text: size sm;
    surface: base transparent;
    opacity: 0.8;
  }

  [class^="status-badge-"] {
    text: size xs, weight bold;
    spacing: y xs, x sm;
    border: radius sm;
  }

  .status-badge-accepted { surface: base surface-2; text: size xs; }
  .status-badge-proposed { surface: base surface-2; text: size xs; }
  .status-badge-deprecated { surface: base surface-2; text: size xs; }

  h2 {
    text: size lg, weight bold;
  }
</style>
```

## 3. Plan Editor

The Plan Editor visualizes the project phases and tasks.

### Layout Strategy

- **Header**: Project status summary.
- **Phases**: List of phases, distinguishing between Active, Completed, and Pending.
- **Active Phase**: Expanded view with tasks.
- **Task**: Checkbox (read-only or interactive), description, status.

### Svelte + RSS Proposal

```svelte
<script lang="ts">
  interface Task {
    description: string;
    status: "pending" | "in-progress" | "completed";
  }
  interface Phase {
    id: number;
    name: string;
    status: "active" | "completed" | "pending";
    tasks: Task[];
  }
  export let phases: Phase[];
</script>

<rtd-container variant="plan-editor">
  <rtd-container variant="header">
    <h1>Project Plan</h1>
  </rtd-container>

  <rtd-container variant="phases">
    {#each phases as phase}
      <rtd-container variant="phase-container-{phase.status}">
        <rtd-container variant="phase-header">
          <rtd-container variant="phase-title">
            <p>Phase {phase.id}</p>
            <h2>{phase.name}</h2>
          </rtd-container>
          <rtd-container variant="status-pill-{phase.status}">
            <p>{phase.status}</p>
          </rtd-container>
        </rtd-container>

        {#if phase.status === 'active' || phase.status === 'pending'}
          <rtd-container variant="task-list">
            {#each phase.tasks as task}
              <rtd-container variant="task-item-{task.status}">
                <rtd-icon name={task.status === 'completed' ? 'check' : 'circle-outline'} />
                <p>{task.description}</p>
              </rtd-container>
            {/each}
          </rtd-container>
        {/if}
      </rtd-container>
    {/each}
  </rtd-container>
</rtd-container>

<style>
  .plan-editor {
    layout: mode stack, gap xl;
    spacing: all xl;
  }

  .phases {
    layout: mode stack, gap lg;
  }

  [class^="phase-container-"] {
    layout: mode stack, gap md;
    surface: base surface-1;
    border: style subtle, radius md;
    spacing: all lg;
  }

  .phase-container-active {
    border: style bold, radius lg;
    surface: base surface-2;
  }

  .phase-header {
    layout: mode row;
    border: style subtle;
    spacing: y md;
  }

  .phase-title {
    layout: mode row, gap sm;
    align-items: baseline;
  }

  .phase-title p {
    text: size sm, family mono;
    opacity: 0.7;
  }

  h2 {
    text: size lg, weight bold;
  }

  .task-list {
    layout: mode stack, gap sm;
  }

  [class^="task-item-"] {
    layout: mode row, gap sm;
    align-items: start;
    text: size md;
  }

  .task-item-completed {
    text-decoration: line-through;
    opacity: 0.6;
  }
</style>
```
