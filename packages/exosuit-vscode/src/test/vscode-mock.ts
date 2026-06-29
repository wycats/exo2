// Mock for vscode module
export const EventEmitter = class {
  listeners: any[] = [];
  event = (listener: any) => {
    this.listeners.push(listener);
    return { dispose: () => {} };
  };
  fire = (data: any) => this.listeners.forEach((l) => l(data));
  dispose = () => {};
};

export const Disposable = {
  from: () => ({ dispose: () => {} }),
};

export const Uri = {
  parse: (s: string) => ({ toString: () => s }),
  file: (s: string) => ({ toString: () => `file://${s}` }),
};

export const workspace = {
  createFileSystemWatcher: () => ({
    onDidCreate: () => ({ dispose: () => {} }),
    onDidChange: () => ({ dispose: () => {} }),
    onDidDelete: () => ({ dispose: () => {} }),
    dispose: () => {},
  }),
};
