/**
 * DaemonChannelServer - Daemon-backed Machine Channel (RFC 0097)
 *
 * This class manages communication with the exo daemon via the platform endpoint.
 * It replaces the stdio-based MachineChannelServer with an endpoint-based
 * implementation that connects through Rust-owned daemon lifecycle.
 *
 * Key features:
 * - Singleton pattern per workspace (avoids multiple connections)
 * - Rust lifecycle authority: opens the endpoint through `daemon ensure`
 * - Request correlation by ID with timeout handling
 * - Graceful reconnection on socket close
 */

import { randomUUID } from "node:crypto";
import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../types/machineChannel";
import { getLogger } from "../logging";

import {
  DEFAULT_DAEMON_REQUEST_TIMEOUT_MS,
  connectToEnsuredDaemon,
  ensureDaemon,
  ensureDaemonLifecycle,
  restartDaemonLifecycle,
  DaemonConnection,
  type DaemonEnsureResult,
} from "./socket-client";
import { getTraceCache } from "../services/TraceCache";
import { getOperation, getRootOperation } from "../lmtool/command-spec.types";

const logger = getLogger("lmtool");

function isPureReadRequest(envelope: MachineChannelRequestEnvelope): boolean {
  if (envelope.op.kind !== "call") {
    return true;
  }

  const params = envelope.op.params;
  if (!("input" in params)) {
    return true;
  }

  const address = params.address;
  if (address.kind !== "operation") {
    return true;
  }

  const [namespaceOrOperation] = address.path;
  const operation =
    address.path.length > 1 ? address.path.slice(1).join(".") : undefined;

  if (operation === undefined) {
    return getRootOperation(namespaceOrOperation)?.effect === "pure";
  }

  if (
    namespaceOrOperation === "sidecar" &&
    operation === "repo" &&
    isRecord(params.input) &&
    params.input.action === "status"
  ) {
    return true;
  }

  return getOperation(namespaceOrOperation, operation)?.effect === "pure";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isReconnectableConnectionLoss(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false;
  }

  const code = (error as { code?: unknown }).code;
  if (code === "EPIPE" || code === "ERR_STREAM_DESTROYED") {
    return true;
  }

  return (
    error.message.includes("Connection closed") ||
    error.message.includes("timed out") ||
    error.message.includes("stream was destroyed") ||
    error.message.includes("ECANCELED") ||
    error.message.includes("EPIPE") ||
    error.message.includes("ERR_STREAM_DESTROYED")
  );
}

export interface ConnectionLike {
  request(
    envelope: MachineChannelRequestEnvelope,
    timeoutMs?: number,
  ): Promise<MachineChannelResponseEnvelope>;
  notify(data: Record<string, unknown>): void;
  close(): void;
  isClosed(): boolean;
  onNotification:
    | ((notification: MachineChannelResponseEnvelope) => void)
    | null;
  onClose: (() => void) | null;
}

export interface DaemonChannelServerDeps {
  connect(
    root: string,
    lifecycle?: DaemonEnsureResult,
  ): Promise<ConnectionLike>;
  ensureLifecycle?(root: string): Promise<DaemonEnsureResult>;
  restartLifecycle?(root: string): Promise<DaemonEnsureResult>;
  traceCache: {
    notifyWrite(): void;
  };
  config?: Partial<DaemonChannelServerConfig>;
  clock?: DaemonChannelClock;
}

type ResolvedDaemonChannelServerDeps = DaemonChannelServerDeps & {
  ensureLifecycle(root: string): Promise<DaemonEnsureResult>;
  restartLifecycle(root: string): Promise<DaemonEnsureResult>;
};

export interface DaemonRestartOptions {
  restartDaemon?: boolean;
}

type TimerHandle = ReturnType<typeof setTimeout>;

interface DaemonChannelClock {
  now(): number;
  setTimeout(callback: () => void, ms: number): TimerHandle;
  clearTimeout(handle: TimerHandle): void;
}

