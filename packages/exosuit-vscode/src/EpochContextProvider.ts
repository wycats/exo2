import * as vscode from "vscode";
import type { TreeItemStatus } from "./TreeModel";
import { ExosuitTreeItem } from "./TreeModel";
import { createTracedProvider } from "./services/TracedProvider";
import type { TraceCacheRootDiagnostic } from "./services/TraceCache";

// ── Types (from daemon phase-details response) ─────────────────────

interface SiblingPhase {
  id: string;
  title: string;
  status: string;
  goalCount: number;
  completedGoals: number;
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
  title: string;
  status: string;
  phases: PlanPhase[];
}

interface PlanReadResult {
  epochs: PlanEpoch[];
}

interface NextEpoch {
  title: string;
  phaseCount: number;
  phaseTitles: string[];
}

interface DaemonEpochContext {
  siblingPhases: SiblingPhase[];
  nextEpoch?: NextEpoch;
}

interface DaemonPhaseDetails {
  progress: { mode: string };
  epochContext: DaemonEpochContext;
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

// ── Status helpers ──────────────────────────────────────────────────

function toTreeItemStatus(status: string): TreeItemStatus {
  switch (status) {
    case "completed":
      return "completed";
    case "in-progress":
      return "in-progress";
    default:
      return "pending";
  }
}

function phaseIcon(status: string, isCurrent: boolean): vscode.ThemeIcon {
  if (isCurrent) {
    return new vscode.ThemeIcon("target", new vscode.ThemeColor("charts.blue"));
  }
  switch (status) {
    case "completed":
      return new vscode.ThemeIcon(
        "pass-filled",
        new vscode.ThemeColor("charts.green"),
      );
    case "in-progress":
      return new vscode.ThemeIcon(
        "play-circle",
        new vscode.ThemeColor("charts.blue"),
      );
    case "deferred":
      return new vscode.ThemeIcon("circle-slash");
    case "abandoned":
      return new vscode.ThemeIcon(
        "circle-slash",
        new vscode.ThemeColor("charts.red"),
      );
    default:
      return new vscode.ThemeIcon("circle-large-outline");
  }
}

function phaseDescription(
  phase: SiblingPhase,
  isCurrent: boolean,
  expanded: boolean,
): string {
  if (!expanded) {
    return "";
  }
  if (isCurrent) {
    return `${phase.completedGoals}/${phase.goalCount} goals`;
  }
  if (phase.status === "completed") {
    return `${phase.completedGoals} goals completed`;
  }
  if (phase.goalCount > 0) {
    return `${phase.goalCount} goals planned`;
  }
  return "goals TBD";
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

function normalizePlanPhase(phase: PlanPhase): SiblingPhase {
  const goals = Array.isArray(phase.goals) ? phase.goals : [];
  return {
    id: phase.id,
    title: phase.title,
    status: phase.status,
    goalCount: phase.goalCount ?? phase.goal_count ?? goals.length,
    completedGoals:
      phase.completedGoals ??
      phase.completed_goals ??
      goals.filter(
        (goal) =>
          typeof goal === "object" &&
          goal !== null &&
          "status" in goal &&
          goal.status === "completed",
      ).length,
  };
}

function findEpochForBetweenPhases(
  plan: PlanReadResult | null | undefined,
  context: BetweenPhasesContext,
): PlanEpoch | undefined {
  return plan?.epochs.find((epoch) => epoch.id === context.epoch_id);
}

function buildBetweenPhasesEpochHeader(
  context: BetweenPhasesContext | null | undefined,
): ExosuitTreeItem | undefined {
  if (!context) {
    return undefined;
  }

  const item = new ExosuitTreeItem(
    context.epoch_title,
    vscode.TreeItemCollapsibleState.None,
    "section",
    "in-progress",
    "epoch-between-phases",
  );
  item.id = `epoch-between-phases:${context.epoch_id}`;
  item.iconPath = new vscode.ThemeIcon(
    "compass",
    new vscode.ThemeColor("charts.blue"),
  );
  item.description = context.next_phase
    ? `Between phases • next: ${context.next_phase.title}`
    : "Between phases";
  item.tooltip = context.completed_phase
    ? `Epoch: ${context.epoch_title}\nCompleted phase: ${context.completed_phase.phase_title}\nNext phase: ${context.next_phase?.title ?? "none"}`
    : `Epoch: ${context.epoch_title}\nBetween phases`;
  return item;
}

function buildBetweenPhasesEpochItems(
  context: BetweenPhasesContext | null | undefined,
  plan: PlanReadResult | null | undefined,
): ExosuitTreeItem[] | undefined {
  if (!context) {
    return undefined;
  }

  const epoch = findEpochForBetweenPhases(plan, context);
  if (!epoch) {
    const header = buildBetweenPhasesEpochHeader(context);
    return header ? [header] : undefined;
  }

  const phases = epoch.phases.map(normalizePlanPhase);
  return phases.map((phase) => {
    const isNext = phase.id === context.next_phase?.id;
    const item = new ExosuitTreeItem(
      phase.title,
      vscode.TreeItemCollapsibleState.None,
      "note",
      toTreeItemStatus(phase.status),
      isNext ? "epoch-next-phase" : "epoch-sibling-phase",
    );
    item.id = `epoch-phase:${phase.id}`;
    item.iconPath = isNext
      ? new vscode.ThemeIcon(
          "arrow-right",
          new vscode.ThemeColor("charts.blue"),
        )
      : phaseIcon(phase.status, false);
    item.description = isNext ? "next" : phaseDescription(phase, false, true);
    item.tooltip = `${phase.title}\nStatus: ${phase.status}\nGoals: ${phase.completedGoals}/${phase.goalCount} completed`;
    item.command = {
      command: "exosuit.focusPhase",
      title: "Focus Phase",
      arguments: [phase.id],
    };
    return item;
  });
}

export const __test__ = {
  buildBetweenPhasesEpochHeader,
  buildBetweenPhasesEpochItems,
};

function buildEpochContextEmptyState(
  diagnostic: TraceCacheRootDiagnostic | undefined,
): ExosuitTreeItem {
  if (diagnostic?.status === "error") {
    const empty = new ExosuitTreeItem(
      "Epoch context unavailable",
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "epoch-error",
    );
    empty.id = "epoch-error";
    empty.iconPath = new vscode.ThemeIcon("warning");
    empty.description = diagnostic.error?.code ?? "daemon error";
    empty.tooltip = `Failed to load epoch context\nOperation: ${diagnostic.namespace}.${diagnostic.operation}\nInput: ${formatInput(diagnostic.input)}\n${diagnostic.error?.message ?? "Unknown error"}`;
    return empty;
  }

  if (diagnostic?.status === "empty" && diagnostic.explicitInput) {
    const id =
      typeof diagnostic.input.id === "string" ? diagnostic.input.id : undefined;
    const empty = new ExosuitTreeItem(
      "Focused phase not found",
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "epoch-stale-focus",
    );
    empty.id = "epoch-stale-focus";
    empty.iconPath = new vscode.ThemeIcon("debug-disconnect");
    empty.description = id ? `cleared ${id}` : "focus cleared";
    empty.tooltip = `The selected phase no longer exists. Exosuit cleared the stale focus and is reloading the active epoch context.\nInput: ${formatInput(diagnostic.input)}`;
    return empty;
  }

  if (diagnostic?.status === "empty") {
    const empty = new ExosuitTreeItem(
      "No active epoch",
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "epoch-empty",
    );
    empty.id = "epoch-empty";
    empty.iconPath = new vscode.ThemeIcon("info");
    empty.description = formatDiagnosticDescription(diagnostic.input);
    empty.tooltip =
      "The daemon returned no phase details for the active-phase request. Start an epoch to see context here, or reset sidebar state if this looks stale.";
    return empty;
  }

  const empty = new ExosuitTreeItem(
    "Loading epoch context",
    vscode.TreeItemCollapsibleState.None,
    "note",
    "pending",
    "epoch-loading",
  );
  empty.id = "epoch-loading";
  empty.iconPath = new vscode.ThemeIcon("sync~spin");
  empty.description = "waiting for daemon";
  return empty;
}

// ── Renderer ────────────────────────────────────────────────────────

const BETWEEN_MODES = new Set([
  "between-phases",
  "between-epochs",
  "roadmap-revision",
]);

export function renderEpochContext(
  roots: ReadonlyMap<string, unknown>,
  diagnostics: ReadonlyMap<string, TraceCacheRootDiagnostic | undefined>,
): ExosuitTreeItem[] {
  const details = roots.get("phase-details") as
    | DaemonPhaseDetails
    | null
    | undefined;
  const betweenPhasesContext = roots.get("between-phases-context") as
    | BetweenPhasesContext
    | null
    | undefined;
  const status = roots.get("status") as
    | { between_phases_context?: BetweenPhasesContext }
    | null
    | undefined;
  const plan = roots.get("plan-read") as PlanReadResult | null | undefined;

  if (!details?.epochContext) {
    const betweenPhasesItems = buildBetweenPhasesEpochItems(
      betweenPhasesContext ?? status?.between_phases_context,
      plan,
    );
    if (betweenPhasesItems) {
      return betweenPhasesItems;
    }
    return [buildEpochContextEmptyState(diagnostics.get("phase-details"))];
  }

  const { epochContext, progress } = details;
  const expanded = BETWEEN_MODES.has(progress.mode);
  const activePhaseId =
    epochContext.siblingPhases.find((p) => p.status === "in-progress")?.id ??
    "";

  const items: ExosuitTreeItem[] = epochContext.siblingPhases.map((phase) => {
    const isCurrent = phase.id === activePhaseId;

    const item = new ExosuitTreeItem(
      phase.title,
      vscode.TreeItemCollapsibleState.None,
      "note",
      toTreeItemStatus(phase.status),
      "epoch-sibling-phase",
    );
    item.id = `epoch-phase:${phase.id}`;
    item.iconPath = phaseIcon(phase.status, isCurrent);
    item.description = phaseDescription(phase, isCurrent, expanded);
    item.tooltip = `${phase.title}\nStatus: ${phase.status}\nGoals: ${phase.completedGoals}/${phase.goalCount} completed`;
    item.command = {
      command: "exosuit.focusPhase",
      title: "Focus Phase",
      arguments: [phase.id],
    };
    return item;
  });

  // Next epoch teaser
  if (epochContext.nextEpoch) {
    const { title, phaseCount, phaseTitles } = epochContext.nextEpoch;

    const nextItem = new ExosuitTreeItem(
      `Next: ${title}`,
      phaseTitles.length > 0
        ? vscode.TreeItemCollapsibleState.Collapsed
        : vscode.TreeItemCollapsibleState.None,
      "section",
      "pending",
      "epoch-next",
    );
    nextItem.id = "epoch-next";
    nextItem.iconPath = new vscode.ThemeIcon("package");
    nextItem.description = `${phaseCount} phases`;
    nextItem.tooltip = `Next epoch: ${title}\n${phaseCount} phases planned`;
    nextItem.children = phaseTitles.map((phaseTitle, i) => {
      const child = new ExosuitTreeItem(
        phaseTitle,
        vscode.TreeItemCollapsibleState.None,
        "note",
        "pending",
        "epoch-next-phase",
      );
      child.id = `epoch-next-phase:${i}`;
      child.iconPath = new vscode.ThemeIcon("circle-outline");
      child.description = "";
      return child;
    });

    items.push(nextItem);
  }

  return items;
}

// ── Factory ─────────────────────────────────────────────────────────

export function createEpochContextProvider() {
  return createTracedProvider<ExosuitTreeItem>(
    ["phase-details", "status", "plan-read"],
    renderEpochContext,
  );
}
