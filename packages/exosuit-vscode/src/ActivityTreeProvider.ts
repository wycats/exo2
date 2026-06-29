import * as vscode from "vscode";
import type { ActivityEvent, ActivityItemData } from "./LogService";
import { LogService } from "./LogService";

type TreeItemData =
  | { kind: "event"; data: ActivityEvent }
  | { kind: "detail"; data: string }
  | { kind: "subitem"; data: ActivityItemData };

class ActivityTreeItem extends vscode.TreeItem {
  constructor(
    public readonly item: TreeItemData,
    previousTimestamp: number | undefined,
    isTopItem: boolean = false
  ) {
    super(ActivityTreeItem.getLabel(item), ActivityTreeItem.getState(item));

    this.configure(item, previousTimestamp, isTopItem);
  }

  private static getLabel(item: TreeItemData): string {
    switch (item.kind) {
      case "event":
        return item.data.label;
      case "detail":
        return item.data;
      case "subitem":
        return item.data.label;
    }
  }

  private static getState(item: TreeItemData): vscode.TreeItemCollapsibleState {
    if (item.kind === "event") {
      return item.data.details ||
        (item.data.items && item.data.items.length > 0)
        ? vscode.TreeItemCollapsibleState.Collapsed
        : vscode.TreeItemCollapsibleState.None;
    }
    return vscode.TreeItemCollapsibleState.None;
  }

  private configure(
    item: TreeItemData,
    previousTimestamp: number | undefined,
    isTopItem: boolean
  ) {
    if (item.kind === "event") {
      // Timestamp logic
      if (isTopItem) {
        this.description = new Date(item.data.timestamp).toLocaleTimeString();
      } else if (previousTimestamp) {
        const delta = item.data.timestamp - previousTimestamp;
        if (delta < 1000) {
          this.description = ""; // "Instant"
        } else {
          this.description = `+${(delta / 1000).toFixed(1)}s`;
        }
      } else {
        // Oldest item
        this.description = new Date(item.data.timestamp).toLocaleTimeString();
      }

      this.iconPath = item.data.icon
        ? new vscode.ThemeIcon(item.data.icon)
        : new vscode.ThemeIcon("circle-outline");
      this.contextValue = "activity-event";
      this.tooltip = `${item.data.label} (${item.data.type})`;

      if (item.data.file) {
        this.contextValue = "activity-file";
        this.tooltip += `\nFile: ${item.data.file}`;
      }
    } else if (item.kind === "detail") {
      this.iconPath = new vscode.ThemeIcon("note");
      this.contextValue = "activity-detail";
      this.tooltip = item.data;
    } else if (item.kind === "subitem") {
      this.description = item.data.description;
      this.iconPath = item.data.icon
        ? new vscode.ThemeIcon(item.data.icon)
        : undefined;
      this.tooltip = item.data.tooltip;
      this.contextValue = "activity-subitem";

      if (item.data.file) {
        this.contextValue = "activity-file";
        this.tooltip += `\nFile: ${item.data.file}`;
      }
    }
  }
}

export class ActivityTreeProvider
  implements vscode.TreeDataProvider<ActivityTreeItem>
{
  private _onDidChangeTreeData: vscode.EventEmitter<
    ActivityTreeItem | undefined | null | void
  > = new vscode.EventEmitter<ActivityTreeItem | undefined | null | void>();
  readonly onDidChangeTreeData: vscode.Event<
    ActivityTreeItem | undefined | null | void
  > = this._onDidChangeTreeData.event;

  constructor() {
    LogService.instance.onLog(() => {
      this._onDidChangeTreeData.fire();
    });
  }

  getTreeItem(element: ActivityTreeItem): vscode.TreeItem {
    return element;
  }

  getChildren(
    element?: ActivityTreeItem
  ): vscode.ProviderResult<ActivityTreeItem[]> {
    if (element) {
      const items: ActivityTreeItem[] = [];

      if (element.item.kind === "event") {
        const event = element.item.data;

        // Add details as a child if present
        if (event.details) {
          items.push(
            new ActivityTreeItem(
              { kind: "detail", data: event.details },
              undefined
            )
          );
        }

        // Add sub-items if present
        if (event.items) {
          items.push(
            ...event.items.map(
              (i) =>
                new ActivityTreeItem({ kind: "subitem", data: i }, undefined)
            )
          );
        }
      }

      return items;
    }

    // Root items: The logs in reverse order (newest first)
    const logs = LogService.instance.getLogs().reverse();

    // We want to calculate deltas based on chronological order (oldest to newest)
    // But we display newest first.
    // So logs[0] is newest. logs[1] is older.
    // Delta for logs[0] = logs[0].timestamp - logs[1].timestamp

    return logs.map((event, index) => {
      const previousEvent = logs[index + 1];
      const previousTimestamp = previousEvent
        ? previousEvent.timestamp
        : undefined;
      return new ActivityTreeItem(
        { kind: "event", data: event },
        previousTimestamp,
        index === 0
      );
    });
  }
}
