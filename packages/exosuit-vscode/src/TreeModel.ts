import * as vscode from "vscode";

export type TreeItemType =
  | "epoch"
  | "phase"
  | "section"
  | "goal"
  | "task"
  | "note";
export type TreeItemStatus =
  | "completed"
  | "pending"
  | "in-progress"
  | "ready-for-logging"
  | "phase-active"
  | "skipped"
  | "abandoned";

export class ExosuitTreeItem extends vscode.TreeItem {
  public children: ExosuitTreeItem[] = [];

  constructor(
    public readonly label: string,
    public collapsibleState: vscode.TreeItemCollapsibleState,
    public readonly type: TreeItemType,
    public readonly status: TreeItemStatus = "pending",
    public readonly contextValue: string = "exosuitItem",
  ) {
    super(label, collapsibleState);
    this.tooltip = `${this.label} (${this.status})`;
    this.description = type === "task" ? "" : status; // Only show status text for non-tasks to avoid clutter

    this.iconPath = this.getIcon(type, status);

    if (type === "task") {
      if (this.contextValue === "phase-task-readonly") {
        this.checkboxState = undefined;
        // Zero-width icon trick: iconPath left undefined (set by getIcon → undefined).
        // Caller prefixes label with Unicode glyph; FileDecorationProvider colors it (RFC 10169).
        // Add description for in-progress tasks to make them stand out
        if (status === "in-progress") {
          this.description = "● Active";
        }
      } else {
        this.checkboxState =
          status === "completed"
            ? vscode.TreeItemCheckboxState.Checked
            : vscode.TreeItemCheckboxState.Unchecked;
        // For checkbox tasks, add visual indicator for in-progress
        if (status === "in-progress") {
          this.description = "● Active";
        }
      }
    }
  }

  private getIcon(
    type: TreeItemType,
    status: TreeItemStatus,
  ): vscode.ThemeIcon | undefined {
    if (type === "task") {
      // If it's a task, we handled it in constructor based on contextValue
      // But if we fall through here, we might want to return undefined if checkbox is used
      if (this.contextValue !== "phase-task-readonly") {
        return undefined;
      }
      // If read-only, we already set iconPath in constructor, but getIcon is called for initial iconPath assignment?
      // Wait, in constructor: this.iconPath = this.getIcon(type, status);
      // So I should handle it here or override it after.
      return undefined;
    }

    if (type === "phase") {
      switch (status) {
        case "completed":
          return new vscode.ThemeIcon(
            "check-all",
            new vscode.ThemeColor("charts.green"),
          );
        case "in-progress":
          return new vscode.ThemeIcon(
            "play-circle",
            new vscode.ThemeColor("charts.blue"),
          );
        case "skipped":
          return new vscode.ThemeIcon(
            "circle-slash",
            new vscode.ThemeColor("charts.gray"),
          );
        default:
          return new vscode.ThemeIcon("list-unordered");
      }
    }

    if (type === "epoch") {
      switch (status) {
        case "completed":
          return new vscode.ThemeIcon(
            "milestone",
            new vscode.ThemeColor("charts.green"),
          );
        case "in-progress":
          return new vscode.ThemeIcon(
            "milestone",
            new vscode.ThemeColor("charts.blue"),
          );
        default:
          return new vscode.ThemeIcon("milestone");
      }
    }

    if (type === "goal") {
      switch (status) {
        case "completed":
          return new vscode.ThemeIcon(
            "pass-filled",
            new vscode.ThemeColor("charts.green"),
          );
        case "ready-for-logging":
          // All tasks done but no completion log yet — done but needs confirmation
          return new vscode.ThemeIcon(
            "pass",
            new vscode.ThemeColor("charts.green"),
          );
        case "in-progress":
          return new vscode.ThemeIcon(
            "target",
            new vscode.ThemeColor("charts.blue"),
          );
        case "abandoned":
          return new vscode.ThemeIcon(
            "circle-slash",
            new vscode.ThemeColor("charts.gray"),
          );
        default:
          return new vscode.ThemeIcon("circle-large-outline");
      }
    }

    if (type === "section") {
      return new vscode.ThemeIcon("symbol-structure");
    }

    return new vscode.ThemeIcon("symbol-text");
  }
}
