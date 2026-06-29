import * as vscode from "vscode";
import type { TreeItemStatus } from "./TreeModel";
import { ExosuitTreeItem } from "./TreeModel";
import { treeItemUri } from "./TreeDecorationProvider";
import { createTracedProvider } from "./services/TracedProvider";
import type { TraceCacheRootDiagnostic } from "./services/TraceCache";

// ── Daemon response types ───────────────────────────────────────────

interface TaskLog {
  kind: string;
  message: string;
  createdAt: string;
}

interface Task {
  id: string;
  title: string;
  status: string;
  notes?: string;
  logs: TaskLog[];
  startedAt?: string;
  completedAt?: string;
}

interface Goal {
  id: string;
  title: string;
  status: string;
  description?: string;
  kind?: string;
  startedAt?: string;
  completionLog?: string;
  tasks: Task[];
}

interface PhaseDetails {
  phaseId: string;
  phaseTitle: string;
  goals: Goal[];
  progress: {
    mode: string;
    goalsCompleted: number;
    goalsTotal: number;
    tasksCompleted: number;
    tasksTotal: number;
  };
  epochContext: {
    siblingPhases: {
      id: string;
      title: string;
      status: string;
      goalCount: number;
    }[];
    nextEpoch?: { title: string; phaseCount: number };
  };
  inboxItems: {
    id: string;
    subject: string;
    body?: string;
    entityType: string;
    entityId?: string;
    source: string;
    intent: string;
    priority: string;
    status: string;
    agentId?: string;
  }[];
  completionDigests?: {
    entityType: string;
    entityId: string;
    claims: {
      id: string;
      subject: string;
      body: string;
      status: string;
      agentId?: string;
    }[];
  }[];
}

interface BetweenPhasesContext {
  epoch_id: string;
  epoch_title: string;
  completed_phase?: {
    phase_id: string;
    phase_title: string;
    goal_count: number;
    completed_goals: number;
  };
  next_phase?: {
    id: string;
    title: string;
    goal_count: number;
    rfcs: string[];
  };
  is_epoch_finale: boolean;
}

interface StatusResult {
  progress_mode?: string;
  between_phases_context?: BetweenPhasesContext;
}

interface PlanPhase {
  id: string;
  title: string;
  status: string;
  goal_count?: number;
  goalCount?: number;
  completed_goals?: number;
  completedGoals?: number;
  goals?: unknown[];
}

interface PlanEpoch {
  id: string;
  phases: PlanPhase[];
}

interface PlanReadResult {
  epochs: PlanEpoch[];
}

/** Outcome review state for a goal or task, derived from completion evidence. */
type ClaimState = "none" | "human-claimed" | "agent-needs-review";

// ── Config tables ───────────────────────────────────────────────────

const MODES: Record<string, { label: string; icon: string }> = {
  executing: { label: "Executing", icon: "run" },
  planning: { label: "Planning", icon: "edit" },
  verifying: { label: "Verifying", icon: "warning" },
  "between-phases": { label: "Between Phases", icon: "compass" },
  "between-epochs": { label: "Between Epochs", icon: "compass" },
  "roadmap-revision": { label: "Roadmap Revision", icon: "compass" },
};

const BETWEEN_MODES = new Set([
  "between-phases",
  "between-epochs",
  "roadmap-revision",
]);

const TASK_GLYPHS: Record<string, string> = {
  completed: "✓",
  "in-progress": "▸",
  abandoned: "⊘",
  skipped: "⊘",
};

const INBOX_ICONS: Record<string, string> = {
  claim: "check",
  concern: "warning",
  inquiry: "comment-discussion",
  fyi: "lightbulb",
};

// ── Helpers ─────────────────────────────────────────────────────────

/** Collapse multi-line text into a single line for description display. */
function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function taskGlyph(status: string): string {
  return TASK_GLYPHS[status] ?? "◦";
}

