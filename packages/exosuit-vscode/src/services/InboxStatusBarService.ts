import * as vscode from "vscode";
import { getTraceCache } from "./TraceCache";
import { getLogger } from "../logging";

const logger = getLogger("extension");

interface InboxSnapshot {
  inbox?: { items?: { status?: string; intent?: string }[] };
}

function isAttentionInboxItem(item: {
  status?: string;
  intent?: string;
}): boolean {
  return item.status === "pending" && item.intent !== "claim";
}

export function formatInboxAttentionTooltip(count: number): string {
  return `${count} inbox item${count === 1 ? "" : "s"} needing attention - click to review`;
}

/**
 * Status bar service for the inbox.
 *
 * Shows a badge with the count of active inbox items. Clicking opens
 * the inbox quick pick. Subscribes to TraceCache for reactive updates.
 */
export class InboxStatusBarService implements vscode.Disposable {
  private statusBarItem: vscode.StatusBarItem;
  private captureButton: vscode.StatusBarItem;
  private disposables: vscode.Disposable[] = [];
  private _lastActiveCount = 0;
  private _onDidChange = new vscode.EventEmitter<number>();

  readonly onDidChange = this._onDidChange.event;

  constructor(_workspaceRoot: string) {
    this.statusBarItem = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      100,
    );
    this.statusBarItem.command = "exosuit.openInboxQuickPick";
    this.disposables.push(this.statusBarItem);

    this.captureButton = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      99,
    );
    this.captureButton.text = "$(plus)";
    this.captureButton.tooltip = "Capture Intent (Ctrl+Shift+I)";
    this.captureButton.command = "exosuit.captureIntent";
    this.captureButton.show();
    this.disposables.push(this.captureButton);

    const traceCache = getTraceCache();
    this.disposables.push(
      traceCache.onDidChange((rootId) => {
        if (rootId === "context-snapshot") {
          void this.updateStatusBar();
        }
      }),
    );

    this.disposables.push(this._onDidChange);
    void this.updateStatusBar();
  }

  private async updateStatusBar(): Promise<void> {
    try {
      const entry = await getTraceCache().get("context-snapshot");
      const snapshot = entry?.data as InboxSnapshot | null;
      const items = snapshot?.inbox?.items ?? [];
      const count = items.filter(isAttentionInboxItem).length;

      this._lastActiveCount = count;

      if (count === 0) {
        this.statusBarItem.hide();
      } else {
        this.statusBarItem.text = `$(inbox) ${count}`;
        this.statusBarItem.tooltip = formatInboxAttentionTooltip(count);
        this.statusBarItem.show();
      }

      this._onDidChange.fire(count);
    } catch (error) {
      logger.error("Failed to update inbox status bar:", error);
      this.statusBarItem.hide();
    }
  }

  getActiveCount(): number {
    return this._lastActiveCount;
  }

  refresh(): void {
    void this.updateStatusBar();
  }

  dispose() {
    this._lastActiveCount = 0;
    this.disposables.forEach((d) => d.dispose());
  }
}
