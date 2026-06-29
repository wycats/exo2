import * as vscode from "vscode";

import { readExosuitTaskConfig } from "./exosuitTaskConfig";

export type ExosuitTaskDefinition = {
  type: "exosuit";
  task: string;
};

function toWorkspaceFolder(
  scope: vscode.TaskScope | vscode.WorkspaceFolder | undefined
): vscode.WorkspaceFolder | undefined {
  if (!scope) {
    return undefined;
  }
  if (typeof scope === "number") {
    return undefined;
  }
  return scope;
}

function makeTask(
  folder: vscode.WorkspaceFolder,
  id: string,
  desc?: string
): vscode.Task {
  const definition: ExosuitTaskDefinition = { type: "exosuit", task: id };

  const execution = new vscode.ProcessExecution("exo", ["run", id], {
    cwd: folder.uri.fsPath,
  });

  const task = new vscode.Task(definition, folder, id, "exosuit", execution);
  task.detail = desc;
  return task;
}

export class ExosuitTaskProvider implements vscode.TaskProvider {
  static readonly type = "exosuit";

  async provideTasks(): Promise<vscode.Task[]> {
    const folders = vscode.workspace.workspaceFolders ?? [];
    const out: vscode.Task[] = [];

    for (const folder of folders) {
      const entries = readExosuitTaskConfig(folder.uri.fsPath);
      for (const entry of entries) {
        out.push(makeTask(folder, entry.id, entry.desc));
      }
    }

    return out;
  }

  async resolveTask(task: vscode.Task): Promise<vscode.Task | undefined> {
    const def = task.definition as Partial<ExosuitTaskDefinition> | undefined;
    const id = typeof def?.task === "string" ? def.task : undefined;

    if (!id) {
      return undefined;
    }

    const folder =
      toWorkspaceFolder(task.scope as any) ??
      vscode.workspace.workspaceFolders?.[0];

    if (!folder) {
      return undefined;
    }

    const entries = readExosuitTaskConfig(folder.uri.fsPath);
    const match = entries.find((e) => e.id === id);

    return makeTask(folder, id, match?.desc ?? task.detail);
  }
}