interface DaemonChannelServerConfig {
  /** Timeout for individual requests (ms) */
  requestTimeoutMs: number;
  /** Number of daemon socket lanes available for concurrent pure reads. */
  readLaneCount: number;
  /** Maximum reconnection attempts before entering cooldown */
  maxReconnectAttempts: number;
  /** Delay between reconnection attempts (ms) */
  reconnectDelayMs: number;
  /** Cooldown after repeated failures before requests may try again (ms) */
  reconnectCooldownMs: number;
  /** Delay before background reconnect after socket close (ms) */
  autoReconnectDelayMs: number;
  /** Grace period for pending requests during shutdown (ms) */
  shutdownGraceMs: number;
}

export type ServerModeAvailability =
  | {
      available: true;
      reason: "available";
      workspaceRoot: string;
      reconnectAttempts: number;
      maxReconnectAttempts: number;
    }
  | {
      available: false;
      reason: "env-disabled";
      workspaceRoot: string;
      envVar: "EXOSUIT_USE_SERVER_MODE";
      value: string;
    }
  | {
      available: false;
      reason: "cooldown";
      workspaceRoot: string;
      reconnectAttempts: number;
      maxReconnectAttempts: number;
      cooldownUntilMs: number;
      retryAfterMs: number;
    };

function createDefaultDeps(): ResolvedDaemonChannelServerDeps {
  return {
    async connect(
      root: string,
      lifecycle?: DaemonEnsureResult,
    ): Promise<ConnectionLike> {
      const socket = lifecycle
        ? await connectToEnsuredDaemon(root, lifecycle)
        : await ensureDaemon(root);
      return new DaemonConnection(socket);
    },
    ensureLifecycle: ensureDaemonLifecycle,
    restartLifecycle: restartDaemonLifecycle,
    traceCache: getTraceCache(),
  };
}

/**
 * Configuration for the server
 */
const CONFIG = {
  /** Timeout for individual requests (ms) */
  requestTimeoutMs: DEFAULT_DAEMON_REQUEST_TIMEOUT_MS,
  /** Number of daemon socket lanes available for concurrent pure reads. */
  readLaneCount: 4,
  /** Maximum reconnection attempts before entering cooldown */
  maxReconnectAttempts: 10,
  /** Delay between reconnection attempts (ms) */
  reconnectDelayMs: 500,
  /** Cooldown after repeated failures before requests may try again (ms) */
  reconnectCooldownMs: 30_000,
  /** Delay before background reconnect after socket close (ms) */
  autoReconnectDelayMs: 1_000,
  /** Grace period for pending requests during shutdown (ms) */
  shutdownGraceMs: 2_000,
} as const;

const DEFAULT_CLOCK: DaemonChannelClock = {
  now: () => Date.now(),
  setTimeout: (callback, ms) => setTimeout(callback, ms),
  clearTimeout: (handle) => clearTimeout(handle),
};

export class DaemonChannelServer {
  /** Singleton instances per workspace root */
  private static instances = new Map<string, DaemonChannelServer>();

  /**
   * Get the singleton instance for a workspace root.
   * Creates a new instance if one doesn't exist.
   */
  static getInstance(workspaceRoot: string): DaemonChannelServer {
    let instance = this.instances.get(workspaceRoot);
    if (!instance) {
      instance = new DaemonChannelServer(workspaceRoot);
      this.instances.set(workspaceRoot, instance);
    }
    return instance;
  }

  static createForTesting(
    workspaceRoot: string,
    deps: DaemonChannelServerDeps,
  ): DaemonChannelServer {
    return new DaemonChannelServer(workspaceRoot, deps);
  }

  /**
   * Dispose all server instances. Called on extension deactivate.
   */
  static disposeAll(): void {
    for (const instance of this.instances.values()) {
      instance.dispose();
    }
    this.instances.clear();
  }

  /**
   * Reset all client instances. The next request reconnects through Rust-owned
   * daemon lifecycle.
   */
  static async restartAll(options: DaemonRestartOptions = {}): Promise<void> {
    for (const instance of this.instances.values()) {
      await instance.restart(options);
    }
    logger.info(
      options.restartDaemon
        ? "[DaemonChannelServer] All daemon connections restarted"
        : "[DaemonChannelServer] All client connections reset",
    );
  }

  static notifyWriteAll(): void {
    for (const instance of this.instances.values()) {
      instance.notifyWrite();
    }
  }

