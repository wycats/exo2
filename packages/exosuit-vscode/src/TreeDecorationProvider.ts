import * as vscode from "vscode";

/**
 * RFC 10169: FileDecoration-based tree item styling.
 *
 * Uses VS Code's FileDecorationProvider + resourceUri to color tree item
 * labels. Combined with the zero-width icon trick (iconPath = undefined),
 * this enables compact, colorful tree items without the 20px icon gutter.
 *
 * URI scheme: exosuit-tree://<type>/<status>/<id>
 *   e.g. exosuit-tree://task/completed/my-task-id
 *        exosuit-tree://goal/in-progress/my-goal-id
 */

export const TREE_URI_SCHEME = "exosuit-tree";

/** Build a resourceUri encoding type and status for FileDecoration. */
export function treeItemUri(
  type: string,
  status: string,
  id: string,
): vscode.Uri {
  return vscode.Uri.parse(
    `${TREE_URI_SCHEME}://${type}/${encodeURIComponent(status)}/${encodeURIComponent(id)}`,
  );
}

/** Status → ThemeColor mapping for tree item labels. */
const STATUS_COLORS: Record<string, vscode.ThemeColor> = {
  completed: new vscode.ThemeColor("charts.green"),
  "in-progress": new vscode.ThemeColor("charts.yellow"),
  "ready-for-logging": new vscode.ThemeColor("charts.green"),
  "phase-active": new vscode.ThemeColor("charts.blue"),
  abandoned: new vscode.ThemeColor("charts.gray"),
  skipped: new vscode.ThemeColor("charts.gray"),
  // "pending" intentionally omitted → no color (default foreground)
};

export class TreeDecorationProvider
  implements vscode.FileDecorationProvider, vscode.Disposable
{
  private readonly _onDidChangeFileDecorations = new vscode.EventEmitter<
    vscode.Uri | vscode.Uri[] | undefined
  >();
  readonly onDidChangeFileDecorations = this._onDidChangeFileDecorations.event;

  provideFileDecoration(
    uri: vscode.Uri,
  ): vscode.ProviderResult<vscode.FileDecoration> {
    if (uri.scheme !== TREE_URI_SCHEME) {
      return undefined;
    }

    // Parse: authority = type, path = /<status>/<id>
    const parts = uri.path.split("/").filter(Boolean);
    if (parts.length < 2) {
      return undefined;
    }

    const status = decodeURIComponent(parts[0]);
    const color = STATUS_COLORS[status];

    if (!color) {
      return undefined;
    }

    return new vscode.FileDecoration(undefined, undefined, color);
  }

  /** Signal that decorations should be recomputed. */
  fireChange(uris?: vscode.Uri[]): void {
    this._onDidChangeFileDecorations.fire(uris ?? undefined);
  }

  dispose(): void {
    this._onDidChangeFileDecorations.dispose();
  }
}
