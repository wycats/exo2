import * as vscode from "vscode";
import { TreeDataService } from "./TreeDataService";
import { type ExosuitTreeItem } from "./TreeModel";
import { LogService } from "./LogService";
import { PlanService } from "./PlanService";
import type { PlanItem } from "@exosuit/core";
import { ErrorItem } from "./ErrorItem";
import { getTraceCache } from "./services/TraceCache";

const DEFAULT_RETRY_BACKOFF_MS = [2000, 5000, 10000] as const;

interface ExosuitTreeProviderOptions {
  retryBackoffMs?: readonly number[];
  maxEmptyRetries?: number;
}

export class ExosuitTreeProvider implements vscode.TreeDataProvider<
  ExosuitTreeItem | ErrorItem
> {
  private _onDidChangeTreeData: vscode.EventEmitter<
    ExosuitTreeItem | ErrorItem | undefined | null | void
  > = new vscode.EventEmitter<
    ExosuitTreeItem | ErrorItem | undefined | null | void
  >();
  readonly onDidChangeTreeData: vscode.Event<
    ExosuitTreeItem | ErrorItem | undefined | null | void
  > = this._onDidChangeTreeData.event;

  private _logger: LogService;
  private _focusedPhaseId: string | undefined;
  private _planService: PlanService;
  private _reactivityDisposable: vscode.Disposable | undefined;
  private _retryCount = 0;
  private _retryTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly _retryBackoffMs: readonly number[];
  private readonly _maxEmptyRetries: number;

  constructor(
    private readonly _type: "project-plan" | "epoch-details" | "current-phase",
    options: ExosuitTreeProviderOptions = {},
  ) {
    this._logger = LogService.instance;
    this._planService = PlanService.instance;
    this._retryBackoffMs = options.retryBackoffMs ?? DEFAULT_RETRY_BACKOFF_MS;
    this._maxEmptyRetries =
      options.maxEmptyRetries ?? this._retryBackoffMs.length;

    // Initialize file watcher via Service
    this.setupWatcher();
  }

  setFocus(phaseId: string | undefined) {
    this._focusedPhaseId = phaseId;
    this.refresh();
  }

  private setupWatcher() {
    this._reactivityDisposable = getTraceCache().onDidWrite(() => {
      this._planService.invalidate();
      this.refresh();
    });
  }

  dispose() {
    this._reactivityDisposable?.dispose();
    this.clearRetry();
  }

  refresh(): void {
    this._onDidChangeTreeData.fire();
  }

  private scheduleRetry(): void {
    if (
      this._retryTimer ||
      this._retryBackoffMs.length === 0 ||
      this._retryCount >= this._maxEmptyRetries
    ) {
      return;
    }

    const delay =
      this._retryBackoffMs[
        Math.min(this._retryCount, this._retryBackoffMs.length - 1)
      ];
    this._retryCount++;
    this._retryTimer = setTimeout(() => {
      this._retryTimer = undefined;
      this._planService.invalidate();
      this.refresh();
    }, delay);
  }

  private clearRetry(): void {
    this._retryCount = 0;
    if (this._retryTimer) {
      clearTimeout(this._retryTimer);
      this._retryTimer = undefined;
    }
  }

  getTreeItem(element: ExosuitTreeItem | ErrorItem): vscode.TreeItem {
    return element;
  }

  async getChildren(
    element?: ExosuitTreeItem | ErrorItem,
  ): Promise<(ExosuitTreeItem | ErrorItem)[]> {
    if (element instanceof ErrorItem) {
      return [];
    }

    if (element) {
      return element.children;
    }

    // Root
    try {
      const planItems = await this._planService.getPlan();
      if (planItems.length === 0) {
        this.scheduleRetry();
      } else {
        this.clearRetry();
      }

      if (this._type === "project-plan") {
        return TreeDataService.convertPlanItems(planItems, 0, "project-plan");
      } else if (this._type === "epoch-details") {
        let targetEpoch: PlanItem | undefined;

        if (this._focusedPhaseId) {
          targetEpoch = planItems.find(
            (epoch) =>
              epoch.id === this._focusedPhaseId ||
              (epoch.children &&
                epoch.children.some(
                  (phase) => phase.id === this._focusedPhaseId,
                )),
          );
        }

        if (!targetEpoch) {
          targetEpoch =
            planItems.find((e) => e.status === "in-progress") ||
            planItems.find((e) => e.status === "todo") ||
            [...planItems].reverse().find((e) => e.status === "done") ||
            planItems[0];
        }

        if (targetEpoch) {
          return TreeDataService.convertPlanItems(
            targetEpoch.children || [],
            1,
            "epoch-details",
          );
        }
        return [];
      }

      return [];
    } catch (error) {
      this._logger.log("TreeProvider", `Error fetching content: ${error}`);
      this.scheduleRetry();
      return [
        new ErrorItem(
          "Failed to load plan",
          error instanceof Error ? error.message : String(error),
        ),
      ];
    }
  }

  // Helper to get the title of the focused epoch for the view description
  async getFocusedEpochTitle(): Promise<string | undefined> {
    if (this._type !== "epoch-details") {
      return undefined;
    }

    const planItems = await this._planService.getPlan();
    let targetEpoch = planItems.find(
      (epoch) =>
        epoch.id === this._focusedPhaseId ||
        (epoch.children &&
          epoch.children.some((phase) => phase.id === this._focusedPhaseId)),
    );

    if (!targetEpoch) {
      targetEpoch =
        planItems.find((e) => e.status === "in-progress") || planItems[0];
    }
    return targetEpoch?.title;
  }

  async getActivePhaseId(): Promise<string | undefined> {
    return this._planService.getActivePhaseId();
  }

  async getItem(id: string): Promise<ExosuitTreeItem | undefined> {
    const planItem = await this._planService.getItem(id);
    if (planItem) {
      const parent = await this._planService.getParent(id);
      let depth = 0;
      let parentPath: string | undefined;

      if (parent) {
        const grandParent = await this._planService.getParent(parent.id);
        if (grandParent) {
          depth = 2; // Task (Child of Phase, which is child of Epoch)
          parentPath = `${grandParent.id}/${parent.id}`;
        } else {
          depth = 1; // Phase (Child of Epoch)
          parentPath = parent.id;
        }
      } else {
        depth = 0; // Epoch (Root)
        parentPath = undefined;
      }

      const viewType =
        this._type === "current-phase" ? "project-plan" : this._type;
      const treeItems = TreeDataService.convertPlanItems(
        [planItem],
        depth,
        viewType,
        parentPath,
      );
      return treeItems[0];
    }
    return undefined;
  }

  async getParent(
    element: ExosuitTreeItem,
  ): Promise<ExosuitTreeItem | undefined> {
    if (this._type === "project-plan" && element.id) {
      // The element.id is now a path like "epoch-0/phase-1/task-1"
      // Extract the actual item ID (last segment)
      const pathSegments = element.id.split("/");
      const itemId = pathSegments[pathSegments.length - 1];

      const parentPlanItem = await this._planService.getParent(itemId);
      if (parentPlanItem) {
        const grandParent = await this._planService.getParent(
          parentPlanItem.id,
        );
        const depth = grandParent ? 1 : 0;
        const parentPath = grandParent ? grandParent.id : undefined;

        const parentItem = TreeDataService.convertPlanItems(
          [parentPlanItem],
          depth,
          "project-plan",
          parentPath,
        )[0];

        return parentItem;
      }
    }
    return undefined;
  }
}
