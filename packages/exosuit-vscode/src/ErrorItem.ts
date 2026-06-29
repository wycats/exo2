import * as vscode from "vscode";

export class ErrorItem extends vscode.TreeItem {
  constructor(message: string, detail?: string) {
    super(message, vscode.TreeItemCollapsibleState.None);
    this.description = detail;
    this.iconPath = new vscode.ThemeIcon(
      "error",
      new vscode.ThemeColor("errorForeground")
    );
    this.tooltip = detail ? `${message}\n${detail}` : message;
    this.contextValue = "error";
  }
}
