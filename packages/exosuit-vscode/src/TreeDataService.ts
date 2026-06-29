import * as vscode from "vscode";
import type { PlanItem, Task, PhaseTask } from "@exosuit/core";
import { ExosuitParser } from "@exosuit/core";
import type { TreeItemType, TreeItemStatus } from "./TreeModel";
import { ExosuitTreeItem } from "./TreeModel";

/** RFC metadata fetched via machine channel */
export type RfcMetadata = {
  id: string;
  title: string;
  stage: number;
  filename: string;
};

export class TreeDataService {
  private static parser = new ExosuitParser();

  static async parsePlan(markdown: string): Promise<PlanItem[]> {
    const plan = await this.parser.parsePlan(markdown);
    return plan.items;
  }

  static convertPlanItems(
    items: PlanItem[],
    depth: number = 0,
    viewType?: "project-plan" | "epoch-details",
    parentPath?: string,
  ): ExosuitTreeItem[] {
    return items.map((item) => {
      const type = item.type || this.determineType(item, depth);
      const treeItemId = parentPath ? `${parentPath}/${item.id}` : item.id;

      let collapsibleState = vscode.TreeItemCollapsibleState.None;
      if (item.children && item.children.length > 0) {
        if (viewType === "epoch-details") {
          collapsibleState = vscode.TreeItemCollapsibleState.None;
        } else {
          collapsibleState = vscode.TreeItemCollapsibleState.Expanded;
        }
      }

      let contextValue = `exosuit-${type}`;
      if (type === "task") {
        contextValue = "phase-task";
      } else if (type === "section") {
        contextValue = "section";
        collapsibleState = vscode.TreeItemCollapsibleState.Expanded;
      }

      const treeItem = new ExosuitTreeItem(
        item.title,
        collapsibleState,
        type as TreeItemType,
        this.mapStatus(item.status),
        contextValue,
      );
      treeItem.id = treeItemId;

      if (type === "phase") {
        treeItem.command = {
          command: "exosuit.focusPhase",
          title: "Focus Phase",
          arguments: [item.id],
        };
      }

      if (item.children) {
        treeItem.children = this.convertPlanItems(
          item.children,
          depth + 1,
          viewType,
          treeItemId,
        );
      }

      return treeItem;
    });
  }

  static convertPhaseTasks(tasks: PhaseTask[]): ExosuitTreeItem[] {
    return tasks.map((task) => {
      const treeItem = new ExosuitTreeItem(
        task.title,
        vscode.TreeItemCollapsibleState.None,
        "task",
        this.mapStatus(task.status),
        "phase-task",
      );
      treeItem.id = task.id;
      treeItem.tooltip = task.description;
      return treeItem;
    });
  }

  static convertTasks(tasks: Task[]): ExosuitTreeItem[] {
    return tasks.map((task) => {
      return new ExosuitTreeItem(
        task.title,
        vscode.TreeItemCollapsibleState.None,
        "task",
        this.mapStatus(task.status),
        "phase-task",
      );
    });
  }

  private static mapStatus(status: string): TreeItemStatus {
    switch (status) {
      case "done":
        return "completed";
      case "in-progress":
        return "in-progress";
      case "skipped":
        return "skipped";
      case "abandoned":
        return "abandoned";
      default:
        return "pending";
    }
  }

  private static determineType(_item: PlanItem, depth: number): TreeItemType {
    if (depth === 0) {
      return "epoch";
    }
    if (depth === 1) {
      return "phase";
    }
    if (depth >= 2) {
      return "task";
    }
    return "section";
  }
}
