import * as vscode from "vscode";

export class ViewRenderer {
  constructor(private readonly _extensionUri: vscode.Uri) {}

  public render(
    webview: vscode.Webview,
    content: string,
    _options: {
      isDebug?: boolean;
    } = {}
  ): string {
    const nonce = this.getNonce();

    const scriptUri = webview.asWebviewUri(
      vscode.Uri.joinPath(this._extensionUri, "media", "main.js")
    );

    return `<!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="UTF-8">
                <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${webview.cspSource} 'unsafe-inline'; font-src ${webview.cspSource}; img-src ${webview.cspSource} https:; script-src ${webview.cspSource} 'nonce-${nonce}';">
                <meta name="viewport" content="width=device-width, initial-scale=1.0">
                <title>Exosuit Context</title>
                <style>
                    body { font-family: var(--vscode-font-family); padding: 10px; display: flex; flex-direction: column; height: 100vh; box-sizing: border-box; }
                    h1, h2, h3 { color: var(--vscode-editor-foreground); }
                    a { color: var(--vscode-textLink-foreground); }
                    code { font-family: var(--vscode-editor-font-family); background-color: var(--vscode-textBlockQuote-background); padding: 2px 4px; border-radius: 3px; }
                    pre { background-color: var(--vscode-textBlockQuote-background); padding: 10px; border-radius: 5px; overflow-x: auto; }
                    pre code { background-color: transparent; padding: 0; }
                    ul { padding-left: 20px; }
                    li { margin-bottom: 5px; }
                    
                    #content { flex: 1; overflow-y: auto; }
                    
                    /* Debug Logs Styling */
                    #logs { 
                        font-family: var(--vscode-editor-font-family);
                        font-size: 0.8em;
                        white-space: pre-wrap;
                    }
                    .log-entry { 
                        margin-bottom: 2px; 
                        border-bottom: 1px solid var(--vscode-widget-border); 
                        padding: 2px 0;
                    }
                </style>
            </head>
            <body>
                <div id="content">
                    ${content}
                </div>
                <script nonce="${nonce}" src="${scriptUri}"></script>
            </body>
            </html>`;
  }

  private getNonce() {
    let text = "";
    const possible =
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    for (let i = 0; i < 32; i++) {
      text += possible.charAt(Math.floor(Math.random() * possible.length));
    }
    return text;
  }
}
