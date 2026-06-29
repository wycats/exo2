import * as vscode from "vscode";
import { type Idea, type IdeaStatus } from "@exosuit/core";
import { createTracedProvider } from "./services/TracedProvider";

// ── Status Config ───────────────────────────────────────────────────

const STATUS: Record<
  IdeaStatus,
  { label: string; icon: string; color?: string; itemLabel?: string }
> = {
  new: { label: "New", icon: "lightbulb", color: "charts.yellow" },
  triaged: {
    label: "Triaged",
    icon: "filter",
    color: "charts.blue",
    itemLabel: "triaged",
  },
  accepted: { label: "Accepted", icon: "check", color: "charts.green" },
  rejected: { label: "Rejected", icon: "x", color: "charts.red" },
  deferred: { label: "Deferred", icon: "clock", color: "charts.gray" },
  implemented: {
    label: "Implemented",
    icon: "check-all",
    color: "charts.green",
    itemLabel: "✓ implemented",
  },
  archived: { label: "Archived", icon: "archive", color: "charts.gray" },
};

/** Display order for sections in the tree. */
const SECTION_ORDER: { status: IdeaStatus; expanded?: boolean }[] = [
  { status: "new", expanded: true },
  { status: "triaged" },
  { status: "accepted" },
  { status: "deferred" },
  { status: "implemented" },
  { status: "rejected" },
];

function icon(status: IdeaStatus): vscode.ThemeIcon {
  const { icon, color } = STATUS[status];
  return color
    ? new vscode.ThemeIcon(icon, new vscode.ThemeColor(color))
    : new vscode.ThemeIcon(icon);
}

// ── Tree Items ──────────────────────────────────────────────────────

export class IdeaTreeItem extends vscode.TreeItem {
  readonly idea: Idea;

  constructor(idea: Idea) {
    super(idea.title, vscode.TreeItemCollapsibleState.None);
    this.idea = idea;
    this.id = `idea-${idea.id}`;
    this.description = STATUS[idea.status].itemLabel ?? idea.status;
    this.iconPath = icon(idea.status);
    this.contextValue = `idea-${idea.status}`;
    this.tooltip = buildTooltip(idea);
  }
}

export class IdeaSectionItem extends vscode.TreeItem {
  readonly status: IdeaStatus;
  readonly children: IdeaTreeItem[];

  constructor(status: IdeaStatus, ideas: Idea[], expanded?: boolean) {
    const cfg = STATUS[status];
    super(
      cfg.label,
      expanded
        ? vscode.TreeItemCollapsibleState.Expanded
        : vscode.TreeItemCollapsibleState.Collapsed,
    );
    this.status = status;
    this.children = ideas.map((i) => new IdeaTreeItem(i));
    this.id = `idea-section-${status}`;
    this.description = `${ideas.length}`;
    this.iconPath = icon(status);
    this.contextValue = "ideaSection";
  }
}

// ── Renderer ────────────────────────────────────────────────────────

function renderIdeasTree(
  roots: ReadonlyMap<string, unknown>,
): (IdeaTreeItem | IdeaSectionItem)[] {
  const snapshot = roots.get("context-snapshot") as
    | { ideas?: { ideas?: Idea[] } }
    | null
    | undefined;

  const ideas = snapshot?.ideas?.ideas;
  if (!ideas || ideas.length === 0) {
    return [];
  }

  const byStatus = new Map<IdeaStatus, Idea[]>();
  for (const idea of ideas) {
    const list = byStatus.get(idea.status);
    if (list) {
      list.push(idea);
    } else {
      byStatus.set(idea.status, [idea]);
    }
  }

  return SECTION_ORDER.flatMap(({ status, expanded }) => {
    const matching = byStatus.get(status);
    return matching ? [new IdeaSectionItem(status, matching, expanded)] : [];
  });
}

// ── Factory ─────────────────────────────────────────────────────────

export function createIdeasTreeProvider() {
  let lastSections: IdeaSectionItem[] = [];

  const provider = createTracedProvider<IdeaTreeItem | IdeaSectionItem>(
    ["context-snapshot"],
    (roots) => {
      const items = renderIdeasTree(roots);
      lastSections = items.filter(
        (s): s is IdeaSectionItem => s instanceof IdeaSectionItem,
      );
      return items;
    },
  );

  return {
    provider,
    connectTreeView(treeView: vscode.TreeView<IdeaTreeItem | IdeaSectionItem>) {
      provider.onDidChangeTreeData(() => {
        const newCount =
          lastSections.find((s) => s.status === "new")?.children.length ?? 0;
        const total = lastSections.reduce((n, s) => n + s.children.length, 0);

        treeView.badge = total
          ? badge("idea", newCount || total, newCount ? "new" : "total")
          : undefined;
      });
    },
  };
}

const pluralize = (count: number, singular: string, plural = `${singular}s`) =>
  count === 1 ? singular : plural;

function badge(
  type: string,
  value: number,
  qualifier: string,
): vscode.ViewBadge {
  return { value, tooltip: `${value} ${qualifier} ${pluralize(value, type)}` };
}

// ── Helpers ─────────────────────────────────────────────────────────

function buildTooltip(idea: Idea): vscode.MarkdownString {
  const md = new vscode.MarkdownString();
  md.appendMarkdown(`### ${idea.title}\n\n`);
  md.appendMarkdown(`**Status:** ${idea.status}\n\n`);
  if (idea.description) {
    md.appendMarkdown(`${idea.description}\n\n`);
  }
  if (idea.tags.length > 0) {
    md.appendMarkdown(`**Tags:** ${idea.tags.join(", ")}\n\n`);
  }
  md.appendMarkdown(
    `**Created:** ${new Date(idea.created_at).toLocaleDateString()}`,
  );
  return md;
}
