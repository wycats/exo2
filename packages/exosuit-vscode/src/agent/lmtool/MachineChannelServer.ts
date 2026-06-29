/**
 * MachineChannelServer - Persistent subprocess for Machine Channel v2 (RFC 0097)
 *
 * This class manages a long-running `exo json server` subprocess that handles
 * newline-delimited JSON requests. Unlike spawn-per-request, this eliminates
 * ~100ms process spawn overhead per call.
 *
 * Key features:
 * - Singleton pattern per workspace (avoids multiple server instances)
 * - Transparent restart on crash (no user-visible errors)
 * - Request correlation by ID with timeout handling
 * - Graceful shutdown on extension deactivate
 */

import { spawn, type ChildProcess } from "node:child_process";
import {
  createInterface,
  type Interface as ReadlineInterface,
} from "node:readline";
import { watchFile, unwatchFile } from "node:fs";
import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../../types/machineChannel";
import { getLogger } from "../../logging";

import { resolveExoBinary, isResolvableBinPath } from "../../exoBin";

const logger = getLogger("lmtool");

interface PendingRequest {
  resolve: (response: MachineChannelResponseEnvelope) => void;
  reject: (error: Error) => void;
  timeoutId: ReturnType<typeof setTimeout>;
}

/**
 * Configuration for the server
 */
const CONFIG = {
  /** Timeout for individual requests (ms) */
  requestTimeoutMs: 10_000,
  /** Maximum restart attempts before giving up */
  maxRestartAttempts: 3,
  /** Delay between restart attempts (ms) */
  restartDelayMs: 100,
  /** Grace period for pending requests during shutdown (ms) */
  shutdownGraceMs: 2_000,
} as const;

export class MachineChannelServer {
  /** Singleton instances per workspace root */
  private static instances = new Map<string, MachineChannelServer>();