function resolveGoalStatus(goal: Goal): TreeItemStatus {
  if (goal.status === "completed" || goal.status === "abandoned") {
    return goal.status as TreeItemStatus;
  }

  const { tasks } = goal;
  const done = tasks.filter((t) => t.status === "completed").length;
  const active = tasks.filter((t) => t.status === "in-progress").length;

  if (
    tasks.length > 0 &&
    done === tasks.length &&
    tasks.every((t) => t.logs.length === 0)
  ) {
    return "ready-for-logging";
  }

  return active > 0 || done > 0 ? "in-progress" : "pending";
}

function lastLogSnippet(tasks: Task[]): string | undefined {
  for (let i = tasks.length - 1; i >= 0; i--) {
    const { logs, status } = tasks[i];
    if (status === "in-progress" && logs.length > 0) {
      return oneLine(logs[logs.length - 1].message);
    }
  }
  return undefined;
}

function isAttentionInboxItem(item: {
  status: string;
  intent: string;
}): boolean {
  return item.status === "pending" && item.intent !== "claim";
}

// ── Tree item builders ──────────────────────────────────────────────

function buildTask(task: Task, goalAbandoned: boolean): ExosuitTreeItem {
  const status = (
    goalAbandoned && task.status === "pending" ? "abandoned" : task.status
  ) as TreeItemStatus;
  const { logs } = task;
  const showLogs = task.status === "in-progress" && logs.length > 0;

  const item = new ExosuitTreeItem(
    `${taskGlyph(status)} ${task.title}`,
    showLogs
      ? vscode.TreeItemCollapsibleState.Expanded
      : vscode.TreeItemCollapsibleState.None,
    "task",
    status,
    "phase-task-readonly",
  );
  item.id = `exec:${task.id}`;
  item.tooltip = task.notes;
  item.resourceUri = treeItemUri("task", status, task.id);
  item.iconPath = undefined;

  if (task.status === "completed" && logs.length > 0) {
    item.description = oneLine(logs[logs.length - 1].message);
  }

  if (showLogs) {
    item.children = logs.map((log, i) => {
      const logItem = new ExosuitTreeItem(
        `• ${log.message}`,
        vscode.TreeItemCollapsibleState.None,
        "note",
        "completed",
        "completion-log",
      );
      logItem.id = `task:${task.id}:log-${i}`;
      logItem.iconPath = undefined;
      logItem.tooltip = log.message;
      return logItem;
    });
  }

  return item;
}

function groupTasks(
  goalId: string,
  taskItems: ExosuitTreeItem[],
): ExosuitTreeItem[] {
  const incomplete = taskItems.filter((t) => t.status !== "completed");
  const completed = taskItems.filter((t) => t.status === "completed");

  if (completed.length >= 3 && incomplete.length >= 2) {
    const section = new ExosuitTreeItem(
      `Completed (${completed.length})`,
      vscode.TreeItemCollapsibleState.Collapsed,
      "section",
      "completed",
      "completed-tasks-section",
    );
    section.id = `goal:${goalId}:completed-tasks`;
    section.iconPath = undefined;
    section.resourceUri = treeItemUri(
      "section",
      "completed",
      `${goalId}-completed`,
    );
    section.children = completed;
    return [...incomplete, section];
  }

  return taskItems;
}

