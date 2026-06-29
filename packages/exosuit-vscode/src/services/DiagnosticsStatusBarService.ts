import * as vscode from "vscode";

/**
 * Status bar service for workspace diagnostics.
 *
 * Shows error/warning counts from the VS Code Problems pane.
 * Subscribes directly to vscode.languages.onDidChangeDiagnostics.
 */
export class DiagnosticsStatusBarService implements vscode.Disposable {
  private readonly _statusBarItem: vscode.StatusBarItem;
  private readonly _disposables: vscode.Disposable[] = [];

  constructor() {
    this._statusBarItem = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Left,
      -100,
    );
    this._statusBarItem.command = "workbench.action.problems.focus";
    this._disposables.push(this._statusBarItem);

    this._disposables.push(
      vscode.languages.onDidChangeDiagnostics(() => this._update()),
    );

    this._update();
  }

  private _update(): void {
    let errorCount = 0;
    let warningCount = 0;

    for (const [, diagnostics] of vscode.languages.getDiagnostics()) {
      for (const d of diagnostics) {
        if (d.severity === vscode.DiagnosticSeverity.Error) {
          errorCount++;
        } else if (d.severity === vscode.DiagnosticSeverity.Warning) {
          warningCount++;
        }
      }
    }

    if (errorCount === 0 && warningCount === 0) {
      this._statusBarItem.hide();
      return;
    }

    const parts: string[] = [];
    if (errorCount > 0) {
      parts.push(`$(error) ${errorCount}`);
    }
    if (warningCount > 0) {
      parts.push(`$(warning) ${warningCount}`);
    }

    this._statusBarItem.text = parts.join("  ");
    this._statusBarItem.tooltip = new vscode.MarkdownString(
      `**Workspace Diagnostics**\n\n` +
        `Errors: ${errorCount}\n\n` +
        `Warnings: ${warningCount}\n\n` +
        `Click to open Problems pane`,
    );
    this._statusBarItem.backgroundColor =
      errorCount > 0
        ? new vscode.ThemeColor("statusBarItem.errorBackground")
        : undefined;
    this._statusBarItem.show();
  }

  dispose(): void {
    this._disposables.forEach((d) => d.dispose());
    this._disposables.length = 0;
  }
}
