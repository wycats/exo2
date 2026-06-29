/**
 * TracedProvider: generic reactive bridge from TraceCache to VS Code tree views.
 *
 * Subscribes to one or more TraceCache roots. When any root's data changes,
 * fires onDidChangeTreeData so VS Code re-calls getChildren, which builds a
 * fresh data snapshot from TraceCache and passes it to the renderer.
 */

import * as vscode from "vscode";
import { getTraceCache, type TraceCacheRootDiagnostic } from "./TraceCache";
import { getLogger } from "../logging";

const logger = getLogger("extension");

export type TreeRenderer<T extends vscode.TreeItem> = (
  roots: ReadonlyMap<string, unknown>,
  diagnostics: ReadonlyMap<string, TraceCacheRootDiagnostic | undefined>,
) => T[] | Promise<T[]>;

export interface TracedProvider<T extends vscode.TreeItem>
  extends vscode.TreeDataProvider<T>, vscode.Disposable {
  readonly onDidChangeTreeData: vscode.Event<void>;
  getChildren: (element?: T) => Promise<T[]>;
  refresh: () => void;
}

const RETRY_BACKOFF_MS = [2000, 5000, 10000];

export function createTracedProvider<T extends vscode.TreeItem>(
  rootIds: string[],
  render: TreeRenderer<T>,
): TracedProvider<T> {
  const emitter = new vscode.EventEmitter<void>();
  const traceCache = getTraceCache();
  const subscription = traceCache.onDidChange((rootId) => {
    if (rootIds.includes(rootId)) {
      emitter.fire();
    }
  });
  const diagnosticSubscription = traceCache.onDidDiagnosticChange((rootId) => {
    if (rootIds.includes(rootId)) {
      emitter.fire();
    }
  });

  let retryCount = 0;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;

  function scheduleRetry(): void {
    if (retryTimer !== null) {
      return; // already scheduled
    }
    const delay =
      RETRY_BACKOFF_MS[Math.min(retryCount, RETRY_BACKOFF_MS.length - 1)];
    retryCount++;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      emitter.fire();
    }, delay);
  }

  function clearRetry(): void {
    retryCount = 0;
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
  }

  return {
    onDidChangeTreeData: emitter.event,
    getTreeItem: (element: T): vscode.TreeItem => element,
    refresh: () => emitter.fire(),

    dispose() {
      clearRetry();
      subscription.dispose();
      diagnosticSubscription.dispose();
      emitter.dispose();
    },

    getChildren: async (element?: T): Promise<T[]> => {
      if (element) {
        return (element as T & { children?: T[] }).children ?? [];
      }

      const roots = new Map<string, unknown>();
      const diagnostics = new Map<
        string,
        TraceCacheRootDiagnostic | undefined
      >();
      let anyNull = false;

      for (const rootId of rootIds) {
        try {
          const entry = await traceCache.get(rootId);
          roots.set(rootId, entry?.data ?? null);
          diagnostics.set(rootId, traceCache.getDiagnostic(rootId));
          if (!entry?.data) {
            anyNull = true;
          }
        } catch (err) {
          logger.warn(`[TracedProvider] error fetching root ${rootId}:`, err);
          roots.set(rootId, null);
          diagnostics.set(rootId, traceCache.getDiagnostic(rootId));
          anyNull = true;
        }
      }

      // If any root failed or returned null, schedule a retry.
      // On success, reset the retry state.
      if (anyNull) {
        scheduleRetry();
      } else {
        clearRetry();
      }

      try {
        return await render(roots, diagnostics);
      } catch (err) {
        logger.warn("[TracedProvider] Renderer error:", err);
        return [];
      }
    },
  };
}
