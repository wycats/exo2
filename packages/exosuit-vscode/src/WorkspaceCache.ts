import * as vscode from "vscode";
import * as path from "path";
import * as fs from "node:fs";
import * as os from "node:os";
import { getLogger } from "./logging";

const logger = getLogger("extension");

export interface IWorkspaceCache {
  hasFile(path: string): boolean;
  hasDirectory(path: string): boolean;
  dispose(): void;
  readonly onDidUpdate: vscode.Event<void>;
}

/**
 * Implements the Workspace Cache as defined in VCOM 5.4.2.
 * Maintains a low-latency, synchronous cache of the workspace file system
 * to support the Reference Resolver.
 */
export class WorkspaceCache implements IWorkspaceCache {
  private fileSet = new Set<string>();
  private dirSet = new Set<string>();
  private initialized = false;
  private _onDidInitialize = new vscode.EventEmitter<void>();
  public readonly onDidInitialize = this._onDidInitialize.event;
  private _onDidUpdate = new vscode.EventEmitter<void>();
  public readonly onDidUpdate = this._onDidUpdate.event;
  private disposables: vscode.Disposable[] = [];
  private updateTimer: NodeJS.Timeout | undefined;

  constructor() {
    this.initialize();
    this.setupWatchers();
  }

  dispose() {
    this.disposables.forEach((d) => d.dispose());
    this.disposables = [];
    if (this.updateTimer) {
      clearTimeout(this.updateTimer);
    }
  }

  private async initialize() {
    const traceEnabled = process.env.EXOSUIT_WORKSPACE_CACHE_TRACE === "true";

    const tracePath = path.join(
      os.tmpdir(),
      "exosuit-workspace-cache-trace.log",
    );
    const trace = (msg: string) => {
      if (!traceEnabled) {
        return;
      }

      const line = `${new Date().toISOString()} [WorkspaceCache] ${msg}\n`;

      try {
        fs.appendFileSync(tracePath, line);
        return;
      } catch (err) {
        try {
          // Last resort: emit to stderr so it ends up in VS Code logs when possible.
          logger.error(
            `[WorkspaceCache] failed to write trace to ${tracePath}`,
            err,
          );
        } catch {
          // ignore
        }
      }
    };

    const t0 = Date.now();
    trace("initialize(): start");

    // Exclude node_modules and .git for performance, though VCOM doesn't strictly specify exclusions,
    // it's practical for a "Project Context" cache.
    const tFindStart = Date.now();
    const files = await vscode.workspace.findFiles(
      "**/*",
      "{**/node_modules/**,**/.git/**,**/.vscode-test/**,**/out/**,**/dist/**}",
    );
    trace(
      `initialize(): findFiles returned ${files.length} in ${
        Date.now() - tFindStart
      }ms`,
    );

    const tLoopStart = Date.now();
    for (const file of files) {
      this.addPath(file, true); // Suppress event during init
    }
    trace(
      `initialize(): addPath loop completed in ${
        Date.now() - tLoopStart
      }ms (fileSet=${this.fileSet.size} dirSet=${this.dirSet.size})`,
    );

    this.initialized = true;
    this._onDidInitialize.fire();

    trace(`initialize(): done in ${Date.now() - t0}ms`);
  }

  private setupWatchers() {
    // Ignore change events (true) to reduce overhead, as we only care about existence.
    // Keep create (false) and delete (false) events.
    const watcher = vscode.workspace.createFileSystemWatcher(
      "**/*",
      false,
      true,
      false,
    );
    this.disposables.push(watcher);

    this.disposables.push(watcher.onDidCreate((uri) => this.addPath(uri)));
    this.disposables.push(watcher.onDidDelete((uri) => this.removePath(uri)));
  }

  private isIgnored(relative: string): boolean {
    return (
      relative.startsWith(".git/") ||
      relative.includes("/.git/") ||
      relative.startsWith("node_modules/") ||
      relative.includes("/node_modules/") ||
      relative.startsWith(".vscode-test/") ||
      relative.includes("/.vscode-test/") ||
      relative.startsWith("out/") ||
      relative.includes("/out/") ||
      relative.startsWith("dist/") ||
      relative.includes("/dist/")
    );
  }

  private normalize(p: string): string {
    // VCOM 5.4.3: Separator Check.
    return p.replace(/\\/g, "/");
  }

  private addPath(uri: vscode.Uri, suppressEvent = false) {
    // Optimization: Fast check on fsPath to avoid expensive relative path calculation
    const fsPath = uri.fsPath;
    if (
      fsPath.includes(`${path.sep}node_modules${path.sep}`) ||
      fsPath.includes(`${path.sep}.git${path.sep}`) ||
      fsPath.includes(`${path.sep}out${path.sep}`) ||
      fsPath.includes(`${path.sep}dist${path.sep}`)
    ) {
      return;
    }

    const relative = vscode.workspace.asRelativePath(uri, false);

    // asRelativePath returns the path itself if it's not in the workspace.
    // We only care about workspace files.
    if (relative === uri.fsPath) {
      return;
    }

    if (this.isIgnored(relative)) {
      return;
    }

    const normalized = this.normalize(relative);
    this.fileSet.add(normalized);

    // Add all parent directories
    let current = path.dirname(normalized);
    while (current !== "." && current !== "/") {
      this.dirSet.add(current);
      current = path.dirname(current);
    }

    if (!suppressEvent) {
      this.scheduleUpdate();
    }
  }

  private removePath(uri: vscode.Uri) {
    // Optimization: Fast check
    const fsPath = uri.fsPath;
    if (
      fsPath.includes(`${path.sep}node_modules${path.sep}`) ||
      fsPath.includes(`${path.sep}.git${path.sep}`) ||
      fsPath.includes(`${path.sep}out${path.sep}`) ||
      fsPath.includes(`${path.sep}dist${path.sep}`)
    ) {
      return;
    }

    const relative = vscode.workspace.asRelativePath(uri, false);

    if (relative === uri.fsPath) {
      return;
    }

    if (this.isIgnored(relative)) {
      return;
    }

    const normalized = this.normalize(relative);
    this.fileSet.delete(normalized);

    // If it's a directory, remove all children
    if (this.dirSet.has(normalized)) {
      const prefix = normalized + "/";
      for (const file of this.fileSet) {
        if (file.startsWith(prefix)) {
          this.fileSet.delete(file);
        }
      }
      this.dirSet.delete(normalized);
    }

    this.scheduleUpdate();
  }

  private scheduleUpdate() {
    if (this.updateTimer) {
      clearTimeout(this.updateTimer);
    }
    this.updateTimer = setTimeout(() => {
      this._onDidUpdate.fire();
      this.updateTimer = undefined;
    }, 200); // Debounce 200ms
  }

  /**
   * Synchronous O(1) check for file existence.
   * @param pathStr Workspace-relative path (e.g., "src/utils.ts")
   */
  public hasFile(pathStr: string): boolean {
    const normalized = this.normalize(pathStr);
    return this.fileSet.has(normalized);
  }

  /**
   * Synchronous O(1) check for directory existence.
   * @param pathStr Workspace-relative path (e.g., "src")
   */
  public hasDirectory(pathStr: string): boolean {
    return this.dirSet.has(this.normalize(pathStr));
  }

  public isInitialized(): boolean {
    return this.initialized;
  }
}