  /**
   * Check if any server instance is temporarily cooled down after reconnect failures.
   */
  static hasExceededReconnects(): boolean {
    for (const instance of this.instances.values()) {
      const availability = instance.getServerModeAvailability();
      if (!availability.available && availability.reason === "cooldown") {
        return true;
      }
    }
    return false;
  }

  /**
   * Proactive connection: spawns the daemon and connects without waiting
   * for the first request. Call fire-and-forget at activation to reduce
   * first-render latency for the sidebar.
   */
  async warmup(): Promise<void> {
    await this.ensureConnected();
  }

  /**
   * Check if server mode should be used based on environment.
   * Returns false if the feature flag is disabled or if reconnect attempts are cooling down.
   */
  shouldUseServerMode(): boolean {
    return this.getServerModeAvailability().available;
  }

  getServerModeAvailability(): ServerModeAvailability {
    if (process.env.EXOSUIT_USE_SERVER_MODE === "false") {
      return {
        available: false,
        reason: "env-disabled",
        workspaceRoot: this.workspaceRoot,
        envVar: "EXOSUIT_USE_SERVER_MODE",
        value: "false",
      };
    }

    if (this.reconnectCooldownUntilMs !== null) {
      const retryAfterMs = this.reconnectCooldownUntilMs - this.clock.now();
      if (retryAfterMs > 0) {
        return {
          available: false,
          reason: "cooldown",
          workspaceRoot: this.workspaceRoot,
          reconnectAttempts: this.reconnectAttempts,
          maxReconnectAttempts: this.config.maxReconnectAttempts,
          cooldownUntilMs: this.reconnectCooldownUntilMs,
          retryAfterMs,
        };
      }

      this.reconnectCooldownUntilMs = null;
      this.reconnectAttempts = 0;
    }

    return {
      available: true,
      reason: "available",
      workspaceRoot: this.workspaceRoot,
      reconnectAttempts: this.reconnectAttempts,
      maxReconnectAttempts: this.config.maxReconnectAttempts,
    };
  }

  private connection: ConnectionLike | null = null;
  private readConnections: Array<ConnectionLike | null> = [];
  private readConnectPromises: Array<Promise<void> | null> = [];
  private readLaneHasConnected: boolean[] = [];
  private nextReadLane = 0;
  private activeReadLaneConnects = 0;
  private readLaneFailureAccountedForActiveConnects = false;
  private reconnectAttempts = 0;
  private reconnectCooldownUntilMs: number | null = null;
  private reconnectTimer: TimerHandle | null = null;
  private isShuttingDown = false;
  private connectPromise: Promise<void> | null = null;
  /** True after the first successful connect(). Survives cleanup(). */
  private hasEverConnected = false;
  private generation = 0;
  private lastLifecycleResetGeneration: number | null = null;
  private daemonInstanceId: string | null = null;

  private readonly deps: ResolvedDaemonChannelServerDeps;
  private readonly config: DaemonChannelServerConfig;
  private readonly clock: DaemonChannelClock;

  private constructor(
    private readonly workspaceRoot: string,
    deps: Partial<DaemonChannelServerDeps> = {},
  ) {
    const defaultDeps = createDefaultDeps();
    const testingLifecycle = async (): Promise<DaemonEnsureResult> => ({
      runtimeDir: "<test-runtime>",
      socketPath: "<test-socket>",
      pidPath: "<test-pid>",
      reused: true,
      spawned: false,
      state: "connected_existing",
    });
    this.deps = {
      ...defaultDeps,
      ...deps,
      ensureLifecycle:
        deps.ensureLifecycle ??
        (deps.connect ? testingLifecycle : defaultDeps.ensureLifecycle),
      restartLifecycle: deps.restartLifecycle ?? defaultDeps.restartLifecycle,
    };
    this.config = {
      ...CONFIG,
      ...deps.config,
    };
    this.readConnections = Array.from(
      { length: Math.max(1, this.config.readLaneCount) },
      () => null,
    );
    this.readConnectPromises = Array.from(
      { length: this.readConnections.length },
      () => null,
    );
    this.readLaneHasConnected = Array.from(
      { length: this.readConnections.length },
      () => false,
    );
    this.clock = deps.clock ?? DEFAULT_CLOCK;

    if (this.config.reconnectCooldownMs < this.config.reconnectDelayMs) {
      throw new Error(
        "reconnectCooldownMs must be greater than or equal to reconnectDelayMs",
      );
    }
  }

