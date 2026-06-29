/**
 * Stub module for "vscode" to allow vitest to resolve the import.
 * Actual mocking is done via vi.mock() in test files.
 *
 * This stub provides minimal implementations that may be overridden
 * by individual tests using vi.mock("vscode", ...).
 */

export class EventEmitter<T> {
  private listeners: ((e: T) => void)[] = [];
  event = (listener: (e: T) => void) => {
    this.listeners.push(listener);
    return { dispose: () => {} };
  };
  fire = (data: T) => this.listeners.forEach((l) => l(data));
  dispose = () => {};
}

export class Disposable {
  static from(..._disposables: { dispose: () => void }[]) {
    return { dispose: () => {} };
  }
}

export enum TreeItemCollapsibleState {
  None = 0,
  Collapsed = 1,
  Expanded = 2,
}

export enum TreeItemCheckboxState {
  Unchecked = 0,
  Checked = 1,
}

export class ThemeColor {
  constructor(public readonly id: string) {}
}

export class ThemeIcon {
  constructor(
    public readonly id: string,
    public readonly color?: ThemeColor,
  ) {}
}

export class TreeItem {
  id?: string;
  tooltip?: string;
  description?: string | boolean;
  iconPath?: ThemeIcon | { light: UriLike; dark: UriLike } | UriLike;
  resourceUri?: UriLike;
  command?: unknown;
  contextValue?: string;
  checkboxState?: TreeItemCheckboxState;

  constructor(
    public label: string,
    public collapsibleState?: TreeItemCollapsibleState,
  ) {}
}

type UriLike = { toString: () => string; fsPath: string };

export class LanguageModelTextPart {
  constructor(public readonly value: string) {}
}

export class LanguageModelDataPart {
  static json(value: unknown) {
    return new LanguageModelDataPart(value);
  }

  constructor(public readonly value: unknown) {}
}

export class LanguageModelToolResult {
  constructor(public readonly content: unknown[]) {}
}

export const ExtensionMode = {
  Production: 1,
  Development: 2,
  Test: 3,
  1: "Production",
  2: "Development",
  3: "Test",
} as const;

export const ExtensionKind = {
  UI: 1,
  Workspace: 2,
  1: "UI",
  2: "Workspace",
} as const;

export const Uri = {
  parse: (s: string) => ({ toString: () => s, fsPath: s }),
  file: (path: string) => ({ toString: () => `file://${path}`, fsPath: path }),
};

export const workspace = {
  workspaceFolders: [],
  getConfiguration: () => ({
    get: () => undefined,
    update: async () => {},
  }),
};

export const window = {
  showInformationMessage: async () => undefined,
  showErrorMessage: async () => undefined,
  showWarningMessage: async () => undefined,
};
