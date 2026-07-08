/**
 * Daemon endpoint client for daemon communication (RFC 0097).
 *
 * This module connects to the daemon endpoint reported by Rust-owned lifecycle
 * code. Production callers use `ensureDaemon()`, which shells out to
 * `exo --format json --direct daemon ensure --workspace <root>` and then
 * connects to the returned platform endpoint.
 */

import { type Socket, connect } from "node:net";
import { spawn } from "node:child_process";
import * as path from "node:path";
import {
  createInterface,
  type Interface as ReadlineInterface,
} from "node:readline";
import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../types/machineChannel";
import { resolveExoBinary } from "../exoBin";
import { getLogger } from "../logging";

const logger = getLogger("lmtool");

export const DEFAULT_DAEMON_REQUEST_TIMEOUT_MS = 30_000;
const DAEMON_CONNECT_ATTEMPT_TIMEOUT_MS = 1_000;
const WINDOWS_NAMED_PIPE_RETRY_TIMEOUT_MS = 2_000;
const WINDOWS_NAMED_PIPE_RETRY_INTERVAL_MS = 25;

export interface DaemonRuntimePaths {
  runtimeDir: string;
  socketPath: string;
  endpoint?: string;
  pidPath: string;
}

export interface DaemonEnsureResult extends DaemonRuntimePaths {
  pid?: number | null;
  instanceId?: string | null;
  probeOk?: boolean | null;
  reused: boolean | null;
  spawned: boolean | null;
  state: string | null;
}

export interface DaemonSocketConnector {
  connectToSocket(endpoint: string): Promise<Socket | null>;
}

const defaultSocketConnector: DaemonSocketConnector = {
  connectToSocket,
};

const daemonEnsurePromisesByWorkspace = new Map<
  string,
  Promise<DaemonEnsureResult>
>();

export const FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR =
  "filesystem root is not a valid Exosuit workspace root; run from a git worktree or use project resolve to diagnose";

function assertNotFilesystemRoot(workspaceRoot: string): void {
  const normalized = path.resolve(workspaceRoot);
  if (normalized === path.parse(normalized).root) {
    throw new Error(FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR);
  }
}

export function getSocketPath(paths: DaemonRuntimePaths): string {
  return paths.socketPath;
}

export function getEndpoint(paths: DaemonRuntimePaths): string {
  return paths.endpoint ?? paths.socketPath;
}

export function getPidPath(paths: DaemonRuntimePaths): string {
  return paths.pidPath;
}

export function getRuntimeDir(paths: DaemonRuntimePaths): string {
  return paths.runtimeDir;
}

interface ProjectResolveJson {
  status?: string;
  result?: {
    paths?: {
      runtime_dir?: unknown;
      socket_path?: unknown;
      endpoint?: unknown;
      pid_path?: unknown;
    };
  };
  error?: {
    message?: string;
  };
}

interface DaemonEnsureJson {
  status?: string;
  result?: {
    runtime_dir?: unknown;
    socket_path?: unknown;
    endpoint?: unknown;
    pid_path?: unknown;
    pid?: unknown;
    instance_id?: unknown;
    probe_ok?: unknown;
    reused?: unknown;
    spawned?: unknown;
    state?: unknown;
  };
  error?: {
    message?: string;
  };
}

interface DaemonStatusJson {
  status?: string;
  result?: {
    state?: unknown;
    socket_path?: unknown;
    endpoint?: unknown;
    pid?: unknown;
    pid_alive?: unknown;
    socket_exists?: unknown;
    socket_connectable?: unknown;
    identity_matches_workspace?: unknown;
    identity_matches_executable?: unknown;
  };
  error?: {
    message?: string;
  };
}

export interface DaemonStatusDiagnostic {
  state: string;
  socket_path: string | null;
  endpoint: string | null;
  pid: number | null;
  pid_alive: boolean | null;
  socket_exists: boolean | null;
  socket_connectable: boolean | null;
  identity_matches_workspace: boolean | null;
  identity_matches_executable: boolean | null;
}

