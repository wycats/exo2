import * as vscode from "vscode";
import { ExosuitTreeItem } from "./TreeModel";
import { createTracedProvider } from "./services/TracedProvider";
import type { TraceCacheRootDiagnostic } from "./services/TraceCache";
import {
  buildSidecarStatusViewModel,
  type ExoStatusSidecarSyncJson,
  type SidecarRepoStatusJson,
  type SidecarStatusJson,
} from "./services/SidecarStatusViewModel";
import type {
  SidecarBindingView,
  SidecarCheckedAttempt,
  SidecarDiscoveryView,
  SidecarPaneAction,
  SidecarPaneSourceDiagnostic,
  SidecarRepoFile,
  SidecarRepositoryView,
  SidecarStatusViewModel,
} from "./types/sidecarStatus";

const ROOT_IDS = ["sidecar-status", "sidecar-repo-status", "status"] as const;

function asSourceDiagnostic(
  rootId: string,
  diagnostic: TraceCacheRootDiagnostic | undefined,
): SidecarPaneSourceDiagnostic {
  if (!diagnostic) {
    return {
      id: rootId,
      status: "unknown",
    };
  }

  return {
    id: rootId,
    status: diagnostic.status,
    message: diagnostic.error?.message,
    fetchedAt: diagnostic.fetchedAt,
  };
}

function normalizeSources(
  roots: ReadonlyMap<string, unknown>,
  diagnostics: ReadonlyMap<string, TraceCacheRootDiagnostic | undefined>,
): SidecarStatusViewModel {
  return buildSidecarStatusViewModel({
    sidecarStatus: roots.get("sidecar-status") as
      | SidecarStatusJson
      | null
      | undefined,
    sidecarRepoStatus: roots.get("sidecar-repo-status") as
      | SidecarRepoStatusJson
      | null
      | undefined,
    exoStatus: roots.get("status") as
      | ExoStatusSidecarSyncJson
      | null
      | undefined,
    diagnostics: ROOT_IDS.map((rootId) =>
      asSourceDiagnostic(rootId, diagnostics.get(rootId)),
    ),
  });
}

function iconForRepository(
  repository: SidecarRepositoryView,
): vscode.ThemeIcon {
  switch (repository.state) {
    case "clean":
      return new vscode.ThemeIcon(
        "pass-filled",
        new vscode.ThemeColor("charts.green"),
      );
    case "dirty":
    case "needs-remote":
    case "needs-push":
    case "behind":
      return new vscode.ThemeIcon(
        "warning",
        new vscode.ThemeColor("charts.yellow"),
      );
    case "error":
      return new vscode.ThemeIcon("error", new vscode.ThemeColor("charts.red"));
    case "unavailable":
      return new vscode.ThemeIcon(
        "circle-slash",
        new vscode.ThemeColor("charts.gray"),
      );
  }
}

function repositoryDescription(repository: SidecarRepositoryView): string {
  switch (repository.state) {
    case "clean":
      return "clean";
    case "dirty":
      return `${repository.files.length} changed`;
    case "needs-remote":
      return "no remote";
    case "needs-push":
      return repository.ahead !== null
        ? `ahead ${repository.ahead}`
        : "needs push";
    case "behind":
      return repository.behind !== null
        ? `behind ${repository.behind}`
        : "behind";
    case "error":
      return repository.issue ?? "error";
    case "unavailable":
      return "unavailable";
  }
}

function buildHeader(view: SidecarStatusViewModel): ExosuitTreeItem {
  const binding = view.binding.state;
  const repository = view.repository.state;
  const item = new ExosuitTreeItem(
    "Sidecar Status",
    vscode.TreeItemCollapsibleState.None,
    "section",
    repository === "clean" ? "completed" : "pending",
    "sidecar-status-header",
  );
  item.id = "sidecar-status-header";
  item.description = `${binding} • ${repository}`;
  item.iconPath = iconForRepository(view.repository);
  item.tooltip = `Binding: ${binding}\nRepository: ${repository}`;
  return item;
}