  /**
   * Attempt to handle the request locally within the extension host.
   */
  private tryHandleLocally(
    _envelope: MachineChannelRequestEnvelope,
  ): MachineChannelResponseEnvelope | null {
    return null;
  }

  /**
   * Send a fire-and-forget notification to the daemon.
   * Silently swallows errors — the caller should not await.
   */
  async notify(data: Record<string, unknown>): Promise<void> {
    try {
      await this.ensureConnected();
      this.connection?.notify(data);
    } catch {
      // fire-and-forget — swallow errors
    }
  }

  /**
   * Send a request to the daemon and wait for the response.
   * Transparently connects/reconnects if needed.
   */
  async request(
    envelope: MachineChannelRequestEnvelope,
  ): Promise<MachineChannelResponseEnvelope> {
    if (this.isShuttingDown) {
      throw new Error("DaemonChannelServer is shutting down");
    }

    // Bind one globally unique identity to this logical invocation. Every
    // socket reconnect below reuses this exact envelope.
    envelope = {
      ...envelope,
      id: `${envelope.id}.${randomUUID()}`,
      workspace_root: this.workspaceRoot,
    };

    // No extension-local request handling remains here.
    const localResponse = this.tryHandleLocally(envelope);
    if (localResponse) {
      return localResponse;
    }

    this.assertServerModeAvailable();

    if (isPureReadRequest(envelope)) {
      return this.requestPureRead(envelope);
    }

    // Retry once with the same globally unique request ID after a connection
    // loss. The daemon outcome ledger makes write/exec recovery idempotent.
    for (let attempt = 0; attempt < 2; attempt++) {
      await this.ensureConnected();

      if (!this.connection || this.connection.isClosed()) {
        if (attempt === 0) {
          continue;
        }
        throw new Error("Failed to connect to daemon");
      }

      try {
        const daemonResponse = await this.connection.request(
          envelope,
          this.config.requestTimeoutMs,
        );

        this.markRequestSucceeded();

        // Notify TraceCache if this request may have mutated workspace state.
        if (daemonResponse.effect === "write" || daemonResponse.effect === "exec") {
          this.deps.traceCache.notifyWrite();
        }

        return daemonResponse;
      } catch (error) {
        if (attempt === 0 && isReconnectableConnectionLoss(error)) {
          logger.info(
            `[DaemonChannelServer] request(): connection lost during request ${envelope.id}, recovering recorded outcome...`,
          );
          this.connection = null;
          continue;
        }
        throw error;
      }
    }

    throw new Error("Failed to connect to daemon after retry");
  }

  private async requestPureRead(
    envelope: MachineChannelRequestEnvelope,
  ): Promise<MachineChannelResponseEnvelope> {
    const lane = this.nextReadLaneIndex();
    for (let attempt = 0; attempt < 2; attempt++) {
      const attemptGeneration = this.generation;

      await this.ensureReadConnection(lane);
      if (this.isShuttingDown) {
        throw new Error(
          `Read lane ${lane} request canceled during shutdown or restart for ${this.workspaceRoot}`,
        );
      }
      if (this.generation !== attemptGeneration) {
        if (
          attempt === 0 &&
          this.lastLifecycleResetGeneration === this.generation
        ) {
          continue;
        }
        throw new Error(
          `Read lane ${lane} request canceled during shutdown or restart for ${this.workspaceRoot}`,
        );
      }

      const connection = this.readConnections[lane];
      if (!connection || connection.isClosed()) {
        if (attempt === 0) {
          this.readConnections[lane] = null;
          continue;
        }
        throw new Error(
          `Failed to connect read lane ${lane} to daemon for ${this.workspaceRoot}`,
        );
      }

      try {
        const daemonResponse = await connection.request(
          envelope,
          this.config.requestTimeoutMs,
        );

        this.markRequestSucceeded();

        if (daemonResponse.effect === "write" || daemonResponse.effect === "exec") {
          this.deps.traceCache.notifyWrite();
          throw new Error(
            `Read lane ${lane} received non-pure response effect=${daemonResponse.effect}; workspace=${this.workspaceRoot}; request=${envelope.id}`,
          );
        }

        return daemonResponse;
      } catch (error) {
        if (attempt === 0 && isReconnectableConnectionLoss(error)) {
          logger.info(
            `[DaemonChannelServer] read request(): lane=${lane} workspace=${this.workspaceRoot} connection lost during request, retrying...`,
          );
          this.readConnections[lane] = null;
          continue;
        }
        throw error;
      }
    }

    throw new Error(
      `Failed to connect read lane ${lane} after retry for ${this.workspaceRoot}`,
    );
  }