function parseProjectResolvePaths(stdout: string): DaemonRuntimePaths {
  let parsed: ProjectResolveJson;
  try {
    parsed = JSON.parse(stdout) as ProjectResolveJson;
  } catch (error) {
    throw new Error(`Failed to parse project resolve output: ${String(error)}`);
  }

  if (parsed.status !== "ok") {
    throw new Error(
      parsed.error?.message ??
        "project resolve failed while finding daemon paths",
    );
  }

  const paths = parsed.result?.paths;
  if (
    typeof paths?.runtime_dir !== "string" ||
    typeof paths.socket_path !== "string" ||
    typeof paths.pid_path !== "string"
  ) {
    throw new Error("project resolve did not return daemon runtime paths");
  }

  return {
    runtimeDir: paths.runtime_dir,
    socketPath: paths.socket_path,
    endpoint: optionalString(paths.endpoint) ?? paths.socket_path,
    pidPath: paths.pid_path,
  };
}

function parseDaemonEnsureResult(stdout: string): DaemonEnsureResult {
  let parsed: DaemonEnsureJson;
  try {
    parsed = JSON.parse(stdout) as DaemonEnsureJson;
  } catch (error) {
    throw new Error(`Failed to parse daemon ensure output: ${String(error)}`);
  }

  if (parsed.status !== "ok") {
    throw new Error(
      parsed.error?.message ??
        "daemon ensure failed while finding daemon paths",
    );
  }

  const paths = parsed.result;
  if (
    typeof paths?.runtime_dir !== "string" ||
    typeof paths.socket_path !== "string" ||
    typeof paths.pid_path !== "string"
  ) {
    throw new Error("daemon ensure did not return daemon runtime paths");
  }

  return {
    runtimeDir: paths.runtime_dir,
    socketPath: paths.socket_path,
    endpoint: optionalString(paths.endpoint) ?? paths.socket_path,
    pidPath: paths.pid_path,
    pid: optionalNumber(paths.pid),
    instanceId: optionalString(paths.instance_id),
    probeOk: optionalBoolean(paths.probe_ok),
    reused: optionalBoolean(paths.reused),
    spawned: optionalBoolean(paths.spawned),
    state: optionalString(paths.state),
  };
}

function optionalBoolean(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function optionalNumber(value: unknown): number | null {
  return typeof value === "number" ? value : null;
}

function optionalString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function parseDaemonStatus(stdout: string): DaemonStatusDiagnostic {
  let parsed: DaemonStatusJson;
  try {
    parsed = JSON.parse(stdout) as DaemonStatusJson;
  } catch (error) {
    throw new Error(`Failed to parse daemon status output: ${String(error)}`);
  }

  if (parsed.status !== "ok") {
    throw new Error(
      parsed.error?.message ?? "daemon status failed while diagnosing socket",
    );
  }

  const result = parsed.result;
  if (typeof result?.state !== "string") {
    throw new Error("daemon status did not return a state");
  }

  return {
    state: result.state,
    socket_path: optionalString(result.socket_path),
    endpoint: optionalString(result.endpoint),
    pid: optionalNumber(result.pid),
    pid_alive: optionalBoolean(result.pid_alive),
    socket_exists: optionalBoolean(result.socket_exists),
    socket_connectable: optionalBoolean(result.socket_connectable),
    identity_matches_workspace: optionalBoolean(result.identity_matches_workspace),
    identity_matches_executable: optionalBoolean(
      result.identity_matches_executable,
    ),
  };
}

function formatDaemonStatus(status: DaemonStatusDiagnostic): string {
  return (
    `daemonStatus.state=${status.state}; ` +
    `daemonStatus.socket_path=${status.socket_path ?? "<none>"}; ` +
    `daemonStatus.endpoint=${status.endpoint ?? "<none>"}; ` +
    `daemonStatus.pid=${status.pid ?? "<none>"}; ` +
    `daemonStatus.pid_alive=${status.pid_alive ?? "<unknown>"}; ` +
    `daemonStatus.socket_exists=${status.socket_exists ?? "<unknown>"}; ` +
    `daemonStatus.socket_connectable=${status.socket_connectable ?? "<unknown>"}; ` +
    `daemonStatus.identity_matches_workspace=${status.identity_matches_workspace ?? "<unknown>"}; ` +
    `daemonStatus.identity_matches_executable=${status.identity_matches_executable ?? "<unknown>"}`
  );
}

function formatPaths(paths: DaemonRuntimePaths): string {
  return (
    `runtimeDir=${paths.runtimeDir}; ` +
    `socketPath=${paths.socketPath}; ` +
    `endpoint=${getEndpoint(paths)}; ` +
    `pidPath=${paths.pidPath}`
  );
}

function runExoProjectResolve(
  exoBin: string,
  workspaceRoot: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(
      exoBin,
      ["--format", "json", "--direct", "project", "resolve"],
      {
        cwd: workspaceRoot,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve(stdout);
      } else {
        reject(
          new Error(
            `project resolve failed with exit code ${code}: ${stderr || stdout}`,
          ),
        );
      }
    });
  });
}

