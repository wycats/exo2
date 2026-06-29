import * as vscode from "vscode";
import { LogService } from "./LogService";
import { ViewRenderer } from "./ViewRenderer";

export class DebugLogProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = "exosuit.debugLogs";
  private _view?: vscode.WebviewView;
  private _logger: LogService;
  private _renderer: ViewRenderer;

  constructor(private readonly _extensionUri: vscode.Uri) {
    this._logger = LogService.instance;
    this._renderer = new ViewRenderer(_extensionUri);

    this._logger.onLog(() => {
      this.updateDebugView();
    });
  }

  private updateDebugView() {
    if (this._view) {
      const logsHtml = this._logger.getLogsHtml();
      this._view.webview.postMessage({ type: "updateLogs", logs: logsHtml });
    }
  }

  public async resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
  ) {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [this._extensionUri],
    };

    this.refresh();
  }

  private refresh() {
    if (!this._view) {
      return;
    }

    // We need to inject the codicon CSS
    const codiconUri = this._view.webview.asWebviewUri(
      vscode.Uri.joinPath(this._extensionUri, "media", "codicon.css")
    );

    const style = `
      <link href="${codiconUri}" rel="stylesheet" />
      <style>
        .log-entry {
          padding: 4px 0;
          border-bottom: 1px solid var(--vscode-tree-tableOddRowsBackground);
          font-family: var(--vscode-font-family);
          font-size: 13px;
        }
        .timestamp {
          color: var(--vscode-descriptionForeground);
          font-size: 0.9em;
          margin-right: 8px;
        }
        .icon {
          margin-right: 6px;
          vertical-align: text-bottom;
        }
        .label {
          color: var(--vscode-foreground);
        }
        .details {
          margin-left: 24px;
          color: var(--vscode-textPreformat-foreground);
          font-size: 0.9em;
          opacity: 0.8;
        }
        .type-axiom .label { color: var(--vscode-charts-blue); }
        .type-context .label { color: var(--vscode-charts-green); }
        .type-llm .label { color: var(--vscode-charts-purple); }
      </style>
    `;

    const logsHtml = `${style}<div id="logs">${this._logger.getLogsHtml()}</div>`;
    this._view.webview.html = this._renderer.render(
      this._view.webview,
      logsHtml,
      { isDebug: true }
    );
  }
}