  /**
   * Ensure connected to daemon. Connects if needed, reconnects if closed.
   */
  private async ensureConnected(): Promise<void> {
    this.assertServerModeAvailable();

    // If already connecting, wait for that to complete
    if (this.connectPromise) {
      return this.connectPromise;
    }

    if (this.connection && !this.connection.isClosed()) {
      if ((await this.ensureDaemonLifecycleReused()) !== null) {
        return;
      }
      return this.ensureConnected();
    }

    const connectGeneration = this.generation;
    await this.delayBeforeReconnect(connectGeneration);
    if (this.generation !== connectGeneration || this.isShuttingDown) {
      return;
    }
    if (this.connectPromise) {
      return this.connectPromise;
    }
    if (this.connection && !this.connection.isClosed()) {
      if ((await this.ensureDaemonLifecycleReused()) !== null) {
        return;
      }
      return this.ensureConnected();
    }

    let lifecycle: DaemonEnsureResult | undefined;
    if (this.hasLiveConnection()) {
      lifecycle = (await this.ensureDaemonLifecycleReused()) ?? undefined;
      if (!lifecycle) {
        return this.ensureConnected();
      }
    }

    // Connect to daemon
    const connectPromise = this.connect(connectGeneration, lifecycle);
    this.connectPromise = connectPromise;
    try {
      await connectPromise;
    } finally {
      if (this.connectPromise === connectPromise) {
        this.connectPromise = null;
      }
    }
  }

  private nextReadLaneIndex(): number {
    for (let index = 0; index < this.readConnections.length; index++) {
      const connection = this.readConnections[index];
      if (connection?.isClosed()) {
        return index;
      }
    }

    const lane = this.nextReadLane % this.readConnections.length;
    this.nextReadLane = (this.nextReadLane + 1) % this.readConnections.length;
    return lane;
  }

  private async ensureDaemonLifecycleReused(): Promise<DaemonEnsureResult | null> {
    const result = await this.deps.ensureLifecycle(this.workspaceRoot);
    const nextInstanceId = result.instanceId ?? null;
    const instanceChanged =
      this.daemonInstanceId !== null &&
      nextInstanceId !== null &&
      this.daemonInstanceId !== nextInstanceId;
    const instanceBecameKnown =
      this.daemonInstanceId === null && nextInstanceId !== null;
    if (
      result.state === "connected_existing" &&
      !instanceChanged &&
      !instanceBecameKnown
    ) {
      return result;
    }

    logger.info(
      `[DaemonChannelServer] Rust daemon lifecycle changed for ${this.workspaceRoot}; state=${result.state ?? "<unknown>"}; instance=${nextInstanceId ?? "<unknown>"}; previousInstance=${this.daemonInstanceId ?? "<unknown>"}; reused=${result.reused ?? "<unknown>"}; spawned=${result.spawned ?? "<unknown>"}`,
    );
    this.daemonInstanceId = null;
    this.generation++;
    this.lastLifecycleResetGeneration = this.generation;
    this.cleanup();
    this.deps.traceCache.notifyWrite();
    return null;
  }

  private hasLiveConnection(): boolean {
    return (
      (this.connection !== null && !this.connection.isClosed()) ||
      this.readConnections.some(
        (connection) => connection !== null && !connection.isClosed(),
      )
    );
  }