function runExoDaemonEnsure(
  exoBin: string,
  workspaceRoot: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(
      exoBin,
      [
        "--format",
        "json",
        "--direct",
        "daemon",
        "ensure",
        "--workspace",
        workspaceRoot,
      ],
      {
        cwd: workspaceRoot,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve(stdout);
      } else if (stdout.trim()) {
        resolve(stdout);
      } else {
        reject(
          new Error(
            `daemon ensure failed with exit code ${code}: ${stderr || stdout}`,
          ),
        );
      }
    });
  });
}

function runExoDaemonRestart(
  exoBin: string,
  workspaceRoot: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(
      exoBin,
      [
        "--format",
        "json",
        "--direct",
        "daemon",
        "restart",
        "--workspace",
        workspaceRoot,
      ],
      {
        cwd: workspaceRoot,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve(stdout);
      } else if (stdout.trim()) {
        resolve(stdout);
      } else {
        reject(
          new Error(
            `daemon restart failed with exit code ${code}: ${stderr || stdout}`,
          ),
        );
      }
    });
  });
}

function runExoDaemonStatus(
  exoBin: string,
  workspaceRoot: string,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(
      exoBin,
      [
        "--format",
        "json",
        "--direct",
        "daemon",
        "status",
        "--workspace",
        workspaceRoot,
      ],
      {
        cwd: workspaceRoot,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve(stdout);
      } else if (stdout.trim()) {
        resolve(stdout);
      } else {
        reject(
          new Error(
            `daemon status failed with exit code ${code}: ${stderr || stdout}`,
          ),
        );
      }
    });
  });
}

export async function resolveDaemonRuntimePaths(
  workspaceRoot: string,
): Promise<DaemonRuntimePaths> {
  assertNotFilesystemRoot(workspaceRoot);
  const exoBin = resolveExoBinary("exo", workspaceRoot);
  const stdout = await runExoProjectResolve(exoBin, workspaceRoot);
  const paths = parseProjectResolvePaths(stdout);
  logger.info(
    `[socket-client] Resolved daemon runtime paths with ${exoBin} --format json --direct project resolve; cwd=${workspaceRoot}; ${formatPaths(paths)}`,
  );
  return paths;
}

export async function ensureDaemonRuntimePaths(
  workspaceRoot: string,
): Promise<DaemonRuntimePaths> {
  const result = await ensureDaemonLifecycle(workspaceRoot);
  return {
    runtimeDir: result.runtimeDir,
    socketPath: result.socketPath,
    endpoint: result.endpoint,
    pidPath: result.pidPath,
  };
}

export async function ensureDaemonLifecycle(
  workspaceRoot: string,
): Promise<DaemonEnsureResult> {
  assertNotFilesystemRoot(workspaceRoot);
  const exoBin = resolveExoBinary("exo", workspaceRoot);
  const stdout = await runExoDaemonEnsure(exoBin, workspaceRoot);
  const paths = parseDaemonEnsureResult(stdout);
  logger.info(
    `[socket-client] Ensured daemon with ${exoBin} --format json --direct daemon ensure --workspace ${workspaceRoot}; cwd=${workspaceRoot}; ${formatPaths(paths)}`,
  );
  return paths;
}

export async function restartDaemonLifecycle(
  workspaceRoot: string,
): Promise<DaemonEnsureResult> {
  assertNotFilesystemRoot(workspaceRoot);
  const exoBin = resolveExoBinary("exo", workspaceRoot);
  const stdout = await runExoDaemonRestart(exoBin, workspaceRoot);
  const paths = parseDaemonEnsureResult(stdout);
  logger.info(
    `[socket-client] Restarted daemon with ${exoBin} --format json --direct daemon restart --workspace ${workspaceRoot}; cwd=${workspaceRoot}; ${formatPaths(paths)}`,
  );
  return paths;
}

