import * as vscode from "vscode";
import {
  workbenchLaneListFrom,
  type WorkbenchLaneSummary,
} from "./services/WorkLanesClient";
import {
  createTracedProvider,
  type TracedProvider,
} from "./services/TracedProvider";
import type { TraceCacheRootDiagnostic } from "./services/TraceCache";

export class WorkLaneTreeItem extends vscode.TreeItem {
  constructor(public readonly lane: WorkbenchLaneSummary) {
    super(lane.title, vscode.TreeItemCollapsibleState.None);
    this.id = `work-lane:${lane.id}`;
    this.description = [
      lane.focused_here ? "focused" : undefined,
      lane.state,
      lane.phase_title,
    ]
      .filter(Boolean)
      .join(" • ");
    this.contextValue = lane.focused_here
      ? "workbench-lane-focused"
      : "workbench-lane";
    this.iconPath = lane.focused_here
      ? new vscode.ThemeIcon(
          "target",
          new vscode.ThemeColor("charts.green"),
        )
      : lane.state === "executing"
        ? new vscode.ThemeIcon(
            "play-circle",
            new vscode.ThemeColor("charts.blue"),
          )
        : new vscode.ThemeIcon("circle-large-outline");
    this.tooltip = [
      lane.title,
      lane.intent,
      "",
      `State: ${lane.state}`,
      `Phase: ${lane.phase_title} (${lane.phase_status})`,
      `Lane ID: ${lane.id}`,
      `Focused in this workspace: ${lane.focused_here ? "yes" : "no"}`,
    ].join("\n");
  }
}

export class WorkLanesStateItem extends vscode.TreeItem {
  constructor(
    label: string,
    description: string | undefined,
    icon: vscode.ThemeIcon,
    tooltip?: string,
  ) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.id = `work-lanes-state:${label}`;
    this.description = description;
    this.iconPath = icon;
    this.tooltip = tooltip ?? label;
    this.contextValue = "workbench-lanes-state";
  }
}

export type WorkLanesTreeItem = WorkLaneTreeItem | WorkLanesStateItem;

export function renderWorkLanes(
  roots: ReadonlyMap<string, unknown>,
  diagnostics: ReadonlyMap<string, TraceCacheRootDiagnostic | undefined>,
): WorkLanesTreeItem[] {
  const raw = roots.get("work-lanes");
  const diagnostic = diagnostics.get("work-lanes");
  const data = workbenchLaneListFrom(raw);

  if (!data) {
    if (diagnostic?.status === "error") {
      return [
        new WorkLanesStateItem(
          "Work lanes unavailable",
          "retrying",
          new vscode.ThemeIcon(
            "warning",
            new vscode.ThemeColor("problemsWarningIcon.foreground"),
          ),
          diagnostic.error?.message,
        ),
      ];
    }
    if (raw !== null && raw !== undefined) {
      return [
        new WorkLanesStateItem(
          "Work lanes unavailable",
          "invalid response",
          new vscode.ThemeIcon(
            "warning",
            new vscode.ThemeColor("problemsWarningIcon.foreground"),
          ),
          "Exo returned an unexpected work lane response.",
        ),
      ];
    }
    return [
      new WorkLanesStateItem(
        "Loading work lanes",
        undefined,
        new vscode.ThemeIcon("loading~spin"),
      ),
    ];
  }

  const items: WorkLanesTreeItem[] = data.diagnostics.map(
    (laneDiagnostic) =>
      new WorkLanesStateItem(
        "Lane focus needs attention",
        "phase mismatch",
        new vscode.ThemeIcon(
          "warning",
          new vscode.ThemeColor("problemsWarningIcon.foreground"),
        ),
        laneDiagnostic.message,
      ),
  );

  if (data.lanes.length === 0) {
    items.push(
      new WorkLanesStateItem(
        "No workbench lanes",
        undefined,
        new vscode.ThemeIcon("circle-large-outline"),
      ),
    );
    return items;
  }

  items.push(...data.lanes.map((lane) => new WorkLaneTreeItem(lane)));
  return items;
}

export function createWorkLanesProvider(): TracedProvider<WorkLanesTreeItem> {
  return createTracedProvider<WorkLanesTreeItem>(
    ["work-lanes"],
    renderWorkLanes,
  );
}
