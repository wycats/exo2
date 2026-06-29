import * as vscode from "vscode";
import { getLogger } from "../logging";

const logger = getLogger("extension");

class ReactiveWatcher {
  private _version = 0;
  private _onDidChange = new vscode.EventEmitter<number>();
  public readonly onDidChange = this._onDidChange.event;

  #watcher: vscode.FileSystemWatcher;
  #debounceTimer: ReturnType<typeof setTimeout> | undefined;
  refCount = 0; // Track usage

  // Event for specific file changes
  private _onDidFileChange = new vscode.EventEmitter<vscode.Uri>();
  public readonly onDidFileChange = this._onDidFileChange.event;

  constructor(pattern: string) {
    this.#watcher = vscode.workspace.createFileSystemWatcher(pattern);

    // Coalescing Logic:
    // Accumulate events, notify once per tick (50ms).
    const notify = (uri: vscode.Uri) => {
      logger.trace(`[FileSystemService] File changed: ${uri.fsPath}`);
      // Fire specific event immediately (for fine-grained invalidation)
      this._onDidFileChange.fire(uri);
      fileSystemService.notifyGlobal(uri);

      if (this.#debounceTimer) {
        clearTimeout(this.#debounceTimer);
      }
      this.#debounceTimer = setTimeout(() => {
        this._version += 1; // Bump!
        this._onDidChange.fire(this._version);
        this.#debounceTimer = undefined;
      }, 50);
    };

    this.#watcher.onDidCreate(notify);
    this.#watcher.onDidChange(notify);
    this.#watcher.onDidDelete(notify);
  }

  get version() {
    return this._version;
  }

  dispose() {
    this.#watcher.dispose();
    if (this.#debounceTimer) {
      clearTimeout(this.#debounceTimer);
    }
    this._onDidChange.dispose();
    this._onDidFileChange.dispose();
  }
}

export class FileSystemService {
  #watchers = new Map<string, ReactiveWatcher>();

  // Global event for any file change (for ReactivityService)
  private _onDidFileChange = new vscode.EventEmitter<vscode.Uri>();
  public readonly onDidFileChange = this._onDidFileChange.event;

  constructor() {}

  watch(pattern: string): {
    version: number;
    onDidChange: vscode.Event<number>;
    onDidFileChange: vscode.Event<vscode.Uri>;
    dispose: () => void;
  } {
    let watcher = this.#watchers.get(pattern);

    if (!watcher) {
      watcher = new ReactiveWatcher(pattern);
      this.#watchers.set(pattern, watcher);
    }

    // Increment Ref Count
    watcher.refCount++;

    return {
      // Expose the signal via a getter so it tracks usage
      get version() {
        return watcher!.version;
      },

      onDidChange: watcher.onDidChange,
      onDidFileChange: watcher.onDidFileChange,

      // Cleanup Hook
      dispose: () => {
        if (!watcher) {
          return;
        }
        watcher.refCount--;
        if (watcher.refCount === 0) {
          watcher.dispose();
          this.#watchers.delete(pattern);
        }
      },
    };
  }

  notifyGlobal(uri: vscode.Uri) {
    this._onDidFileChange.fire(uri);
  }

  dispose() {
    for (const watcher of this.#watchers.values()) {
      watcher.dispose();
    }
    this.#watchers.clear();
    this._onDidFileChange.dispose();
  }
}

export const fileSystemService = new FileSystemService();