export async function daemonStatus(
  workspaceRoot: string,
): Promise<DaemonStatusDiagnostic> {
  assertNotFilesystemRoot(workspaceRoot);
  const exoBin = resolveExoBinary("exo", workspaceRoot);
  const stdout = await runExoDaemonStatus(exoBin, workspaceRoot);
  const status = parseDaemonStatus(stdout);
  logger.info(
    `[socket-client] Inspected daemon status with ${exoBin} --format json --direct daemon status --workspace ${workspaceRoot}; cwd=${workspaceRoot}; ${formatDaemonStatus(status)}`,
  );
  return status;
}

function daemonEnsureKey(workspaceRoot: string): string {
  return path.resolve(workspaceRoot);
}

async function coalescedEnsureDaemonLifecycle(
  workspaceRoot: string,
): Promise<DaemonEnsureResult> {
  const key = daemonEnsureKey(workspaceRoot);
  const existing = daemonEnsurePromisesByWorkspace.get(key);
  if (existing) {
    return existing;
  }

  const promise = ensureDaemonLifecycle(workspaceRoot);
  daemonEnsurePromisesByWorkspace.set(key, promise);
  void promise
    .finally(() => {
      if (daemonEnsurePromisesByWorkspace.get(key) === promise) {
        daemonEnsurePromisesByWorkspace.delete(key);
      }
    })
    .catch(() => undefined);
  return promise;
}

export function resetDaemonEnsureCacheForTesting(): void {
  daemonEnsurePromisesByWorkspace.clear();
}

function isWindowsNamedPipeEndpoint(endpoint: string): boolean {
  return endpoint.startsWith("\\\\.\\pipe\\");
}

function isTransientWindowsNamedPipeConnectError(error: Error): boolean {
  const code = (error as NodeJS.ErrnoException).code;
  return code === "ENOENT" || code === "EBUSY" || code === "EAGAIN";
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function connectToSocketOnce(
  endpoint: string,
): Promise<{ socket: Socket | null; error: Error | null }> {
  return new Promise((resolve) => {
    const socket = connect(endpoint);
    let settled = false;

    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        socket.destroy();
        resolve({ socket: null, error: null });
      }
    }, DAEMON_CONNECT_ATTEMPT_TIMEOUT_MS);

    socket.once("connect", () => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        socket.removeAllListeners("error");
        resolve({ socket, error: null });
      }
    });

    socket.once("error", (error) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        socket.destroy();
        resolve({ socket: null, error });
      }
    });
  });
}

/**
 * Try to connect to an existing daemon endpoint.
 * Returns the socket if successful, null otherwise.
 */
export async function connectToSocket(endpoint: string): Promise<Socket | null> {
  const retryUntil = isWindowsNamedPipeEndpoint(endpoint)
    ? Date.now() + WINDOWS_NAMED_PIPE_RETRY_TIMEOUT_MS
    : 0;

  for (;;) {
    const result = await connectToSocketOnce(endpoint);
    if (result.socket) {
      return result.socket;
    }

    if (
      !result.error ||
      !isWindowsNamedPipeEndpoint(endpoint) ||
      !isTransientWindowsNamedPipeConnectError(result.error) ||
      Date.now() >= retryUntil
    ) {
      return null;
    }

    await sleep(WINDOWS_NAMED_PIPE_RETRY_INTERVAL_MS);
  }
}

/**
 * Ensure a daemon is running and return a connection to it.
 *
 * Daemon lifecycle authority lives in Rust. This function calls the direct
 * lifecycle endpoint and connects only to the platform endpoint it reports.
 */
export async function ensureDaemon(workspaceRoot: string): Promise<Socket> {
  return ensureDaemonWithConnector(workspaceRoot, defaultSocketConnector);
}

export async function ensureDaemonWithConnector(
  workspaceRoot: string,
  connector: DaemonSocketConnector,
): Promise<Socket> {
  assertNotFilesystemRoot(workspaceRoot);
  const paths = await coalescedEnsureDaemonLifecycle(workspaceRoot);
  return connectToEnsuredDaemonWithConnector(workspaceRoot, paths, connector);
}