  /**
   * Get the singleton instance for a workspace root.
   * Creates a new instance if one doesn't exist.
   */
  static getInstance(workspaceRoot: string): MachineChannelServer {
    let instance = this.instances.get(workspaceRoot);
    if (!instance) {
      instance = new MachineChannelServer(workspaceRoot);
      this.instances.set(workspaceRoot, instance);
    }
    return instance;
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
   * Restart all server instances. Resets restart counters and forces a fresh start.
   * Called by the "Exosuit: Restart Machine Channel Server" command.
   */
  static restartAll(): void {
    for (const instance of this.instances.values()) {
      instance.restart();
    }
    logger.info("[MachineChannelServer] All server connections reset");
  }

  /**
   * Check if any server instance has exceeded restart attempts.
   * Used to show appropriate error messages to users.
   */
  static hasExceededRestarts(): boolean {
    for (const instance of this.instances.values()) {
      if (instance.restartAttempts > CONFIG.maxRestartAttempts) {
        return true;
      }
    }
    return false;
  }

  private process: ChildProcess | null = null;
  private readline: ReadlineInterface | null = null;
  private pendingRequests = new Map<string, PendingRequest>();
  private restartAttempts = 0;
  private isShuttingDown = false;
  private startPromise: Promise<void> | null = null;
  private shutdownTimeoutId: ReturnType<typeof setTimeout> | null = null;
  private hasSuccessfulRequest = false;
  /** Path currently being watched for binary changes, or null if not watching. */
  private watchedBinaryPath: string | null = null;
  /** Debounce timer for binary-change restart (cargo touches files multiple times during a build). */
  private binaryChangeDebounce: ReturnType<typeof setTimeout> | null = null;

  private constructor(private readonly workspaceRoot: string) {}

  /**
   * Attempt to handle the request locally within the extension host.
   */
  private tryHandleLocally(
    _envelope: MachineChannelRequestEnvelope,
  ): MachineChannelResponseEnvelope | null {
    return null;
  }

  /**
   * Send a request to the server and wait for the response.
   * Transparently starts/restarts the server if needed.
   */
  async request(
    envelope: MachineChannelRequestEnvelope,
  ): Promise<MachineChannelResponseEnvelope> {
    if (this.isShuttingDown) {
      throw new Error("MachineChannelServer is shutting down");
    }

    // No extension-local request handling remains here.
    const localResponse = this.tryHandleLocally(envelope);
    if (localResponse) {
      return localResponse;
    }

    // Ensure server is running (lazy start or restart if dead)
    await this.ensureRunning();

    const cliResponse = await new Promise<MachineChannelResponseEnvelope>(
      (resolve, reject) => {
        const id = envelope.id;

        // Set up timeout
        const timeoutId = setTimeout(() => {
          const pending = this.pendingRequests.get(id);
          if (pending) {
            this.pendingRequests.delete(id);
            pending.reject(
              new Error(
                `Request '${id}' timed out after ${CONFIG.requestTimeoutMs}ms`,
              ),
            );
          }
        }, CONFIG.requestTimeoutMs);

        // Track pending request
        this.pendingRequests.set(id, { resolve, reject, timeoutId });

        // Send request as newline-delimited JSON
        if (!this.process?.stdin?.writable) {
          clearTimeout(timeoutId);
          this.pendingRequests.delete(id);
          reject(new Error("Server stdin is not writable"));
          return;
        }

        const data = JSON.stringify(envelope) + "\n";
        this.process.stdin.write(data, (error) => {
          if (error) {
            // Clean up pending request on write error
            const pending = this.pendingRequests.get(id);
            if (pending) {
              clearTimeout(pending.timeoutId);
              this.pendingRequests.delete(id);
              pending.reject(error);
            }
          }
        });
      },
    );

    return cliResponse;
  }

  /**
   * Ensure the server is running. Starts it if needed, restarts if crashed.
   */
  private async ensureRunning(): Promise<void> {
    // If already starting, wait for that to complete
    if (this.startPromise) {
      return this.startPromise;
    }

    // If running and healthy, nothing to do
    if (
      this.process &&
      !this.process.killed &&
      this.process.exitCode === null
    ) {
      return;
    }

    // Add delay before restart if this is a restart attempt
    if (this.restartAttempts > 0) {
      await new Promise((resolve) =>
        setTimeout(resolve, CONFIG.restartDelayMs),
      );
    }

    // Start the server
    this.startPromise = this.start();
    try {
      await this.startPromise;
    } finally {
      this.startPromise = null;
    }
  }

  /**
   * Start the server subprocess.
   */
  private async start(): Promise<void> {
    // Clean up any existing process
    this.cleanup();

    this.process = spawn(
      resolveExoBinary("exo", this.workspaceRoot),
      ["json", "server"],
      {
        cwd: this.workspaceRoot,
        stdio: ["pipe", "pipe", "pipe"],
      },
    );

    // Set up readline interface for stdout (one response per line)
    this.readline = createInterface({
      input: this.process.stdout!,
      crlfDelay: Infinity,
    });

    this.readline.on("line", (line: string) => {
      this.handleLine(line);
    });

    // Handle process errors
    this.process.on("error", (err) => {
      logger.error("[MachineChannelServer] Process error:", err);
      this.handleCrash(err);
    });

    // Handle process exit
    this.process.on("exit", (code, signal) => {
      if (!this.isShuttingDown) {
        logger.warn(
          `[MachineChannelServer] Process exited: code=${code}, signal=${signal}`,
        );
        this.handleCrash(new Error(`Server exited with code ${code}`));
      }
    });

    // Log stderr for debugging
    this.process.stderr?.on("data", (data: Buffer) => {
      logger.warn("[MachineChannelServer] stderr:", data.toString());
    });

    // Reset successful request flag - restart attempts will be reset after first successful response
    this.hasSuccessfulRequest = false;

    // Watch the binary for recompilation so the server auto-restarts
    this.watchBinary();
  }

  /**
   * Watch the exo binary for changes (recompilation).
   *
   * Resolves the configured binary path (absolute, relative, or
   * `${workspaceFolder}`-prefixed) via {@link resolveExoBinary} and watches
   * the resulting file.  Bare command names like `"exo"` (PATH lookup)
   * cannot be watched.
   *
   * Uses `fs.watchFile` (stat polling) because it reliably detects
   * atomic replacements (cargo writes a temp file then renames) and
   * the 1 s polling interval is negligible for a binary that rebuilds
   * at most every few seconds.
   */
  private watchBinary(): void {
    const binPath = resolveExoBinary("exo", this.workspaceRoot);

    // Bare names like "exo" resolve via PATH — we can't watch those
    if (!isResolvableBinPath(binPath)) {
      this.unwatchBinary();
      return;
    }

    // Already watching this exact path — nothing to do
    if (this.watchedBinaryPath === binPath) {
      return;
    }

    // Clean up any previous watcher (path may have changed via settings)
    this.unwatchBinary();

    watchFile(binPath, { interval: 1_000 }, (curr, prev) => {
      if (
        curr.mtimeMs !== prev.mtimeMs ||
        curr.size !== prev.size ||
        curr.ino !== prev.ino
      ) {
        // Debounce: cargo can touch the file several times in quick succession
        if (this.binaryChangeDebounce) {
          clearTimeout(this.binaryChangeDebounce);
        }
        this.binaryChangeDebounce = setTimeout(() => {
          this.binaryChangeDebounce = null;
          if (this.isShuttingDown) {
            return;
          }
          logger.info(
            "[MachineChannelServer] exo binary changed on disk — restarting server",
          );
          this.restartAttempts = 0; // Not a crash — don't count toward max restarts
          this.cleanup();
          // Next request() call triggers ensureRunning() → start()
        }, 500);
      }
    });

    this.watchedBinaryPath = binPath;
    logger.debug(
      `[MachineChannelServer] Watching binary for changes: ${binPath}`,
    );
  }

  /**
   * Stop watching the binary file.
   */
  private unwatchBinary(): void {
    if (this.watchedBinaryPath) {
      unwatchFile(this.watchedBinaryPath);
      this.watchedBinaryPath = null;
    }
    if (this.binaryChangeDebounce) {
      clearTimeout(this.binaryChangeDebounce);
      this.binaryChangeDebounce = null;
    }
  }

  /**
   * Handle a line of output from the server.
   */
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
        pending.resolve(response);

        // Reset restart attempts after first successful request
        if (!this.hasSuccessfulRequest) {
          this.hasSuccessfulRequest = true;
          this.restartAttempts = 0;
        }
      } else {
        // Response for unknown request ID - log and ignore
        logger.warn(
          `[MachineChannelServer] Received response for unknown id: ${response.id}`,
        );
      }
    } catch (err) {
      logger.error(
        "[MachineChannelServer] Failed to parse response:",
        err,
        "line:",
        line,
      );

      // Try to extract request ID from malformed line and reject that pending request
      const idMatch = line.match(/"id"\s*:\s*"([^"]+)"/);
      if (idMatch) {
        const extractedId = idMatch[1];
        const pending = this.pendingRequests.get(extractedId);
        if (pending) {
          clearTimeout(pending.timeoutId);
          this.pendingRequests.delete(extractedId);
          pending.reject(
            new Error(`Failed to parse server response: ${String(err)}`),
          );
        }
      }
    }
  }

  /**
   * Handle server crash. Reject pending requests and optionally restart.
   */
  private handleCrash(error: Error): void {
    // Reject all pending requests
    for (const [id, pending] of this.pendingRequests) {
      clearTimeout(pending.timeoutId);
      pending.reject(new Error(`Server crashed: ${error.message}`));
      this.pendingRequests.delete(id);
    }

    this.cleanup();

    // Auto-restart logic is handled by next request calling ensureRunning()
    // We just track restart attempts to avoid infinite loops
    this.restartAttempts++;

    if (this.restartAttempts > CONFIG.maxRestartAttempts) {
      logger.error(
        `[MachineChannelServer] Max restart attempts (${CONFIG.maxRestartAttempts}) exceeded`,
      );
    }
  }

  /**
   * Clean up subprocess resources.
   */
  private cleanup(): void {
    if (this.readline) {
      this.readline.close();
      this.readline = null;
    }

    if (this.process) {
      try {
        this.process.stdin?.end();
        this.process.kill();
      } catch {
        // Ignore errors during cleanup
      }
      this.process = null;
    }
  }

  /**
   * Restart the server. Resets the restart counter and forces a fresh start.
   * Unlike dispose(), this keeps the instance in the singleton map.
   */
  restart(): void {
    logger.info(
      `[MachineChannelServer] Restarting server for ${this.workspaceRoot}`,
    );

    // Reset state
    this.restartAttempts = 0;
    this.isShuttingDown = false;
    this.hasSuccessfulRequest = false;

    // Clean up existing process
    this.cleanup();

    // The next request() call will trigger ensureRunning() → start()
  }

  /**
   * Gracefully shut down the server.
   */
  dispose(): void {
    this.isShuttingDown = true;

    // Clear any existing shutdown timeout
    if (this.shutdownTimeoutId) {
      clearTimeout(this.shutdownTimeoutId);
      this.shutdownTimeoutId = null;
    }

    // Give pending requests a grace period
    if (this.pendingRequests.size > 0) {
      logger.trace(
        `[MachineChannelServer] Waiting for ${this.pendingRequests.size} pending requests...`,
      );

      // Set a hard timeout for shutdown
      this.shutdownTimeoutId = setTimeout(() => {
        this.shutdownTimeoutId = null;
        this.forceDispose();
      }, CONFIG.shutdownGraceMs);
    } else {
      this.forceDispose();
    }
  }

  /**
   * Force dispose without waiting for pending requests.
   */
  private forceDispose(): void {
    // Clear shutdown timeout if it exists
    if (this.shutdownTimeoutId) {
      clearTimeout(this.shutdownTimeoutId);
      this.shutdownTimeoutId = null;
    }

    // Reject remaining pending requests
    for (const [id, pending] of this.pendingRequests) {
      clearTimeout(pending.timeoutId);
      pending.reject(new Error("Server shutting down"));
      this.pendingRequests.delete(id);
    }

    this.unwatchBinary();
    this.cleanup();

    // Remove from singleton map
    MachineChannelServer.instances.delete(this.workspaceRoot);
  }

  /**
   * Check if server mode should be used based on environment.
   * Returns false if the feature flag is disabled or if we've exceeded restart attempts.
   */
  shouldUseServerMode(): boolean {
    // Feature flag check
    if (process.env.EXOSUIT_USE_SERVER_MODE === "false") {
      return false;
    }

    // Bail out if we've had too many restarts
    if (this.restartAttempts > CONFIG.maxRestartAttempts) {
      return false;
    }

    return true;
  }
}
