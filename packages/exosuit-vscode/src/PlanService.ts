import { LogService } from "./LogService";
import { PerformanceMonitor } from "./PerformanceMonitor";
import type { PlanItem, PlanStruct, PhaseTask } from "@exosuit/core";
import { PlanSchema, PhaseTaskSchema } from "@exosuit/core";
import { exoMachineChannel } from "./agent/lmtool/machineChannel";
import { currentWorkspaceRoot } from "./workspaceRoot";

type PlanItemStatus = "todo" | "in-progress" | "done" | "skipped";

export class PlanService {
  private static _instance: PlanService;
  private _planCache: PlanItem[] | null = null;
  private _planPromise: Promise<PlanItem[]> | null = null;
  private _phaseTasksCache: PhaseTask[] | null = null;
  private _phaseTasksPromise: Promise<PhaseTask[]> | null = null;
  private _phaseGoalsCache: PhaseTask[] | null = null;
  private _phaseGoalsPromise: Promise<PhaseTask[]> | null = null;
  private _generation = 0;
  private _logger: LogService;

  private constructor() {
    this._logger = LogService.instance;
  }

  public static get instance(): PlanService {
    if (!this._instance) {
      this._instance = new PlanService();
    }
    return this._instance;
  }

  public invalidate() {
    this._generation++;
    this._planCache = null;
    this._planPromise = null;
    this._phaseTasksCache = null;
    this._phaseTasksPromise = null;
    this._phaseGoalsCache = null;
    this._phaseGoalsPromise = null;
  }

  public async getPhaseGoals(): Promise<PhaseTask[]> {
    return PerformanceMonitor.measure("PlanService.getPhaseGoals", async () => {
      if (this._phaseGoalsCache) {
        return this._phaseGoalsCache;
      }
      if (this._phaseGoalsPromise) {
        return this._phaseGoalsPromise;
      }

      const generation = this._generation;
      this._phaseGoalsPromise = this.loadPhaseGoals(generation);
      try {
        return await this._phaseGoalsPromise;
      } finally {
        if (this._phaseGoalsPromise && this._generation === generation) {
          this._phaseGoalsPromise = null;
        }
      }
    });
  }

  private async loadPhaseGoals(generation: number): Promise<PhaseTask[]> {
    try {
      const wsRoot = currentWorkspaceRoot();
      if (wsRoot) {
        try {
          const response = await exoMachineChannel(wsRoot, {
            protocol_version: 1,
            id: `vscode.phase.goals.read`,
            op: {
              kind: "call",
              params: {
                address: { kind: "operation", path: ["phase", "read-goals"] },
                input: {},
              },
            },
          });

          if (response.status === "ok" && Array.isArray(response.result)) {
            const goals = response.result as PhaseTask[];
            if (this._generation !== generation) {
              return [];
            }
            this._phaseGoalsCache = goals;
            return goals;
          }

          this._logger.log(
            "Warning",
            `phase.read-goals returned status=${response.status}`,
          );
        } catch (daemonErr) {
          this._logger.log(
            "Warning",
            `phase.read-goals daemon call failed: ${daemonErr}`,
          );
        }
      }
      return [];
    } catch (e) {
      this._logger.log("Error", `Failed to load phase goals: ${e}`);
      throw e;
    }
  }

  public async getPhaseTasks(): Promise<PhaseTask[]> {
    return PerformanceMonitor.measure("PlanService.getPhaseTasks", async () => {
      if (this._phaseTasksCache) {
        return this._phaseTasksCache;
      }
      if (this._phaseTasksPromise) {
        return this._phaseTasksPromise;
      }

      const generation = this._generation;
      this._phaseTasksPromise = this.loadPhaseTasks(generation);
      try {
        return await this._phaseTasksPromise;
      } finally {
        if (this._phaseTasksPromise && this._generation === generation) {
          this._phaseTasksPromise = null;
        }
      }
    });
  }

  private async loadPhaseTasks(generation: number): Promise<PhaseTask[]> {
    try {
      const wsRoot = currentWorkspaceRoot();
      if (wsRoot) {
        try {
          const response = await exoMachineChannel(wsRoot, {
            protocol_version: 1,
            id: `vscode.phase.tasks.read`,
            op: {
              kind: "call",
              params: {
                address: { kind: "operation", path: ["phase", "read-tasks"] },
                input: {},
              },
            },
          });

          if (response.status === "ok" && Array.isArray(response.result)) {
            const tasks = response.result.map((task) =>
              PhaseTaskSchema.parse(task),
            );
            if (this._generation !== generation) {
              return [];
            }
            this._phaseTasksCache = tasks;
            return tasks;
          }

          this._logger.log(
            "Warning",
            `phase.read-tasks returned status=${response.status}`,
          );
        } catch (daemonErr) {
          this._logger.log(
            "Warning",
            `phase.read-tasks daemon call failed: ${daemonErr}`,
          );
        }
      }
      return [];
    } catch (e) {
      this._logger.log("Error", `Failed to load phase tasks: ${e}`);
      throw e;
    }
  }