  private async ensureReadConnection(index: number): Promise<void> {
    this.assertServerModeAvailable();
    if (this.isShuttingDown) {
      throw new Error(
        `Read lane ${index} connection canceled during shutdown for ${this.workspaceRoot}`,
      );
    }

    const existing = this.readConnections[index];
    if (existing && !existing.isClosed()) {
      if ((await this.ensureDaemonLifecycleReused()) !== null) {
        return;
      }
    }

    if (this.readConnectPromises[index]) {
      return this.readConnectPromises[index] ?? undefined;
    }

    const connectGeneration = this.generation;
    await this.delayBeforeReconnect(connectGeneration);
    if (this.generation !== connectGeneration || this.isShuttingDown) {
      return;
    }
    if (this.readConnectPromises[index]) {
      return this.readConnectPromises[index] ?? undefined;
    }
    const afterDelayExisting = this.readConnections[index];
    if (afterDelayExisting && !afterDelayExisting.isClosed()) {
      if ((await this.ensureDaemonLifecycleReused()) !== null) {
        return;
      }
      return this.ensureReadConnection(index);
    }
    let lifecycle: DaemonEnsureResult | undefined;
    if (this.hasLiveConnection()) {
      lifecycle = (await this.ensureDaemonLifecycleReused()) ?? undefined;
      if (!lifecycle) {
        return this.ensureReadConnection(index);
      }
    }

    const promise = this.connectReadLane(index, connectGeneration, lifecycle);
    this.readConnectPromises[index] = promise;
    try {
      await promise;
    } finally {
      if (this.readConnectPromises[index] === promise) {
        this.readConnectPromises[index] = null;
      }
    }
  }

  private async connectReadLane(
    index: number,
    connectGeneration: number,
    ensuredLifecycle?: DaemonEnsureResult,
  ): Promise<void> {
    const existing = this.readConnections[index];
    if (existing) {
      existing.onClose = null;
      existing.close();
      this.readConnections[index] = null;
    }

    this.beginReadLaneConnect();
    try {
      const lifecycle =
        ensuredLifecycle ??
        (await this.deps.ensureLifecycle(this.workspaceRoot));
      const connection = await this.deps.connect(this.workspaceRoot, lifecycle);
      if (this.generation !== connectGeneration) {
        connection.close();
        throw new Error(
          `Stale read-lane daemon connection ignored after restart; lane=${index}; workspace=${this.workspaceRoot}`,
        );
      }

      this.readConnections[index] = connection;
      this.daemonInstanceId = lifecycle.instanceId ?? null;
      this.markConnected();

      connection.onNotification = (notification) => {
        const kind = (notification.result as { kind?: string })?.kind;
        if (kind === "write_happened") {
          this.deps.traceCache.notifyWrite();
        }
      };

      connection.onClose = () => {
        logger.info(
          `[DaemonChannelServer] Read lane ${index} closed — reconnecting on next read`,
        );
        if (this.readConnections[index] === connection) {
          this.readConnections[index] = null;
        }
      };

      if (this.readLaneHasConnected[index]) {
        this.deps.traceCache.notifyWrite();
      }

      this.hasEverConnected = true;
      this.readLaneHasConnected[index] = true;

      logger.info(
        `[DaemonChannelServer] Connected read lane ${index} to daemon for ${this.workspaceRoot}`,
      );
    } catch (error) {
      if (this.generation !== connectGeneration) {
        logger.info(
          "[DaemonChannelServer] Ignoring stale read-lane connection failure after restart",
          error,
        );
        return;
      }

      this.recordReadLaneConnectFailure();
      this.enterCooldownIfNeeded(error);
      logger.error(
        `[DaemonChannelServer] Failed to connect read lane ${index} for ${this.workspaceRoot}:`,
        error,
      );
      throw error;
    } finally {
      this.finishReadLaneConnect();
    }
  }

  private beginReadLaneConnect(): void {
    this.activeReadLaneConnects++;
  }

  private recordReadLaneConnectFailure(): void {
    if (this.readLaneFailureAccountedForActiveConnects) {
      return;
    }
    this.readLaneFailureAccountedForActiveConnects = true;
    this.reconnectAttempts++;
  }

