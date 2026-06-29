/**
 * TraceCache: daemon-trace-based reactive cache for extension sidebar data.
 *
 * Holds cached (data, trace) pairs for each registered root. When a write
 * happens (detected via response `effect` or daemon push notification),
 * validates all cached traces against the daemon and re-fetches invalid ones.
 *
 * Replaces the WASM-based ReactivityService/DerivedRootRegistry stack with
 * a simple daemon-validated model per RFC 10165.
 */

import * as vscode from "vscode";
import { getLogger } from "../logging";
import { exoMachineChannel } from "../agent/lmtool/machineChannel";

const logger = getLogger("extension");

/** A registered root that TraceCache manages. */
export interface TraceCacheRoot {
  /** Namespace for the daemon command (e.g., "context"). */
  namespace: string;
  /** Operation for the daemon command (e.g., "snapshot"). */
  operation: string;
  /** Optional input parameters passed to the daemon command. */
  input?: Record<string, unknown>;
}

export type TraceCacheFetchStatus = "success" | "empty" | "error";

export interface TraceCacheRootDiagnostic {
  rootId: string;
  namespace: string;
  operation: string;
  /** Last completed fetch result for this root. */
  status: TraceCacheFetchStatus;
  /** Input sent to the daemon for the fetch, after defaulting to `{}`. */
  input: Record<string, unknown>;
  /** Whether the root had explicit input configured, rather than default `{}`. */
  explicitInput: boolean;
  fetchedAt: number;
  /** Total wall-clock time spent fetching this root through the machine channel. */
  durationMs?: number;
  error?: {
    code?: string;
    message: string;
    details?: unknown;
  };
}

/** A cached entry: data + the trace that produced it. */
interface CacheEntry {
  data: unknown;
  trace: unknown;
}

interface InFlightEntry {
  key: string;
  generation: number;
  token: symbol;
  promise: Promise<CacheEntry | null>;
}

interface PendingFetchState {
  running: boolean;
  dirty: boolean;
}

function cloneInput(input: Record<string, unknown> | undefined): {
  input: Record<string, unknown>;
  explicitInput: boolean;
} {
  return {
    input: input ? { ...input } : {},
    explicitInput: input !== undefined,
  };
}

/** Build a machine channel request envelope. */
function buildEnvelope(
  namespace: string,
  operation: string,
  input: Record<string, unknown> = {},
) {
  const path = namespace.length > 0 ? [namespace, operation] : [operation];
  return {
    protocol_version: 1,
    id: `trace-cache.${namespace}.${operation}`,
    op: {
      kind: "call" as const,
      params: {
        address: { kind: "operation" as const, path },
        input,
      },
    },
  };
}

export class TraceCache implements vscode.Disposable {
  #cache = new Map<string, CacheEntry>();
  #syntheticRoots = new Map<string, CacheEntry>();
  #inFlight = new Map<string, InFlightEntry>();
  #roots = new Map<string, TraceCacheRoot>();
  #diagnostics = new Map<string, TraceCacheRootDiagnostic>();
  #pendingFetches = new Map<string, PendingFetchState>();
  #generation = 0;
  #pendingValidation: ReturnType<typeof setTimeout> | null = null;
  #workspaceRoot: string | null = null;

  readonly #onDidChange = new vscode.EventEmitter<string>();
  /** Fired when a root's cached data has been refreshed after invalidation. */
  readonly onDidChange: vscode.Event<string> = this.#onDidChange.event;

  readonly #onDidWrite = new vscode.EventEmitter<void>();
  /** Fired when any write is detected (for external consumers). */
  readonly onDidWrite: vscode.Event<void> = this.#onDidWrite.event;

  readonly #onDidDiagnosticChange = new vscode.EventEmitter<string>();
  /** Fired when a root records fetch diagnostics. */
  readonly onDidDiagnosticChange: vscode.Event<string> =
    this.#onDidDiagnosticChange.event;

  setWorkspaceRoot(root: string): void {
    this.#workspaceRoot = root;
  }

  registerRoot(rootId: string, root: TraceCacheRoot): void {
    this.#roots.set(rootId, root);
    void this.#scheduleFetch(rootId);
  }

  setSyntheticRoot(rootId: string, data: unknown, trace: unknown = null): void {
    this.#syntheticRoots.set(rootId, { data, trace });
    this.#diagnostics.delete(rootId);
    this.#onDidChange.fire(rootId);
  }

  clearSyntheticRoot(rootId: string): void {
    const hadRoot = this.#syntheticRoots.delete(rootId);
    this.#diagnostics.delete(rootId);
    if (hadRoot) {
      this.#onDidChange.fire(rootId);
    }
  }