function buildGoal(
  goal: Goal,
  claimState: ClaimState = "none",
): ExosuitTreeItem {
  const status = resolveGoalStatus(goal);
  const isAbandoned = goal.status === "abandoned";
  const { tasks } = goal;

  const expanded = status === "in-progress" || status === "ready-for-logging";
  const collapseState =
    tasks.length > 0
      ? expanded
        ? vscode.TreeItemCollapsibleState.Expanded
        : vscode.TreeItemCollapsibleState.Collapsed
      : vscode.TreeItemCollapsibleState.None;

  // Encode outcome review state in contextValue so package.json when clauses
  // can show the right button while keeping the internal command IDs stable.
  let contextValue: string;
  if (goal.status === "completed") {
    contextValue =
      goal.kind === "strike"
        ? "phase-strike-completed"
        : "phase-goal-completed";
  } else if (claimState === "agent-needs-review") {
    contextValue =
      goal.kind === "strike" ? "phase-strike-review" : "phase-goal-review";
  } else {
    contextValue = goal.kind === "strike" ? "phase-strike" : "phase-goal";
  }

  const item = new ExosuitTreeItem(
    goal.title,
    collapseState,
    "goal",
    status,
    contextValue,
  );
  item.id = `goal:${goal.id}`;
  item.resourceUri = treeItemUri("goal", status, goal.id);

  if (tasks.length > 0) {
    const done = tasks.filter((t) => t.status === "completed").length;
    const base = `${done}/${tasks.length} tasks`;
    const snippet = lastLogSnippet(tasks);
    item.description = snippet ? `${base} • ${snippet}` : base;
    item.children = groupTasks(
      goal.id,
      tasks.map((t) => buildTask(t, isAbandoned)),
    );
  }

  if (goal.kind === "strike") {
    item.iconPath = new vscode.ThemeIcon(
      "zap",
      new vscode.ThemeColor("charts.yellow"),
    );
  }

  return item;
}

function buildInbox(
  items: PhaseDetails["inboxItems"],
  completionDigests: PhaseDetails["completionDigests"],
  expanded: boolean,
): ExosuitTreeItem | null {
  const activeItems = items.filter(isAttentionInboxItem);

  if (activeItems.length === 0) {
    return null;
  }

  const completionDigestByClaim = new Map<
    string,
    { subject: string; body: string }
  >();
  for (const digest of completionDigests ?? []) {
    for (const claim of digest.claims) {
      completionDigestByClaim.set(claim.id, {
        subject: claim.subject,
        body: claim.body,
      });
    }
  }

  const section = new ExosuitTreeItem(
    `Inbox (${activeItems.length})`,
    expanded
      ? vscode.TreeItemCollapsibleState.Expanded
      : vscode.TreeItemCollapsibleState.Collapsed,
    "section",
    "pending",
    "inbox-section",
  );
  section.id = "inbox-section";
  section.iconPath = new vscode.ThemeIcon("inbox");
  section.children = activeItems.map((item) => {
    const digest = completionDigestByClaim.get(item.id);
    const subject = digest?.subject ?? item.subject;
    const body = digest?.body ?? item.body;
    const child = new ExosuitTreeItem(
      subject,
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "inbox-item",
    );
    child.id = `inbox-item-${item.id}`;
    child.iconPath = new vscode.ThemeIcon(INBOX_ICONS[item.intent] ?? "mail");
    child.description = item.intent;
    child.tooltip = `${subject}${body ? `\n\n${body}` : ""}\n\nIntent: ${item.intent}\nPriority: ${item.priority}\nEntity: ${item.entityType}${item.entityId ? `:${item.entityId}` : ""}`;
    return child;
  });

  return section;
}

function buildComingUp(details: PhaseDetails): ExosuitTreeItem | null {
  const { siblingPhases, nextEpoch } = details.epochContext;
  const idx = siblingPhases.findIndex((p) => p.id === details.phaseId);
  const nextPhase = idx >= 0 ? siblingPhases[idx + 1] : undefined;

  if (!nextPhase && !nextEpoch) {
    return null;
  }

  const section = new ExosuitTreeItem(
    "Coming Up",
    vscode.TreeItemCollapsibleState.Collapsed,
    "section",
    "pending",
    "coming-up-section",
  );
  section.id = "coming-up";
  section.iconPath = new vscode.ThemeIcon("arrow-right");
  section.children = [];

  if (nextPhase) {
    const item = new ExosuitTreeItem(
      `Next Phase: ${nextPhase.title}`,
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "coming-up-phase",
    );
    item.id = "coming-up-next-phase";
    item.iconPath = new vscode.ThemeIcon("circle-large-outline");
    item.description =
      nextPhase.goalCount > 0 ? `${nextPhase.goalCount} goals` : "";
    section.children.push(item);
  }

  if (nextEpoch) {
    const item = new ExosuitTreeItem(
      `Next Epoch: ${nextEpoch.title}`,
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "coming-up-epoch",
    );
    item.id = "coming-up-next-epoch";
    item.iconPath = new vscode.ThemeIcon("package");
    item.description = `${nextEpoch.phaseCount} phases`;
    section.children.push(item);
  }

  return section;
}