  private finishReadLaneConnect(): void {
    this.activeReadLaneConnects = Math.max(0, this.activeReadLaneConnects - 1);
    if (this.activeReadLaneConnects === 0) {
      this.readLaneFailureAccountedForActiveConnects = false;
    }
  }

  private async delayBeforeReconnect(connectGeneration: number): Promise<void> {
    if (this.reconnectAttempts <= 0 || this.config.reconnectDelayMs <= 0) {
      return;
    }

    await new Promise<void>((resolve) => {
      this.clock.setTimeout(resolve, this.config.reconnectDelayMs);
    });

    if (this.generation !== connectGeneration || this.isShuttingDown) {
      return;
    }
  }

  /**
   * Connect to the daemon.
   */
  private async connect(
    connectGeneration: number,
    ensuredLifecycle?: DaemonEnsureResult,
  ): Promise<void> {
    // Clean up only the primary connection. Read lanes are independent and may
    // still be serving pure reads while the primary socket reconnects.
    this.cleanupPrimaryConnection();

    try {
      const lifecycle =
        ensuredLifecycle ??
        (await this.deps.ensureLifecycle(this.workspaceRoot));
      const connection = await this.deps.connect(this.workspaceRoot, lifecycle);
      if (this.generation !== connectGeneration) {
        connection.close();
        throw new Error("Stale daemon connection ignored after restart");
      }

      this.connection = connection;
      this.daemonInstanceId = lifecycle.instanceId ?? null;

      this.markConnected();

      // Handle unsolicited push notifications from daemon
      this.connection.onNotification = (notification) => {
        const kind = (notification.result as { kind?: string })?.kind;
        if (kind === "write_happened") {
          this.deps.traceCache.notifyWrite();
        }
      };

      // Auto-reconnect when the daemon dies (idle timeout, crash, kill).
      // On successful reconnect, connect() calls notifyWrite() which
      // invalidates the trace cache and refreshes the sidebar.
      const connectionGeneration = this.generation;
      this.connection.onClose = () => {
        logger.info(
          "[DaemonChannelServer] Connection closed — scheduling reconnect",
        );
        this.connection = null;
        this.clearReconnectTimer();
        this.reconnectTimer = this.clock.setTimeout(() => {
          this.reconnectTimer = null;
          if (this.generation !== connectionGeneration || this.isShuttingDown) {
            return;
          }
          this.ensureConnected().catch((err) => {
            logger.error("[DaemonChannelServer] Auto-reconnect failed:", err);
          });
        }, this.config.autoReconnectDelayMs);
      };

      // On reconnection (not first connect), invalidate cached data.
      // The daemon may have restarted or repaired itself behind this client
      // connection, so cached roots (context-snapshot, phase-details) are stale.
      if (this.hasEverConnected) {
        this.deps.traceCache.notifyWrite();
      }

      // Mark that we've connected at least once (survives cleanup)
      this.hasEverConnected = true;

      logger.info(
        `[DaemonChannelServer] Connected to daemon for ${this.workspaceRoot}`,
      );
    } catch (error) {
      if (this.generation !== connectGeneration) {
        logger.info(
          "[DaemonChannelServer] Ignoring stale connection failure after restart",
          error,
        );
        return;
      }

      this.reconnectAttempts++;
      this.enterCooldownIfNeeded(error);
      logger.error("[DaemonChannelServer] Failed to connect to daemon:", error);

      if (this.reconnectCooldownUntilMs !== null) {
        logger.error(
          `[DaemonChannelServer] Max reconnect attempts (${this.config.maxReconnectAttempts}) exceeded; cooling down for ${Math.max(
            0,
            this.reconnectCooldownUntilMs - this.clock.now(),
          )}ms`,
        );
      }

      throw error;
    }
  }

  /**
   * Clean up connection resources.
   */
  private cleanup(): void {
    this.cleanupPrimaryConnection();
    this.cleanupReadConnections();
  }

  private cleanupPrimaryConnection(): void {
    if (this.connection) {
      // Detach close handler before closing to prevent auto-reconnect
      // from firing on intentional shutdown (cleanup, dispose, client reset).
      this.connection.onClose = null;
      this.connection.close();
      this.connection = null;
    }
  }

