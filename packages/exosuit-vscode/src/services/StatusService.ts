import * as vscode from "vscode";
import { exec } from "child_process";
import { promisify } from "util";
import type { ExoStatusResponse, ProgressMode } from "../types/progress";
import { getTraceCache } from "./TraceCache";
import { getLogger } from "../logging";
import { exoCommand } from "../exoBin";
import { selectCurrentWorkspaceRoot } from "../workspaceRoot";

const execAsync = promisify(exec);
const logger = getLogger("extension");

/** Cache TTL in milliseconds (5 seconds per RFC 00184) */
const CACHE_TTL_MS = 5_000;

/**
 * Service for fetching and caching CLI status.
 *
 * - Caches result for 5 seconds
 * - Invalidates on plan state changes
 * - Singleton pattern
 *
 * @see RFC 00184: Mode-Aware Sidebar Cockpit
 */
export class StatusService implements vscode.Disposable {
  private _cache: ExoStatusResponse | null = null;
  private _cacheTimestamp: number = 0;
  private _fetchPromise: Promise<ExoStatusResponse> | null = null;
  private _disposables: vscode.Disposable[] = [];

  // Event: When status changes (for UI refresh)
  private _onDidStatusChange = new vscode.EventEmitter<ExoStatusResponse>();
  public readonly onDidStatusChange = this._onDidStatusChange.event;

  constructor() {
    // Set up reactivity-based invalidation
    this._setupReactivityInvalidation();
  }

  private _setupReactivityInvalidation(): void {
    this._disposables.push(
      getTraceCache().onDidWrite(() => {
        logger.trace("[StatusService] write detected, invalidating cache");
        this.invalidate();
      }),
    );
  }

  /**
   * Get the current progress mode.
   * Uses cached status if available and fresh.
   */
  async getProgressMode(): Promise<ProgressMode> {
    const status = await this.getStatus();
    return status.progress_mode;
  }

  /**
   * Get the full status response.
   * Uses cached status if available and fresh (< 5 seconds old).
   */
  async getStatus(): Promise<ExoStatusResponse> {
    const now = Date.now();

    // Return cached value if fresh
    if (this._cache && now - this._cacheTimestamp < CACHE_TTL_MS) {
      return this._cache;
    }

    // If already fetching, wait for that promise
    if (this._fetchPromise) {
      return this._fetchPromise;
    }

    // Fetch fresh status
    this._fetchPromise = this._fetchStatus();

    try {
      const status = await this._fetchPromise;
      this._cache = status;
      this._cacheTimestamp = Date.now();
      return status;
    } finally {
      this._fetchPromise = null;
    }
  }

  /**
   * Invalidate the cache, forcing next getStatus() to fetch fresh data.
   * Also fires the onDidStatusChange event after fetching new status.
   */
  invalidate(): void {
    this._cache = null;
    this._cacheTimestamp = 0;
    this._fetchPromise = null; // Cancel any in-flight fetch to ensure fresh data

    // Fire event asynchronously after fetching new status
    this.getStatus()
      .then((status) => {
        this._onDidStatusChange.fire(status);
      })
      .catch((err) => {
        logger.error(
          "[StatusService] Failed to fetch status after invalidation:",
          err,
        );
      });
  }

  private async _fetchStatus(): Promise<ExoStatusResponse> {
    const workspaceSelection = selectCurrentWorkspaceRoot();
    const rootPath = workspaceSelection.rootPath;
    if (!rootPath) {
      throw new Error(
        `No usable Exosuit workspace root: ${workspaceSelection.reason}`,
      );
    }

    try {
      const { stdout } = await execAsync(
        exoCommand("status --format json", rootPath),
        {
          cwd: rootPath,
          timeout: 10_000, // 10 second timeout
        },
      );

      const wrapper = JSON.parse(stdout) as {
        id: string;
        protocol_version: number;
        result?: ExoStatusResponse;
        status?: string;
        error?: { code: string; message: string };
      };

      // Handle CLI error responses (status: "error", result: null)
      if (wrapper.status === "error" || !wrapper.result) {
        const errorMsg = wrapper.error?.message ?? "Unknown CLI error";
        logger.warn("[StatusService] CLI returned error:", errorMsg);
        throw new Error(errorMsg);
      }

      const parsed = wrapper.result;
      logger.trace("[StatusService] Fetched status:", parsed.progress_mode);
      return parsed;
    } catch (error) {
      logger.error("[StatusService] Failed to fetch status:", error);

      // Return a fallback status on error
      return {
        git_dirty: false,
        progress_mode: "between-phases",
        steering: {
          primary_intent: "",
          progress_mode: "between-phases",
          situation: "",
          perception_summaries: [],
        },
        pending_goals: 0,
        completed_goals: 0,
      };
    }
  }

  dispose(): void {
    this._disposables.forEach((d) => d.dispose());
    this._onDidStatusChange.dispose();
  }
}

/** Singleton instance */
export const statusService = new StatusService();