function buildBinding(view: SidecarStatusViewModel): ExosuitTreeItem {
  if (view.binding.state === "unknown") {
    return buildUnknownBinding();
  }

  const item = new ExosuitTreeItem(
    view.binding.state === "linked" ? "Linked sidecar" : "Sidecar not linked",
    vscode.TreeItemCollapsibleState.Expanded,
    "note",
    view.binding.state === "linked" ? "completed" : "pending",
    "sidecar-status-binding",
  );
  item.id = "sidecar-status-binding";
  item.description = bindingDescription(view.binding);
  item.iconPath = new vscode.ThemeIcon(
    view.binding.state === "linked" ? "link" : "circle-slash",
  );
  item.children = bindingChildren(view.binding);
  item.tooltip = [
    `State: ${view.binding.state}`,
    view.binding.sidecarKey ? `Key: ${view.binding.sidecarKey}` : undefined,
    view.binding.sidecarRoot ? `Root: ${view.binding.sidecarRoot}` : undefined,
  ]
    .filter(Boolean)
    .join("\n");
  return item;
}

function buildUnknownBinding(): ExosuitTreeItem {
  const item = new ExosuitTreeItem(
    "Sidecar status unknown",
    vscode.TreeItemCollapsibleState.None,
    "note",
    "pending",
    "sidecar-status-binding",
  );
  item.id = "sidecar-status-binding";
  item.description = "waiting for status source";
  item.iconPath = new vscode.ThemeIcon(
    "sync~spin",
    new vscode.ThemeColor("charts.gray"),
  );
  item.tooltip = "Waiting for sidecar status data from the daemon.";
  return item;
}

function bindingDescription(binding: SidecarBindingView): string {
  if (binding.state === "linked") {
    return binding.sidecarKey ?? "linked";
  }
  return binding.policy === "local" ? "local state" : "not linked";
}

function bindingChildren(binding: SidecarBindingView): ExosuitTreeItem[] {
  if (binding.state === "linked") {
    return compactItems([
      detailItem("Policy", binding.policy),
      detailItem("Root", binding.sidecarRoot),
      detailItem("Auto persist", autoPersistDescription(binding)),
      detailItem("Projection", binding.paths.projectionDir),
    ]);
  }

  return compactItems([
    detailItem("Policy", binding.policy),
    detailItem("Manifest", binding.paths.manifest),
    detailItem("Next", "bootstrap sidecar state"),
  ]);
}

function autoPersistDescription(binding: SidecarBindingView): string {
  const commit = binding.autoCommit === true ? "commit on" : "commit off";
  const push = binding.autoPush ? `push ${binding.autoPush}` : "push unknown";
  return `${commit} • ${push}`;
}

function compactItems(items: Array<ExosuitTreeItem | null>): ExosuitTreeItem[] {
  return items.filter((item): item is ExosuitTreeItem => item !== null);
}

function detailItem(
  label: string,
  description: string | null,
): ExosuitTreeItem | null {
  if (!description) {
    return null;
  }

  const item = new ExosuitTreeItem(
    label,
    vscode.TreeItemCollapsibleState.None,
    "note",
    "pending",
    "sidecar-status-detail",
  );
  item.id = `sidecar-status-detail-${label.toLowerCase().replace(/\s+/g, "-")}`;
  item.description = description;
  item.iconPath = undefined;
  item.tooltip = `${label}: ${description}`;
  return item;
}