  public async getPlan(): Promise<PlanItem[]> {
    return PerformanceMonitor.measure("PlanService.getPlan", async () => {
      if (this._planCache) {
        return this._planCache;
      }
      if (this._planPromise) {
        return this._planPromise;
      }

      const generation = this._generation;
      this._planPromise = this.loadPlan(generation);
      try {
        return await this._planPromise;
      } finally {
        if (this._planPromise && this._generation === generation) {
          this._planPromise = null;
        }
      }
    });
  }

  private async loadPlan(generation: number): Promise<PlanItem[]> {
    try {
      // Read plan from daemon via machine channel
      const wsRoot = currentWorkspaceRoot();
      if (wsRoot) {
        try {
          const response = await exoMachineChannel(wsRoot, {
            protocol_version: 1,
            id: `vscode.plan.read`,
            op: {
              kind: "call",
              params: {
                address: { kind: "operation", path: ["plan", "read"] },
                input: {},
              },
            },
          });
          if (response.status === "ok" && response.result) {
            const plan = PlanSchema.parse(response.result);
            const items = this.mapPlanToItems(plan);
            if (this._generation !== generation) {
              return [];
            }
            this._planCache = items;
            return items;
          }
          this._logger.log(
            "Warning",
            `plan.read returned status=${response.status}`,
          );
        } catch (daemonErr) {
          this._logger.log(
            "Warning",
            `plan.read daemon call failed: ${daemonErr}`,
          );
        }
      }
      return [];
    } catch (e) {
      this._logger.log("Error", `Failed to load plan: ${e}`);
      throw e;
    }
  }

  private mapPlanToItems(plan: PlanStruct): PlanItem[] {
    return plan.epochs.map((epoch) => ({
      id: epoch.id,
      title: epoch.title,
      status: this.mapStatus(epoch.status),
      type: "epoch",
      children: epoch.phases.map((phase) => ({
        id: phase.id,
        title: phase.title,
        status: this.mapStatus(phase.status),
        type: "phase",
        children: phase.goals.map((goal) => ({
          id: goal.id,
          title: goal.label,
          status: this.mapStatus(goal.status),
          type: "task",
          children: [],
        })),
      })),
    }));
  }

  private mapStatus(status: string): PlanItemStatus {
    switch (status) {
      case "completed":
        return "done";
      case "in-progress":
        return "in-progress";
      case "skipped":
      case "abandoned":
        return "skipped";
      case "deferred":
        return "todo";
      default:
        return "todo";
    }
  }

  public async getItem(id: string): Promise<PlanItem | undefined> {
    const plan = await this.getPlan();
    return this.findItem(plan, id);
  }

  private findItem(items: PlanItem[], id: string): PlanItem | undefined {
    for (const item of items) {
      if (item.id === id) {
        return item;
      }
      if (item.children) {
        const found = this.findItem(item.children, id);
        if (found) {
          return found;
        }
      }
    }
    return undefined;
  }

  public async getParent(id: string): Promise<PlanItem | undefined> {
    const plan = await this.getPlan();
    return this.findParent(plan, id);
  }

  private findParent(items: PlanItem[], childId: string): PlanItem | undefined {
    for (const item of items) {
      if (
        item.children &&
        item.children.some((child) => child.id === childId)
      ) {
        return item;
      }
      if (item.children) {
        const found = this.findParent(item.children, childId);
        if (found) {
          return found;
        }
      }
    }
    return undefined;
  }

  public async getActivePhaseId(): Promise<string | undefined> {
    const plan = await this.getPlan();
    let activeId: string | undefined;
    const findActive = (items: PlanItem[]) => {
      for (const item of items) {
        if (item.status === "in-progress" && item.type === "phase") {
          activeId = item.id;
          return;
        }
        if (item.children) {
          findActive(item.children);
        }
      }
    };
    findActive(plan);
    return activeId;
  }

  public async moveTasks(
    sourcePhaseId: string,
    targetPhaseId: string,
    taskIds: string[],
  ): Promise<void> {
    const wsRoot = currentWorkspaceRoot();
    if (wsRoot) {
      const response = await exoMachineChannel(wsRoot, {
        protocol_version: 1,
        id: `vscode.plan.move-goals`,
        op: {
          kind: "call",
          params: {
            address: { kind: "operation", path: ["plan", "move-goals"] },
            input: {
              source_phase_id: sourcePhaseId,
              target_phase_id: targetPhaseId,
              goal_ids: taskIds,
            },
          },
        },
      });

      if (response.status === "ok") {
        this.invalidate();
        return;
      }

      this._logger.log(
        "Warning",
        `plan.move-goals returned status=${response.status}`,
      );
    }
  }
}