export async function connectToEnsuredDaemon(
  workspaceRoot: string,
  paths: DaemonEnsureResult,
): Promise<Socket> {
  return connectToEnsuredDaemonWithConnector(
    workspaceRoot,
    paths,
    defaultSocketConnector,
  );
}

async function connectToEnsuredDaemonWithConnector(
  workspaceRoot: string,
  paths: DaemonEnsureResult,
  connector: DaemonSocketConnector,
): Promise<Socket> {
  const endpoint = getEndpoint(paths);
  const socket = await connector.connectToSocket(endpoint);
  if (!socket) {
    let statusDiagnostic = "daemonStatus=<unavailable>";
    try {
      statusDiagnostic = formatDaemonStatus(await daemonStatus(workspaceRoot));
    } catch (error) {
      statusDiagnostic = `daemonStatus.error=${JSON.stringify(String(error))}`;
    }
    throw new Error(
      `Daemon ensure reported an endpoint but connection failed; workspace=${workspaceRoot}; ${formatPaths(paths)}; ${statusDiagnostic}`,
    );
  }

  logger.info(`[socket-client] Connected to daemon at ${endpoint}`);
  return socket;
}

/**
 * A managed socket connection with request/response correlation.
 *
 * This class wraps a raw socket and provides:
 * - NDJSON framing (one JSON object per line)
 * - Request/response correlation by ID
 * - Timeout handling
 */
export class DaemonConnection {
  private static nextId = 0;
  private socket: Socket;
  private readline: ReadlineInterface;
  private queuedRequests: QueuedRequest[] = [];
  private activeRequest: QueuedRequest | null = null;
  private pendingRequests = new Map<
    string,
    {
      resolve: (response: MachineChannelResponseEnvelope) => void;
      reject: (error: Error) => void;
      timeoutId: ReturnType<typeof setTimeout>;
    }
  >();
  private closed = false;

  /** Callback for unsolicited daemon notifications (e.g., write_happened). */
  onNotification:
    | ((notification: MachineChannelResponseEnvelope) => void)
    | null = null;

  /** Callback fired when the connection closes (daemon died, socket error, etc.). */
  onClose: (() => void) | null = null;

  constructor(socket: Socket) {
    this.socket = socket;
    this.readline = createInterface({
      input: socket,
      crlfDelay: Infinity,
    });

    this.readline.on("line", (line) => this.handleLine(line));
    this.socket.on("close", () => this.handleClose());
    this.socket.on("error", (err) => this.handleError(err));
  }

  /**
   * Send a request and wait for the response.
   */
  async request(
    envelope: MachineChannelRequestEnvelope,
    timeoutMs: number = DEFAULT_DAEMON_REQUEST_TIMEOUT_MS,
  ): Promise<MachineChannelResponseEnvelope> {
    if (this.closed) {
      throw new Error("Connection is closed");
    }

    return new Promise((resolve, reject) => {
      // The connection owns ID generation to guarantee uniqueness.
      // Callers can set envelope.id for logging, but it's overwritten here.
      const id = `req.${++DaemonConnection.nextId}`;
      envelope = { ...envelope, id };
      const requestPath = requestPathForLog(envelope);
      this.queuedRequests.push({
        envelope,
        id,
        enqueuedAt: Date.now(),
        requestPath,
        timeoutMs,
        resolve,
        reject,
      });
      this.drainRequestQueue();
    });
  }

  /**
   * Send a fire-and-forget notification (no response expected).
   */
  notify(data: Record<string, unknown>): void {
    if (this.closed) {
      return;
    }
    const json = JSON.stringify(data) + "\n";
    this.socket.write(json);
  }

  /**
   * Close the connection.
   */
  close(): void {
    this.closeConnection(new Error("Connection closed"), true);
  }

  /**
   * Check if the connection is closed.
   */
  isClosed(): boolean {
    return this.closed;
  }