function buildRepository(view: SidecarStatusViewModel): ExosuitTreeItem {
  const item = new ExosuitTreeItem(
    "Repository",
    repositoryChildren(view.repository).length > 0
      ? vscode.TreeItemCollapsibleState.Expanded
      : vscode.TreeItemCollapsibleState.None,
    "note",
    view.repository.state === "clean" ? "completed" : "pending",
    "sidecar-status-repository",
  );
  item.id = "sidecar-status-repository";
  item.description = repositoryDescription(view.repository);
  item.iconPath = iconForRepository(view.repository);
  item.children = repositoryChildren(view.repository);
  item.tooltip = [
    `State: ${view.repository.state}`,
    view.repository.sidecarRoot
      ? `Root: ${view.repository.sidecarRoot}`
      : undefined,
    view.repository.branch ? `Branch: ${view.repository.branch}` : undefined,
    view.repository.remote ? `Remote: ${view.repository.remote}` : undefined,
    view.repository.issue ? `Issue: ${view.repository.issue}` : undefined,
  ]
    .filter(Boolean)
    .join("\n");
  return item;
}

function repositoryChildren(
  repository: SidecarRepositoryView,
): ExosuitTreeItem[] {
  return compactItems([
    detailItem("Branch", repository.branch),
    detailItem("Remote", repository.remote),
    detailItem("Sync", syncDescription(repository)),
    detailItem("Issue", repository.issue),
    detailItem(
      "Changed files",
      repository.files.length > 0 ? String(repository.files.length) : null,
    ),
    ...repository.files.map(fileItem),
  ]);
}

function syncDescription(repository: SidecarRepositoryView): string | null {
  const parts = [];
  if (repository.ahead !== null) {
    parts.push(`ahead ${repository.ahead}`);
  }
  if (repository.behind !== null) {
    parts.push(`behind ${repository.behind}`);
  }
  return parts.length > 0 ? parts.join(" • ") : null;
}

function fileItem(file: SidecarRepoFile): ExosuitTreeItem {
  const item = new ExosuitTreeItem(
    `${file.status} ${file.path}`,
    vscode.TreeItemCollapsibleState.None,
    "note",
    "pending",
    "sidecar-status-file",
  );
  item.id = `sidecar-status-file-${file.path}`;
  item.description = undefined;
  item.iconPath = undefined;
  item.tooltip = `${file.status} ${file.path}`;
  return item;
}

function buildDiscovery(view: SidecarStatusViewModel): ExosuitTreeItem | null {
  if (
    view.discovery.state === "not-needed" ||
    view.discovery.state === "not-run"
  ) {
    return null;
  }

  const children = discoveryChildren(view.discovery);
  const item = new ExosuitTreeItem(
    "Discovery",
    children.length > 0
      ? vscode.TreeItemCollapsibleState.Expanded
      : vscode.TreeItemCollapsibleState.None,
    "section",
    view.discovery.state === "available" ? "completed" : "pending",
    "sidecar-status-discovery",
  );
  item.id = "sidecar-status-discovery";
  item.description = view.discovery.state;
  item.iconPath = new vscode.ThemeIcon(
    view.discovery.state === "available" ? "search" : "warning",
  );
  item.children = children;
  item.tooltip = `Discovery: ${view.discovery.state}`;
  return item;
}

function discoveryChildren(discovery: SidecarDiscoveryView): ExosuitTreeItem[] {
  return compactItems([
    detailItem("Source", discovery.sourceSummary),
    detailItem("Registry", discovery.registry?.label ?? null),
    detailItem("Profile", discovery.registry?.profileRepo ?? null),
    detailItem("Match", matchDescription(discovery)),
    detailItem("Proposal", discovery.proposal?.key ?? null),
    detailItem("Root", discovery.proposal?.root ?? null),
    detailItem("Remote", discovery.proposal?.remote ?? null),
    detailItem("Failure", discovery.failure?.classification ?? null),
    detailItem("Message", discovery.failure?.message ?? null),
    detailItem(
      "Checked attempts",
      discovery.checked.length > 0 ? String(discovery.checked.length) : null,
    ),
    ...discovery.checked.map(checkedAttemptItem),
  ]);
}

