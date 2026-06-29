import * as vscode from "vscode";

class ReactiveDoc {
  // The raw VS Code document
  private _raw: vscode.TextDocument;
  // Signal for content changes (typing)
  private _version: number;
  
  private _onDidChange = new vscode.EventEmitter<void>();
  public readonly onDidChange = this._onDidChange.event;

  constructor(doc: vscode.TextDocument) {
    this._raw = doc;
    this._version = doc.version;
  }

  // Called by the Service when `onDidChangeTextDocument` fires
  refresh(doc: vscode.TextDocument) {
    this._raw = doc;
    this._version = doc.version;
    this._onDidChange.fire();
  }

  get value() {
    return this._raw;
  }

  get version() {
    return this._version;
  }
}

export class DocumentService {
  // Map of URI -> Reactive Wrapper
  private _store = new Map<string, ReactiveDoc>();
  #disposables: vscode.Disposable[] = [];

  init() {
    // 1. Initial Population
    vscode.workspace.textDocuments.forEach((doc) => this.#add(doc));

    // 2. Lifecycle: Open/Close (Structure)
    this.#disposables.push(
      vscode.workspace.onDidOpenTextDocument((d) => this.#add(d)),
      vscode.workspace.onDidCloseTextDocument((d) =>
        this._store.delete(d.uri.toString())
      )
    );

    // 3. Lifecycle: Change (Content)
    this.#disposables.push(
      vscode.workspace.onDidChangeTextDocument((e) => {
        const wrapper = this._store.get(e.document.uri.toString());
        wrapper?.refresh(e.document);
      })
    );
  }

  constructor() {}

  #add(doc: vscode.TextDocument) {
    this._store.set(doc.uri.toString(), new ReactiveDoc(doc));
  }

  get(uri: string) {
    return this._store.get(uri); // Returns ReactiveDoc
  }

  dispose() {
    this.#disposables.forEach((d) => d.dispose());
    this._store.clear();
  }
}

export const documentService = new DocumentService();