  private handleLine(line: string): void {
    if (!line.trim()) {
      return;
    }

    try {
      const response = JSON.parse(line) as MachineChannelResponseEnvelope;
      const pending = this.pendingRequests.get(response.id);

      if (pending) {
        clearTimeout(pending.timeoutId);
        this.pendingRequests.delete(response.id);
        if (this.activeRequest?.id === response.id) {
          this.activeRequest = null;
        }
        this.drainRequestQueue();
        pending.resolve(response);
      } else if (response.id === "_notify") {
        // Unsolicited push notification from daemon
        this.onNotification?.(response);
      } else {
        logger.warn(
          `[socket-client] Received response for unknown id: ${response.id}`,
        );
      }
    } catch (err) {
      logger.error(
        `[socket-client] Failed to parse response:`,
        err,
        "line:",
        line,
      );
    }
  }

  private handleClose(): void {
    this.closeConnection(new Error("Connection closed by server"), false);
  }

  private handleError(err: Error): void {
    logger.error(`[socket-client] Socket error:`, err);
    this.closeConnection(err, true);
  }

  private closeConnection(error: Error, destroySocket: boolean): void {
    if (this.closed) {
      return;
    }

    this.closed = true;

    try {
      this.readline.close();
    } catch {
      // Already closed.
    }

    if (destroySocket) {
      this.socket.destroy();
    }

    this.rejectAllPending(error);
    this.rejectAllQueued(error);
    this.onClose?.();
  }

  private drainRequestQueue(): void {
    if (this.closed || this.activeRequest || this.queuedRequests.length === 0) {
      return;
    }

    const request = this.queuedRequests.shift();
    if (!request) {
      return;
    }
    this.activeRequest = request;
    const startedAt = Date.now();
    const queueWaitMs = startedAt - request.enqueuedAt;

    const timeoutId = setTimeout(() => {
      const pending = this.pendingRequests.get(request.id);
      if (!pending) {
        return;
      }
      const error = new Error(
        `Request '${request.id}' timed out after ${request.timeoutMs}ms`,
      );
      logger.warn(
        `[socket-client] Request ${request.id} ${request.requestPath} timed out after ${Date.now() - startedAt}ms (timeout=${request.timeoutMs}ms; queueWait=${queueWaitMs}ms)`,
      );
      this.closeConnection(error, true);
    }, request.timeoutMs);

    this.pendingRequests.set(request.id, {
      resolve: (response) => {
        const activeMs = Date.now() - startedAt;
        logger.info(
          `[socket-client] Request ${request.id} ${request.requestPath} completed in ${activeMs}ms (timeout=${request.timeoutMs}ms; queueWait=${queueWaitMs}ms)`,
        );
        request.resolve(response);
      },
      reject: request.reject,
      timeoutId,
    });

    const data = JSON.stringify(request.envelope) + "\n";
    this.socket.write(data, (error) => {
      if (!error) {
        return;
      }
      logger.warn(
        `[socket-client] Request ${request.id} ${request.requestPath} write failed after ${Date.now() - startedAt}ms (timeout=${request.timeoutMs}ms; queueWait=${queueWaitMs}ms): ${error.message}`,
      );
      this.closeConnection(error, true);
    });
  }

  private rejectAllPending(error: Error): void {
    for (const [id, pending] of this.pendingRequests) {
      clearTimeout(pending.timeoutId);
      pending.reject(error);
      this.pendingRequests.delete(id);
    }
    this.activeRequest = null;
  }

  private rejectAllQueued(error: Error): void {
    for (const queued of this.queuedRequests.splice(0)) {
      queued.reject(error);
    }
  }
}

interface QueuedRequest {
  envelope: MachineChannelRequestEnvelope;
  id: string;
  enqueuedAt: number;
  requestPath: string;
  timeoutMs: number;
  resolve: (response: MachineChannelResponseEnvelope) => void;
  reject: (error: Error) => void;
}

function requestPathForLog(envelope: MachineChannelRequestEnvelope): string {
  const op = envelope.op;
  if (op.kind === "call" || op.kind === "preview") {
    const address = op.params.address;
    if (address.kind === "operation" || address.kind === "namespace") {
      return `${op.kind}:${address.path.join(".")}`;
    }
    return `${op.kind}:root`;
  }
  if (op.kind === "help" || op.kind === "list") {
    const address = op.params.address;
    if (address.kind === "operation" || address.kind === "namespace") {
      return `${op.kind}:${address.path.join(".")}`;
    }
    return `${op.kind}:root`;
  }
  return op.kind;
}
