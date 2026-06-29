import * as vscode from "vscode";
import { PlanService } from "../PlanService";
import { getTraceCache } from "./TraceCache";
import { getLogger } from "../logging";

const logger = getLogger("extension");

interface DaemonPhaseDetails {
  phaseId: string | null;
  phaseTitle: string | null;
  epochId: string | null;
  epochTitle: string | null;
  progress: { goalsCompleted: number; goalsTotal: number };
}

/**
 * Status bar service for the active phase.
 *
 * Shows the current phase name and task progress in the status bar.
 * Subscribes to TraceCache for reactive updates.
 */
export class PhaseStatusBarService implements vscode.Disposable {
  private planService: PlanService;
  private statusBarItem: vscode.StatusBarItem;
  private disposables: vscode.Disposable[] = [];
  private _treeView: vscode.TreeView<unknown> | undefined;

  constructor() {
    this.planService = PlanService.instance;
    this.statusBarItem = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Left,
      100,
    );
    this.statusBarItem.command = "exosuit.revealActivePhase";
    this.disposables.push(this.statusBarItem);

    const traceCache = getTraceCache();
    this.disposables.push(
      traceCache.onDidChange((rootId) => {
        if (rootId === "phase-details") {
          this.planService.invalidate();
          void this.updateStatusBar();
        }
      }),
    );

    void this.updateStatusBar();
  }

  /** Set the tree view to update its description with the active phase. */
  setTreeView(treeView: vscode.TreeView<unknown>): void {
    this._treeView = treeView;
    // Update description immediately
    this.updateStatusBar();
  }

  private async updateStatusBar(): Promise<void> {
    try {
      const plan = await this.planService.getPlan();
      const hasPlan = plan.length > 0;

      const entry = await getTraceCache().get("phase-details");
      const details = entry?.data as DaemonPhaseDetails | null;

      await vscode.commands.executeCommand(
        "setContext",
        "exosuit.hasPlan",
        hasPlan,
      );

      if (details?.phaseId) {
        const progressText =
          details.progress.goalsTotal > 0
            ? ` (${details.progress.goalsCompleted}/${details.progress.goalsTotal})`
            : "";
        this.statusBarItem.text = `$(rocket) ${details.phaseTitle}${progressText}`;
        this.statusBarItem.tooltip = details.epochTitle
          ? `Active Phase: ${details.phaseTitle}\nEpoch: ${details.epochTitle}\n\nClick to reveal in Project Plan`
          : `Active Phase: ${details.phaseTitle}\n\nClick to reveal in Project Plan`;
        this.statusBarItem.show();

        if (this._treeView) {
          this._treeView.description = details.phaseTitle ?? undefined;
        }

        await vscode.commands.executeCommand(
          "setContext",
          "exosuit.hasActivePhase",
          true,
        );
        await vscode.commands.executeCommand(
          "setContext",
          "exosuit.activePhaseId",
          details.phaseId,
        );
      } else {
        this.statusBarItem.text = "$(rocket) No active phase";
        this.statusBarItem.tooltip =
          "No active phase\n\nClick to open Project Plan";
        this.statusBarItem.show();

        if (this._treeView) {
          this._treeView.description = undefined;
        }

        await vscode.commands.executeCommand(
          "setContext",
          "exosuit.hasActivePhase",
          false,
        );
        await vscode.commands.executeCommand(
          "setContext",
          "exosuit.activePhaseId",
          undefined,
        );
      }
    } catch (error) {
      logger.error("Failed to update phase status bar:", error);
      this.statusBarItem.hide();
    }
  }

  refresh(): void {
    void this.updateStatusBar();
  }

  dispose(): void {
    for (const d of this.disposables) {
      d.dispose();
    }
  }
}