  getDiagnostic(rootId: string): TraceCacheRootDiagnostic | undefined {
    return this.#diagnostics.get(rootId);
  }

  async #scheduleFetch(rootId: string): Promise<void> {
    const pending = this.#pendingFetches.get(rootId);
    if (pending?.running) {
      pending.dirty = true;
      return;
    }

    this.#pendingFetches.set(rootId, { running: true, dirty: false });
    await Promise.resolve();

    while (true) {
      const state = this.#pendingFetches.get(rootId);
      if (!state) {
        return;
      }
      state.dirty = false;
      await this.#fetchRoot(rootId);

      if (!state.dirty) {
        this.#pendingFetches.delete(rootId);
        return;
      }
    }
  }

  /** Update a root's input params, invalidate its cache, and re-fetch. */
  updateRootInput(
    rootId: string,
    input: Record<string, unknown> | undefined,
  ): void {
    const root = this.#roots.get(rootId);
    if (!root) {
      return;
    }
    root.input = input;
    this.#cache.delete(rootId);
    this.#inFlight.delete(rootId);
    this.#onDidChange.fire(rootId);
    void this.#scheduleFetch(rootId);
  }

  async get(rootId: string): Promise<{ data: unknown; trace: unknown } | null> {
    const synthetic = this.#syntheticRoots.get(rootId);
    if (synthetic) {
      return synthetic;
    }

    const cached = this.#cache.get(rootId);
    if (cached) {
      return cached;
    }
    const key = this.#rootFetchKey(rootId);
    const inFlight = this.#inFlight.get(rootId);
    if (inFlight?.key === key && inFlight.generation === this.#generation) {
      return inFlight.promise;
    }
    return this.#fetchRoot(rootId);
  }

  notifyWrite(): void {
    this.#onDidWrite.fire();
    this.revalidateAll();
  }

  revalidateAll(): void {
    if (this.#pendingValidation !== null) {
      clearTimeout(this.#pendingValidation);
    }
    this.#pendingValidation = setTimeout(() => {
      this.#pendingValidation = null;
      void this.#validateAll();
    }, 0);
  }

  async #validateAll(): Promise<void> {
    if (!this.#workspaceRoot) {
      return;
    }

    const myGeneration = ++this.#generation;

    for (const [rootId, entry] of this.#cache) {
      if (this.#generation !== myGeneration) {
        return;
      }

      try {
        const traceJson = JSON.stringify(entry.trace);
        const response = await exoMachineChannel(
          this.#workspaceRoot,
          buildEnvelope("context", "validate-trace", { trace_json: traceJson }),
        );

        if (this.#generation !== myGeneration) {
          return;
        }

        const valid =
          response.status === "ok" &&
          (response.result as { valid?: boolean })?.valid === true;

        if (!valid) {
          await this.#fetchRoot(rootId);
          if (this.#generation !== myGeneration) {
            return;
          }
        }
      } catch (err) {
        logger.warn(
          `[trace-cache] Failed to validate trace for ${rootId}:`,
          err,
        );
        try {
          await this.#fetchRoot(rootId);
        } catch {
          // Ignore fetch errors during validation
        }
      }
    }

    // Fetch any registered roots that aren't in the cache yet.
    // This covers the cold-start case: the daemon reconnects but the
    // cache is empty because the initial fetch failed. Without this,
    // notifyWrite() is a no-op and the sidebar stays blank.
    for (const rootId of this.#roots.keys()) {
      if (this.#generation !== myGeneration) {
        return;
      }
      if (!this.#cache.has(rootId)) {
        try {
          await this.#fetchRoot(rootId);
        } catch {
          // Ignore — will be retried on next notifyWrite
        }
      }
    }
  }

  async #fetchRoot(
    rootId: string,
  ): Promise<{ data: unknown; trace: unknown } | null> {
    const root = this.#roots.get(rootId);
    if (!root) {
      logger.warn(`[trace-cache] No root registered for ${rootId}`);
      return null;
    }

    const { input, explicitInput } = cloneInput(root.input);

    if (!this.#workspaceRoot) {
      this.#recordFetchDiagnostic({
        rootId,
        namespace: root.namespace,
        operation: root.operation,
        status: "error",
        input,
        explicitInput,
        fetchedAt: Date.now(),
        error: { message: "No workspace root selected" },
      });
      return null;
    }

    const key = this.#rootFetchKey(rootId);
    const inFlight = this.#inFlight.get(rootId);
    if (inFlight?.key === key && inFlight.generation === this.#generation) {
      return inFlight.promise;
    }

    const generation = this.#generation;
    const token = Symbol(rootId);
    const promise = this.#fetchRootUncached(
      rootId,
      root,
      key,
      generation,
      token,
      input,
      explicitInput,
    );
    this.#inFlight.set(rootId, { key, generation, token, promise });
    return promise;
  }

  async #fetchRootUncached(
    rootId: string,
    root: TraceCacheRoot,
    key: string,
    generation: number,
    token: symbol,
    input: Record<string, unknown>,
    explicitInput: boolean,
  ): Promise<CacheEntry | null> {
    const workspaceRoot = this.#workspaceRoot;
    if (!workspaceRoot) {
      return null;
    }
    const startedAt = Date.now();
    const baseDiagnostic = () => ({
      rootId,
      namespace: root.namespace,
      operation: root.operation,
      input,
      explicitInput,
      fetchedAt: Date.now(),
      durationMs: Date.now() - startedAt,
    });
    try {
      const response = await exoMachineChannel(
        workspaceRoot,
        buildEnvelope(root.namespace, root.operation, input),
      );

      if (
        this.#generation !== generation ||
        this.#rootFetchKey(rootId) !== key
      ) {
        return null;
      }

      if (
        response.status === "ok" &&
        response.result !== null &&
        response.result !== undefined
      ) {
        const entry: CacheEntry = {
          data: response.result,
          trace: response.trace,
        };
        this.#cache.set(rootId, entry);
        this.#recordFetchDiagnostic({
          ...baseDiagnostic(),
          status: "success",
        });
        this.#onDidChange.fire(rootId);
        return entry;
      }

      if (response.status === "ok") {
        const hadCache = this.#cache.delete(rootId);
        this.#recordFetchDiagnostic({
          ...baseDiagnostic(),
          status: "empty",
        });
        if (hadCache) {
          this.#onDidChange.fire(rootId);
        }
        return null;
      }

      const hadCache = this.#cache.delete(rootId);
      this.#recordFetchDiagnostic({
        ...baseDiagnostic(),
        status: "error",
        error: {
          code: response.error?.code,
          message:
            response.error?.message ?? `Daemon returned ${response.status}`,
          details: response.error?.details,
        },
      });
      if (hadCache) {
        this.#onDidChange.fire(rootId);
      }

      return null;
    } catch (err) {
      const hadCache = this.#cache.delete(rootId);
      this.#recordFetchDiagnostic({
        ...baseDiagnostic(),
        status: "error",
        error: {
          message: err instanceof Error ? err.message : String(err),
        },
      });
      if (hadCache) {
        this.#onDidChange.fire(rootId);
      }
      logger.warn(`[trace-cache] Failed to fetch root ${rootId}:`, err);
      return null;
    } finally {
      const current = this.#inFlight.get(rootId);
      if (
        current?.key === key &&
        current.generation === generation &&
        current.token === token
      ) {
        this.#inFlight.delete(rootId);
      }
    }
  }

  #recordFetchDiagnostic(diagnostic: TraceCacheRootDiagnostic): void {
    this.#diagnostics.set(diagnostic.rootId, diagnostic);
    this.#onDidDiagnosticChange.fire(diagnostic.rootId);

    if (
      diagnostic.durationMs !== undefined &&
      (diagnostic.status === "error" || diagnostic.durationMs >= 2_000)
    ) {
      const duration = `${diagnostic.durationMs}ms`;
      logger.warn(
        `[trace-cache] Fetch root ${diagnostic.rootId} (${diagnostic.namespace}.${diagnostic.operation}) ${diagnostic.status} in ${duration}`,
      );
    }
  }

  #rootFetchKey(rootId: string): string {
    const root = this.#roots.get(rootId);
    return JSON.stringify({
      workspaceRoot: this.#workspaceRoot,
      rootId,
      namespace: root?.namespace,
      operation: root?.operation,
      input: root?.input ?? {},
    });
  }

  dispose(): void {
    if (this.#pendingValidation !== null) {
      clearTimeout(this.#pendingValidation);
    }
    this.#onDidChange.dispose();
    this.#onDidWrite.dispose();
    this.#onDidDiagnosticChange.dispose();
    this.#cache.clear();
    this.#inFlight.clear();
    this.#diagnostics.clear();
    this.#pendingFetches.clear();
    this.#roots.clear();
  }
}

/** Singleton instance for the workspace. */
let instance: TraceCache | null = null;

export function getTraceCache(): TraceCache {
  if (!instance) {
    instance = new TraceCache();
  }
  return instance;
}

export function disposeTraceCache(): void {
  if (instance) {
    instance.dispose();
    instance = null;
  }
}