  private cleanupReadConnections(): void {
    for (let index = 0; index < this.readConnections.length; index++) {
      const connection = this.readConnections[index];
      if (!connection) {
        continue;
      }
      connection.onClose = null;
      connection.close();
      this.readConnections[index] = null;
    }
    this.readConnectPromises.fill(null);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer) {
      this.clock.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private assertServerModeAvailable(): void {
    const availability = this.getServerModeAvailability();
    if (availability.available) {
      return;
    }

    if (availability.reason === "env-disabled") {
      throw new Error(
        `[DaemonChannelServer] Server mode is disabled by ${availability.envVar}=${availability.value}; workspace=${availability.workspaceRoot}`,
      );
    }

    throw new Error(
      `[DaemonChannelServer] Server mode is cooling down after ${availability.reconnectAttempts} failed reconnect attempts; retryAfterMs=${availability.retryAfterMs}; cooldownUntilMs=${availability.cooldownUntilMs}; workspace=${availability.workspaceRoot}`,
    );
  }

  private markConnected(): void {
    this.reconnectAttempts = 0;
    this.reconnectCooldownUntilMs = null;
    this.clearReconnectTimer();
  }

  private markRequestSucceeded(): void {
    this.reconnectAttempts = 0;
    this.reconnectCooldownUntilMs = null;
  }

  private enterCooldownIfNeeded(error: unknown): void {
    if (this.reconnectAttempts <= this.config.maxReconnectAttempts) {
      return;
    }

    this.reconnectCooldownUntilMs =
      this.clock.now() + this.config.reconnectCooldownMs;
    logger.warn(
      `[DaemonChannelServer] Entering reconnect cooldown; workspace=${this.workspaceRoot}; reconnectAttempts=${this.reconnectAttempts}; maxReconnectAttempts=${this.config.maxReconnectAttempts}; cooldownUntilMs=${this.reconnectCooldownUntilMs}; error=${String(
        error,
      )}`,
    );
  }

  /**
   * Reset local client connection state. The next request reconnects through
   * Rust-owned daemon lifecycle (`daemon ensure`).
   */
  async restart(options: DaemonRestartOptions = {}): Promise<void> {
    logger.info(
      options.restartDaemon
        ? `[DaemonChannelServer] Restarting daemon lifecycle for ${this.workspaceRoot}`
        : `[DaemonChannelServer] Resetting client connection for ${this.workspaceRoot}`,
    );

    let restartError: unknown;
    if (options.restartDaemon) {
      try {
        const result = await this.deps.restartLifecycle(this.workspaceRoot);
        logger.info(
          `[DaemonChannelServer] Rust daemon restart completed for ${this.workspaceRoot}; state=${result.state ?? "<unknown>"}; reused=${result.reused ?? "<unknown>"}; spawned=${result.spawned ?? "<unknown>"}`,
        );
      } catch (error) {
        restartError = error;
      }
    }

    this.reconnectAttempts = 0;
    this.reconnectCooldownUntilMs = null;
    this.isShuttingDown = false;
    this.connectPromise = null;
    this.generation++;
    this.clearReconnectTimer();

    this.cleanup();

    // Local reset invalidates daemon-backed reads. Consumers must drop cached
    // state so the next request reconnects and re-reads fresh data.
    this.deps.traceCache.notifyWrite();

    // Next request() call triggers ensureConnected() → connect()
    if (restartError) {
      throw restartError;
    }
  }

  notifyWrite(): void {
    this.deps.traceCache.notifyWrite();
  }

  /**
   * Gracefully shut down the server.
   */
  dispose(): void {
    this.isShuttingDown = true;
    this.generation++;
    this.clearReconnectTimer();
    this.cleanup();

    // Remove from singleton map
    DaemonChannelServer.instances.delete(this.workspaceRoot);
  }

  /**
   * Check if daemon mode should be used based on environment.
   */
  shouldUseDaemonMode(): boolean {
    // Feature flag check
    if (process.env.EXOSUIT_USE_DAEMON_MODE === "false") {
      return false;
    }

    return this.shouldUseServerMode();
  }
}
