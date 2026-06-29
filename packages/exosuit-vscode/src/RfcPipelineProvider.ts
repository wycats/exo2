import * as vscode from "vscode";
import { ExosuitTreeItem } from "./TreeModel";
import { formatRfcId, renderStageDots } from "./services/rfcDisplay";
import { createTracedProvider } from "./services/TracedProvider";

// ── Daemon response types ───────────────────────────────────────────

interface PipelineEntry {
  id: string;
  title: string;
  currentStage: number | null;
  targetStage: number | null;
  role: string;
  promotionRequirement: string | null;
  isInMotion: boolean;
  path: string | null;
}

interface PipelineData {
  phaseId: string | null;
  phaseTitle: string | null;
  entries: PipelineEntry[];
}

// ── Config ──────────────────────────────────────────────────────────

const ROLE_SECTIONS: { role: string; label: (n: number) => string }[] = [
  { role: "driving", label: (n) => `In-Flight (${n})` },
  { role: "blocked", label: (n) => `Blocked (${n})` },
  { role: "related", label: (n) => `Related (${n})` },
];

// ── Entry builder ───────────────────────────────────────────────────

function buildEntry(entry: PipelineEntry): ExosuitTreeItem {
  const stageDots = renderStageDots({
    id: entry.id,
    title: entry.title,
    currentStage: entry.currentStage ?? 0,
    targetStages: entry.targetStage !== null ? [entry.targetStage] : [],
    isInMotion: entry.isInMotion,
    role: entry.role as "driving" | "related" | "blocked",
  });

  const hasRequirement =
    entry.role === "driving" && entry.promotionRequirement !== null;

  const item = new ExosuitTreeItem(
    `${stageDots} ${formatRfcId(entry.id)}`,
    hasRequirement
      ? vscode.TreeItemCollapsibleState.Collapsed
      : vscode.TreeItemCollapsibleState.None,
    "note",
    "pending",
    "pipeline-entry",
  );
  item.id = `pipeline:${entry.id}`;
  item.description = entry.title;
  item.iconPath = undefined;
  item.tooltip = `RFC ${formatRfcId(entry.id, "full")}: ${entry.title}\nStage: ${stageDots}`;
  item.command = {
    command: "exosuit.openRfc",
    title: "Open RFC",
    arguments: [entry.id],
  };

  if (hasRequirement && entry.promotionRequirement) {
    const req = new ExosuitTreeItem(
      `Needs: ${entry.promotionRequirement}`,
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "pipeline-requirement",
    );
    req.id = `pipeline:${entry.id}:requirement`;
    req.iconPath = undefined;
    req.tooltip = `Promotion requirement for Stage ${entry.currentStage}→${entry.targetStage}`;
    item.children = [req];
  }

  return item;
}

// ── Renderer ────────────────────────────────────────────────────────

function renderRfcPipeline(
  roots: ReadonlyMap<string, unknown>,
): ExosuitTreeItem[] {
  const data = roots.get("rfc-pipeline") as PipelineData | null | undefined;

  if (!data?.phaseId) {
    const empty = new ExosuitTreeItem(
      "No active phase",
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "pipeline-empty",
    );
    empty.id = "pipeline-empty";
    empty.tooltip =
      "No phase is currently active. Start a phase to see RFC pipeline status.";
    return [empty];
  }

  if (data.entries.length === 0) {
    const none = new ExosuitTreeItem(
      "No RFCs linked to this phase",
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "pipeline-empty",
    );
    none.id = "pipeline-empty";
    none.tooltip = "Link RFCs to the active phase to populate this view.";
    return [none];
  }

  const byRole = new Map<string, PipelineEntry[]>();
  for (const entry of data.entries) {
    const list = byRole.get(entry.role);
    if (list) {
      list.push(entry);
    } else {
      byRole.set(entry.role, [entry]);
    }
  }

  return ROLE_SECTIONS.flatMap(({ role, label }) => {
    const entries = byRole.get(role);
    if (!entries) {
      return [];
    }

    const section = new ExosuitTreeItem(
      label(entries.length),
      vscode.TreeItemCollapsibleState.Expanded,
      "section",
      "pending",
      `pipeline-${role}`,
    );
    section.id = `pipeline-${role}`;
    section.iconPath = undefined;
    section.children = entries.map(buildEntry);
    return [section];
  });
}

// ── Factory ─────────────────────────────────────────────────────────

export function createRfcPipelineProvider() {
  return createTracedProvider<ExosuitTreeItem>(
    ["rfc-pipeline"],
    renderRfcPipeline,
  );
}