function matchDescription(discovery: SidecarDiscoveryView): string | null {
  if (!discovery.match) {
    return null;
  }
  return discovery.match.key
    ? `${discovery.match.kind} ${discovery.match.key}`
    : discovery.match.kind;
}

function checkedAttemptItem(attempt: SidecarCheckedAttempt): ExosuitTreeItem {
  const item = new ExosuitTreeItem(
    `${attempt.attemptIndex} ${attempt.source}`,
    vscode.TreeItemCollapsibleState.None,
    "note",
    "pending",
    "sidecar-status-checked-attempt",
  );
  item.id = `sidecar-status-checked-attempt-${attempt.attemptIndex}`;
  item.description = attempt.status;
  item.iconPath = undefined;
  item.tooltip = [attempt.label, attempt.message].filter(Boolean).join("\n");
  return item;
}

function buildAction(
  action: SidecarPaneAction,
  index: number,
): ExosuitTreeItem {
  const item = new ExosuitTreeItem(
    action.label,
    vscode.TreeItemCollapsibleState.None,
    "note",
    "pending",
    "sidecar-status-action",
  );
  item.id = `sidecar-status-action-${index}`;
  item.description = action.kind;
  item.iconPath = new vscode.ThemeIcon("terminal");
  item.tooltip = [action.command, action.rationale].filter(Boolean).join("\n");
  item.command = {
    command: "exosuit.sidecar.runAction",
    title: action.label,
    arguments: [action],
  };
  return item;
}

function buildActions(view: SidecarStatusViewModel): ExosuitTreeItem | null {
  if (view.actions.length === 0) {
    return null;
  }

  const item = new ExosuitTreeItem(
    `Actions (${view.actions.length})`,
    vscode.TreeItemCollapsibleState.Expanded,
    "section",
    "pending",
    "sidecar-status-actions",
  );
  item.id = "sidecar-status-actions";
  item.iconPath = new vscode.ThemeIcon("run-all");
  item.children = view.actions.map(buildAction);
  return item;
}

function buildDiagnostics(
  view: SidecarStatusViewModel,
): ExosuitTreeItem | null {
  const errors = view.diagnostics.sources.filter(
    (source) => source.status === "error",
  );
  if (errors.length === 0) {
    return null;
  }

  const item = new ExosuitTreeItem(
    `Diagnostics (${errors.length})`,
    vscode.TreeItemCollapsibleState.Collapsed,
    "section",
    "pending",
    "sidecar-status-diagnostics",
  );
  item.id = "sidecar-status-diagnostics";
  item.iconPath = new vscode.ThemeIcon(
    "warning",
    new vscode.ThemeColor("charts.yellow"),
  );
  item.children = errors.map((source) => {
    const child = new ExosuitTreeItem(
      source.id,
      vscode.TreeItemCollapsibleState.None,
      "note",
      "pending",
      "sidecar-status-diagnostic",
    );
    child.id = `sidecar-status-diagnostic-${source.id}`;
    child.description = source.status;
    child.iconPath = new vscode.ThemeIcon("warning");
    child.tooltip = source.message ?? source.status;
    return child;
  });
  return item;
}

export function renderSidecarStatus(
  roots: ReadonlyMap<string, unknown>,
  diagnostics: ReadonlyMap<string, TraceCacheRootDiagnostic | undefined>,
): ExosuitTreeItem[] {
  const view = normalizeSources(roots, diagnostics);
  const items = [buildHeader(view), buildBinding(view), buildRepository(view)];
  const discovery = buildDiscovery(view);
  if (discovery) {
    items.push(discovery);
  }
  const actions = buildActions(view);
  if (actions) {
    items.push(actions);
  }
  const diagnosticsItem = buildDiagnostics(view);
  if (diagnosticsItem) {
    items.push(diagnosticsItem);
  }
  return items;
}

export function createSidecarStatusProvider() {
  return createTracedProvider<ExosuitTreeItem>(
    [...ROOT_IDS],
    renderSidecarStatus,
  );
}