function goalCountForPlanPhase(phase: PlanPhase): number {
  return (
    phase.goalCount ??
    phase.goal_count ??
    (Array.isArray(phase.goals) ? phase.goals.length : 0)
  );
}

function completedGoalsForPlanPhase(phase: PlanPhase | undefined): number {
  if (!phase) {
    return 0;
  }
  if (typeof phase.completedGoals === "number") {
    return phase.completedGoals;
  }
  if (typeof phase.completed_goals === "number") {
    return phase.completed_goals;
  }
  if (!Array.isArray(phase.goals)) {
    return 0;
  }
  return phase.goals.filter(
    (goal) =>
      typeof goal === "object" &&
      goal !== null &&
      "status" in goal &&
      goal.status === "completed",
  ).length;
}

function findPlanPhase(
  plan: PlanReadResult | null | undefined,
  epochId: string,
  phaseId: string | undefined,
): PlanPhase | undefined {
  if (!phaseId) {
    return undefined;
  }
  return plan?.epochs
    .find((epoch) => epoch.id === epochId)
    ?.phases.find((phase) => phase.id === phaseId);
}

function buildBetweenPhasesDetails(
  status: StatusResult | null | undefined,
  plan: PlanReadResult | null | undefined,
): ExosuitTreeItem[] | undefined {
  if (
    !status?.progress_mode ||
    !BETWEEN_MODES.has(status.progress_mode) ||
    !status.between_phases_context
  ) {
    return undefined;
  }

  const context = status.between_phases_context;
  const nextPhase = findPlanPhase(plan, context.epoch_id, context.next_phase?.id);
  const nextGoalCount =
    nextPhase !== undefined
      ? goalCountForPlanPhase(nextPhase)
      : context.next_phase?.goal_count;
  const completedGoalCount =
    context.completed_phase?.completed_goals ??
    completedGoalsForPlanPhase(
      findPlanPhase(plan, context.epoch_id, context.completed_phase?.phase_id),
    );

  const header = new ExosuitTreeItem(
    "Between phases",
    vscode.TreeItemCollapsibleState.None,
    "section",
    "pending",
    "phase-between-phases",
  );
  header.id = "phase-between-phases";
  header.iconPath = new vscode.ThemeIcon(
    "compass",
    new vscode.ThemeColor("charts.blue"),
  );
  header.description = context.next_phase
    ? `next: ${context.next_phase.title}`
    : context.is_epoch_finale
      ? "epoch complete"
      : context.epoch_title;
  header.tooltip = `Epoch: ${context.epoch_title}\nMode: ${MODES[status.progress_mode]?.label ?? "Between phases"}`;

  const items = [header];

  if (context.completed_phase) {
    const completed = new ExosuitTreeItem(
      context.completed_phase.phase_title,
      vscode.TreeItemCollapsibleState.None,
      "note",
      "completed",
      "phase-between-completed",
    );
    completed.id = `phase-between-completed:${context.completed_phase.phase_id}`;
    completed.iconPath = new vscode.ThemeIcon(
      "pass-filled",
      new vscode.ThemeColor("charts.green"),
    );
    completed.description = `${completedGoalCount} goals completed`;
    completed.tooltip = `Completed phase: ${context.completed_phase.phase_title}`;
    completed.command = {
      command: "exosuit.focusPhase",
      title: "Focus Phase",
      arguments: [context.completed_phase.phase_id],
    };
    items.push(completed);
  }

  if (context.next_phase) {
    const next = new ExosuitTreeItem(
      context.next_phase.title,
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "phase-between-next",
    );
    next.id = `phase-between-next:${context.next_phase.id}`;
    next.iconPath = new vscode.ThemeIcon(
      "arrow-right",
      new vscode.ThemeColor("charts.blue"),
    );
    next.description =
      nextGoalCount && nextGoalCount > 0
        ? `${nextGoalCount} goals planned`
        : "next";
    next.tooltip = `Next phase: ${context.next_phase.title}`;
    next.command = {
      command: "exosuit.focusPhase",
      title: "Focus Phase",
      arguments: [context.next_phase.id],
    };
    items.push(next);
  }

  return items;
}

