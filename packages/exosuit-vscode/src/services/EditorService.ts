import * as vscode from "vscode";

export class EditorService {
  // State: The current active editor
  private _active: vscode.TextEditor | undefined;
  private _onDidChangeActiveTextEditor = new vscode.EventEmitter<vscode.TextEditor | undefined>();
  public readonly onDidChangeActiveTextEditor = this._onDidChangeActiveTextEditor.event;

  get active() {
    return this._active;
  }

  #disposables: vscode.Disposable[] = [];

  init() {
    this._active = vscode.window.activeTextEditor;
    // Adapter: Listen to VS Code event and update state
    this.#disposables.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        this._active = editor;
        this._onDidChangeActiveTextEditor.fire(editor);
      })
    );
  }

  dispose() {
    this.#disposables.forEach((d) => d.dispose());
    this.#disposables = [];
    this._onDidChangeActiveTextEditor.dispose();
  }
}

export const editorService = new EditorService();