function formatInput(input: Record<string, unknown>): string {
  return JSON.stringify(input);
}

function formatDiagnosticDescription(
  input: Record<string, unknown>,
): string | undefined {
  return Object.keys(input).length > 0
    ? `input ${formatInput(input)}`
    : undefined;
}

function buildPhaseDetailsEmptyState(
  diagnostic: TraceCacheRootDiagnostic | undefined,
): ExosuitTreeItem {
  if (diagnostic?.status === "error") {
    const empty = new ExosuitTreeItem(
      "Phase details unavailable",
      vscode.TreeItemCollapsibleState.None,
      "section",
      "pending",
      "phase-details-error",
    );
    empty.id = "phase-details-error";
    empty.description = diagnostic.error?.code ?? "daemon error";
    empty.iconPath = new vscode.ThemeIcon("warning");
    empty.tooltip = `Failed to load phase details\nOperation: ${diagnostic.namespace}.${diagnostic.operation}\nInput: ${formatInput(diagnostic.input)}\n${diagnostic.error?.message ?? "Unknown error"}`;
    return empty;
  }

  if (diagnostic?.status === "empty" && diagnostic.explicitInput) {
    const id =
      typeof diagnostic.input.id === "string" ? diagnostic.input.id : undefined;
    const empty = new ExosuitTreeItem(
      "Focused phase not found",
      vscode.TreeItemCollapsibleState.None,
      "section",
      "pending",
      "phase-details-stale-focus",
    );
    empty.id = "phase-details-stale-focus";
    empty.description = id ? `cleared ${id}` : "focus cleared";
    empty.iconPath = new vscode.ThemeIcon("debug-disconnect");
    empty.tooltip = `The selected phase no longer exists. Exosuit cleared the stale focus and is reloading the active phase.\nInput: ${formatInput(diagnostic.input)}`;
    return empty;
  }

  if (diagnostic?.status === "empty") {
    const empty = new ExosuitTreeItem(
      "No active phase",
      vscode.TreeItemCollapsibleState.None,
      "section",
      "pending",
      "no-phase-message",
    );
    empty.id = "no-active-phase";
    empty.description = formatDiagnosticDescription(diagnostic.input);
    empty.iconPath = new vscode.ThemeIcon("info");
    empty.tooltip =
      "The daemon returned no phase details for the active-phase request. Run 'exo phase start' to begin, or reset sidebar state if this looks stale.";
    return empty;
  }

  const empty = new ExosuitTreeItem(
    "Loading phase details",
    vscode.TreeItemCollapsibleState.None,
    "section",
    "pending",
    "phase-details-loading",
  );
  empty.id = "phase-details-loading";
  empty.description = "waiting for daemon";
  empty.iconPath = new vscode.ThemeIcon("sync~spin");
  return empty;
}

// ── Renderer ────────────────────────────────────────────────────────

export function renderPhaseDetails(
  roots: ReadonlyMap<string, unknown>,
  diagnostics: ReadonlyMap<string, TraceCacheRootDiagnostic | undefined>,
): ExosuitTreeItem[] {
  const details = roots.get("phase-details") as PhaseDetails | null | undefined;
  const phaseDetailsDiagnostic = diagnostics.get("phase-details");

  if (!details) {
    if (!phaseDetailsDiagnostic?.explicitInput) {
      const status = roots.get("status") as StatusResult | null | undefined;
      const plan = roots.get("plan-read") as PlanReadResult | null | undefined;
      const betweenPhaseItems = buildBetweenPhasesDetails(status, plan);
      if (betweenPhaseItems) {
        return betweenPhaseItems;
      }
    }
    return [buildPhaseDetailsEmptyState(phaseDetailsDiagnostic)];
  }

  const { progress, goals, inboxItems, completionDigests } = details;
  const mode = MODES[progress.mode] ?? MODES.executing;
  const isBetween = BETWEEN_MODES.has(progress.mode);
  const activePhaseId = details.epochContext.siblingPhases.find(
    (phase) => phase.status === "in-progress",
  )?.id;
  const isFocusedNonActive = Boolean(
    activePhaseId && details.phaseId !== activePhaseId,
  );

  const progressText =
    progress.goalsTotal > 0 || progress.tasksTotal > 0
      ? `${progress.goalsCompleted}/${progress.goalsTotal} goals • ${progress.tasksCompleted}/${progress.tasksTotal} tasks`
      : "No goals defined";

  const allDone =
    progress.goalsCompleted === progress.goalsTotal &&
    progress.tasksCompleted === progress.tasksTotal;

  // Phase header
  const header = new ExosuitTreeItem(
    details.phaseTitle || "Current Phase",
    vscode.TreeItemCollapsibleState.None,
    "section",
    allDone ? "completed" : "phase-active",
    "phase-header",
  );
  header.id = "phase-header";
  header.description = isFocusedNonActive
    ? `Viewing non-active phase • ${progressText}`
    : progressText;
  header.iconPath = new vscode.ThemeIcon(mode.icon);
  header.tooltip = `${details.phaseTitle} (${details.phaseId})\n${mode.label}\n${
    isFocusedNonActive ? "Focused phase (not active)\n" : ""
  }${progressText}`;

  const tree: ExosuitTreeItem[] = [header];

  // Inbox
  const inbox = buildInbox(
    inboxItems ?? [],
    completionDigests ?? [],
    isBetween,
  );
  if (inbox) {
    tree.push(inbox);
  }

  // Goals
  if (goals.length === 0) {
    const hint = new ExosuitTreeItem(
      "No goals defined yet",
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "no-goals-hint",
    );
    hint.id = "no-goals-hint";
    hint.iconPath = new vscode.ThemeIcon("info");
    hint.description = "Use 'exo goal add' to create goals";
    tree.push(hint);
    return tree;
  }

  const completed = goals.filter((g) => g.status === "completed");
  const active = goals.filter((g) => g.status !== "completed");

  if (completed.length > 0) {
    const section = new ExosuitTreeItem(
      `Recently Completed (${completed.length})`,
      vscode.TreeItemCollapsibleState.Collapsed,
      "section",
      "completed",
      "completed-goals-section",
    );
    section.id = "completed-goals";
    section.iconPath = new vscode.ThemeIcon(
      "pass-filled",
      new vscode.ThemeColor("charts.green"),
    );
    section.children = completed.map((g) => buildGoal(g));
    tree.push(section);
  }

  // Build per-entity claim state map from completion digests. Claims are
  // outcome-review evidence, not regular inbox attention items.
  const claimStates = new Map<string, ClaimState>();
  for (const digest of completionDigests ?? []) {
    const key = `${digest.entityType}:${digest.entityId}`;
    for (const claim of digest.claims) {
      if (claim.status !== "pending" || claimStates.has(key)) {
        continue;
      }
      claimStates.set(
        key,
        claim.agentId ? "agent-needs-review" : "human-claimed",
      );
    }
  }

  for (const goal of active) {
    const cs = claimStates.get(`goal:${goal.id}`) ?? "none";
    tree.push(buildGoal(goal, cs));
  }

  const comingUp = buildComingUp(details);
  if (comingUp) {
    tree.push(comingUp);
  }

  return tree;
}

// ── Factory ─────────────────────────────────────────────────────────

export function createPhaseDetailsProvider() {
  return createTracedProvider<ExosuitTreeItem>(
    ["phase-details", "status", "plan-read"],
    renderPhaseDetails,
  );
}
