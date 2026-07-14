import * as vscode from "vscode";
import * as fs from "node:fs";
import * as path from "node:path";
import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";
import { resolveExoBinary } from "../exoBin";
import { getLogger } from "../logging";
import picomatch from "picomatch";

type DiscoverySuiteEvent = {
  type: "suite";
  id: string;
  label: string;
  checks: string[];
};

type DiscoveryCheckEvent = {
  type: "check";
  id: string;
  label: string;
  command: string;
  lane: string;
  category?: "observe" | "mutate";
  filters?: string[];
};

type DiscoveryEvent = DiscoverySuiteEvent | DiscoveryCheckEvent;

type ValidateEvent =
  | { type: "lane_started"; lane: string; check_count: number }
  | {
      type: "check_enqueued";
      check_id: string;
      lane: string;
      index: number;
      label: string;
    }
  | {
      type: "check_started";
      check_id: string;
      lane: string;
      label?: string;
      command?: string;
      filters?: string[];
      matched_files?: string[];
    }
  | {
      type: "check_output";
      check_id: string;
      lane: string;
      stream?: string;
      data: string;
    }
  | {
      type: "check_completed";
      check_id: string;
      lane: string;
      status: string;
      duration_ms?: number;
      output_preview?: string | null;
      skip_reason?: string | null;
    }
  | {
      type: "restage_failed";
      check_id: string;
      lane: string;
      error: string;
    }
  | {
      type: "lane_completed";
      lane: string;
      status: string;
      passed?: number;
      failed?: number;
      skipped?: number;
      duration_ms?: number;
    }
  | { type: "summary" };

type ItemMeta = {
  kind: "suite" | "check";
  lane: string;
  checkId?: string;
  category?: "observe" | "mutate";
  filters?: string[];
};

type ValidationMode = "manual" | "continuous";

export function exohookValidateArgs(
  lane: string,
  mode: ValidationMode = "manual",
): string[] {
  const args = ["validate", lane, "--format=jsonl"];
  if (mode === "continuous") {
    args.push("--category", "observe");
  }
  return args;
}

const logger = getLogger("extension");
const INITIAL_DISCOVERY_DELAY_MS = 5_000;

export class ExohookTestController implements vscode.Disposable {
  private readonly controller: vscode.TestController;
  private readonly disposables: vscode.Disposable[] = [];
  private readonly workspaceRoot: string;
  private readonly hooksPath: string;
  private readonly hooksUri: vscode.Uri;
  private readonly suiteItems = new Map<string, vscode.TestItem>();
  private readonly checkItems = new Map<string, vscode.TestItem>();
  private readonly itemMeta = new Map<string, ItemMeta>();
  private currentProcess: ChildProcess | null = null;
  private initialRefreshTimer: ReturnType<typeof setTimeout> | null = null;
  /** Per-check settled results (suite items excluded to avoid inflating counts) */
  private readonly _checkResults = new Map<string, "pass" | "fail" | "skip">();
  /** Whether a continuous save-triggered run is currently executing */
  private _continuousRunActive = false;
  /** Lanes queued for rerun while a continuous run is in progress */
  private _pendingRerunLanes: Set<string> | null = null;

  constructor(workspaceRoot: string) {
    logger.debug("[ExohookTestController] constructor called", {
      workspaceRoot,
    });
    this.workspaceRoot = workspaceRoot;
    this.hooksPath = path.join(workspaceRoot, ".config", "exo", "hooks.toml");
    this.hooksUri = vscode.Uri.file(this.hooksPath);

    this.controller = vscode.tests.createTestController(
      "exohook-checks",
      "Exohook Validation",
    );

    const runProfile = this.controller.createRunProfile(
      "Run",
      vscode.TestRunProfileKind.Run,
      (request, token) => this.runHandler(request, token),
    );
    runProfile.supportsContinuousRun = true;

    this.controller.refreshHandler = () => this.refresh();

    this.disposables.push(this.controller);
    this.setupWatcher();
    this.initialRefreshTimer = setTimeout(() => {
      this.initialRefreshTimer = null;
      void this.refresh();
    }, INITIAL_DISCOVERY_DELAY_MS);
  }

  dispose(): void {
    if (this.initialRefreshTimer !== null) {
      clearTimeout(this.initialRefreshTimer);
      this.initialRefreshTimer = null;
    }
    this.killProcess("dispose");
    this.disposables.forEach((disposable) => disposable.dispose());
    this.disposables.length = 0;
  }

  private setupWatcher(): void {
    const pattern = new vscode.RelativePattern(
      this.workspaceRoot,
      ".config/exo/hooks.toml",
    );
    const watcher = vscode.workspace.createFileSystemWatcher(pattern);
    this.disposables.push(watcher);
    this.disposables.push(
      watcher.onDidChange(() => this.refresh()),
      watcher.onDidCreate(() => this.refresh()),
      watcher.onDidDelete(() => this.clearItems()),
    );
  }

  private clearItems(): void {
    this.controller.items.replace([]);
    this.suiteItems.clear();
    this.checkItems.clear();
    this.itemMeta.clear();
  }

  async refresh(): Promise<void> {
    if (this.initialRefreshTimer !== null) {
      clearTimeout(this.initialRefreshTimer);
      this.initialRefreshTimer = null;
    }
    logger.debug("[ExohookTestController] refresh called", {
      hooksPath: this.hooksPath,
      exists: fs.existsSync(this.hooksPath),
    });
    if (!fs.existsSync(this.hooksPath)) {
      logger.warn("[ExohookTestController] hooks.toml not found, clearing");
      this.clearItems();
      return;
    }

    try {
      const { suites, checks } = await this.runDiscovery();
      logger.info("[ExohookTestController] discovery complete", {
        suites: suites.length,
        checks: checks.length,
      });
      this.populateItems(suites, checks);
    } catch (error) {
      logger.error("[ExohookTestController] discovery failed", error);
      this.clearItems();
    }
  }

  private async runDiscovery(): Promise<{
    suites: DiscoverySuiteEvent[];
    checks: DiscoveryCheckEvent[];
  }> {
    const suites: DiscoverySuiteEvent[] = [];
    const checks: DiscoveryCheckEvent[] = [];

    const bin = this.resolveExohookBin();
    logger.debug("[ExohookTestController] spawning discovery", {
      bin,
      cwd: this.workspaceRoot,
    });

    const child = spawn(bin, ["discover", "--format=jsonl"], {
      cwd: this.workspaceRoot,
      stdio: ["ignore", "pipe", "pipe"],
    });

    const readline = createInterface({
      input: child.stdout!,
      crlfDelay: Infinity,
    });

    readline.on("line", (line) => {
      logger.debug("[ExohookTestController] discover raw line", {
        line: line.substring(0, 200),
      });
      const event = this.parseJsonLine<DiscoveryEvent>(line);
      if (!event) {
        return;
      }

      if (event.type === "suite") {
        suites.push(event);
      } else if (event.type === "check") {
        checks.push(event);
      }
    });

    child.stderr?.on("data", (data: Buffer) => {
      logger.warn(
        `[ExohookTestController] discover stderr: ${data.toString()}`,
      );
    });

    // Track exit code separately — do NOT close readline from here.
    // The `exit` event fires when the process exits, but stdout may still
    // have buffered data that readline hasn't processed yet.
    let exitCode: number | null = null;
    child.on("exit", (code) => {
      logger.debug("[ExohookTestController] discover exited", { code });
      exitCode = code;
    });

    // Wait for readline's `close` event, which fires *after* all buffered
    // lines have been emitted (i.e. after stdout is fully drained).
    await new Promise<void>((resolve, reject) => {
      child.on("error", (error) => {
        logger.error("[ExohookTestController] discover spawn error", error);
        reject(error);
      });
      readline.on("close", () => {
        if (exitCode !== null && exitCode !== 0) {
          reject(new Error(`exohook discover exited with code ${exitCode}`));
        } else {
          resolve();
        }
      });
    });

    return { suites, checks };
  }

  private populateItems(
    suites: DiscoverySuiteEvent[],
    checks: DiscoveryCheckEvent[],
  ): void {
    this.clearItems();

    const suiteItems: vscode.TestItem[] = [];
    const childrenByLane = new Map<string, vscode.TestItem[]>();

    for (const suite of suites) {
      const suiteItem = this.controller.createTestItem(
        this.suiteId(suite.id),
        suite.label,
        this.hooksUri,
      );
      suiteItem.description = `${suite.checks.length} checks`;
      this.suiteItems.set(suite.id, suiteItem);
      this.itemMeta.set(suiteItem.id, { kind: "suite", lane: suite.id });
      childrenByLane.set(suite.id, []);
      suiteItems.push(suiteItem);
    }

    for (const check of checks) {
      if (!this.suiteItems.has(check.lane)) {
        const suiteItem = this.controller.createTestItem(
          this.suiteId(check.lane),
          check.lane,
          this.hooksUri,
        );
        suiteItem.description = "Lane";
        this.suiteItems.set(check.lane, suiteItem);
        this.itemMeta.set(suiteItem.id, { kind: "suite", lane: check.lane });
        childrenByLane.set(check.lane, []);
        suiteItems.push(suiteItem);
      }

      const checkItem = this.controller.createTestItem(
        this.checkId(check.lane, check.id),
        check.label,
        this.hooksUri,
      );
      checkItem.description = check.command;
      this.checkItems.set(this.checkKey(check.lane, check.id), checkItem);
      this.itemMeta.set(checkItem.id, {
        kind: "check",
        lane: check.lane,
        checkId: check.id,
        category: check.category,
        filters: check.filters,
      });

      const children = childrenByLane.get(check.lane);
      if (children) {
        children.push(checkItem);
      }
    }

    for (const [lane, children] of childrenByLane.entries()) {
      const suiteItem = this.suiteItems.get(lane);
      if (suiteItem) {
        suiteItem.children.replace(children);
      }
    }

    this.controller.items.replace(suiteItems);
  }

  private async runHandler(
    request: vscode.TestRunRequest,
    token: vscode.CancellationToken,
  ): Promise<void> {
    if (request.continuous) {
      await this.runContinuous(request, token);
    } else {
      await this.runOnce(request, token);
    }
  }

  private async runOnce(
    request: vscode.TestRunRequest,
    token: vscode.CancellationToken,
    mode: ValidationMode = "manual",
  ): Promise<void> {
    if (!this.hasLoadedItems() && !token.isCancellationRequested) {
      await this.refresh();
    }

    // Full run: reset all check results
    this._checkResults.clear();

    const run = this.controller.createTestRun(request);
    const lanes = this.collectLanes(request);

    for (const lane of lanes) {
      if (token.isCancellationRequested) {
        this.skipLane(run, lane);
        continue;
      }

      await this.runLane(lane, run, token, mode);
    }

    run.end();
    this.publishValidationSnapshot();
  }

  private hasLoadedItems(): boolean {
    let hasItems = false;
    this.controller.items.forEach(() => {
      hasItems = true;
    });
    return hasItems;
  }

  private async runContinuous(
    request: vscode.TestRunRequest,
    token: vscode.CancellationToken,
  ): Promise<void> {
    // Continuous validation observes workspace state without mutating it.
    await this.runOnce(request, token, "continuous");
    if (token.isCancellationRequested) {
      return;
    }

    // Watch for file saves and re-run matching lanes
    const saveListener = vscode.workspace.onDidSaveTextDocument(async (doc) => {
      if (token.isCancellationRequested) {
        return;
      }

      const relativePath = vscode.workspace.asRelativePath(doc.uri, false);
      const matchingLanes = this.findMatchingLanes(relativePath, request);
      if (matchingLanes.length === 0) {
        return;
      }

      // Concurrency guard: if a run is in progress, queue these lanes
      if (this._continuousRunActive) {
        if (!this._pendingRerunLanes) {
          this._pendingRerunLanes = new Set(matchingLanes);
        } else {
          for (const lane of matchingLanes) {
            this._pendingRerunLanes.add(lane);
          }
        }
        return;
      }

      await this.runContinuousLanes(matchingLanes, request, token);
    });

    // Wait until continuous run is toggled off
    await new Promise<void>((resolve) => {
      token.onCancellationRequested(() => {
        saveListener.dispose();
        this._continuousRunActive = false;
        this._pendingRerunLanes = null;
        resolve();
      });
    });
  }

  /**
   * Run a set of lanes as part of a continuous save-triggered revalidation.
   * Uses a concurrency guard: if another save fires while running, the new
   * lanes are queued and executed after the current run completes.
   */
  private async runContinuousLanes(
    lanes: string[],
    request: vscode.TestRunRequest,
    token: vscode.CancellationToken,
  ): Promise<void> {
    this._continuousRunActive = true;
    try {
      const run = this.controller.createTestRun(request);
      for (const lane of lanes) {
        if (token.isCancellationRequested) {
          this.skipLane(run, lane);
          continue;
        }
        await this.runLane(lane, run, token, "continuous");
      }
      run.end();
      this.publishValidationSnapshot();
    } finally {
      this._continuousRunActive = false;
    }

    // Drain queued reruns (latest-pending pattern)
    if (this._pendingRerunLanes && !token.isCancellationRequested) {
      const pendingLanes = Array.from(this._pendingRerunLanes);
      this._pendingRerunLanes = null;
      await this.runContinuousLanes(pendingLanes, request, token);
    }
  }

  /**
   * Find lanes that have at least one check whose filters match the given file path.
   * Checks with no filters match any file.
   */
  private findMatchingLanes(
    relativePath: string,
    request: vscode.TestRunRequest,
  ): string[] {
    const requestedLanes = new Set(this.collectLanes(request));
    const matchingLanes = new Set<string>();

    for (const [_itemId, meta] of this.itemMeta) {
      if (meta.kind !== "check") {
        continue;
      }
      if (!requestedLanes.has(meta.lane)) {
        continue;
      }
      if (matchingLanes.has(meta.lane)) {
        continue;
      }
      if (meta.category === "mutate") {
        continue;
      }

      // No filters = matches everything
      if (!meta.filters || meta.filters.length === 0) {
        matchingLanes.add(meta.lane);
        continue;
      }

      // Check if any filter matches the saved file
      if (picomatch.isMatch(relativePath, meta.filters)) {
        matchingLanes.add(meta.lane);
      }
    }

    return Array.from(matchingLanes);
  }

  private collectLanes(request: vscode.TestRunRequest): string[] {
    const excludedIds = new Set(request.exclude?.map((item) => item.id));
    const lanes = new Set<string>();

    const addItem = (item: vscode.TestItem) => {
      if (excludedIds.has(item.id)) {
        return;
      }
      const meta = this.itemMeta.get(item.id);
      if (meta) {
        lanes.add(meta.lane);
      }
    };

    if (request.include && request.include.length > 0) {
      request.include.forEach(addItem);
    } else {
      this.controller.items.forEach(addItem);
    }

    return Array.from(lanes);
  }

  private async runLane(
    lane: string,
    run: vscode.TestRun,
    token: vscode.CancellationToken,
    mode: ValidationMode = "manual",
  ): Promise<void> {
    const suiteItem = this.suiteItems.get(lane);
    const laneChecks = this.getLaneChecks(lane);
    const completed = new Set<string>();
    let suiteCompleted = false;
    let cancelled = false;
    let processError: unknown = null;

    // Buffer per-check output so we can attach it to failures
    // instead of dumping raw process output into the shared terminal.
    const outputBuffers = new Map<string, string[]>();
    // Track labels for progress lines
    const checkLabels = new Map<string, string>();
    // Track scope info (filters + matched files) per check
    const checkScopes = new Map<
      string,
      { filters: string[]; matchedFiles: string[] }
    >();

    const child = spawn(
      this.resolveExohookBin(),
      exohookValidateArgs(lane, mode),
      {
        cwd: this.workspaceRoot,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );

    this.currentProcess = child;

    const cancelSubscription = token.onCancellationRequested(() => {
      cancelled = true;
      this.killProcess("cancel");
    });

    const readline = createInterface({
      input: child.stdout!,
      crlfDelay: Infinity,
    });

    readline.on("line", (line) => {
      const event = this.parseJsonLine<ValidateEvent>(line);
      if (!event) {
        return;
      }

      switch (event.type) {
        case "lane_started": {
          if (suiteItem) {
            // Don't call run.started(suiteItem) — per VS Code's test-provider-sample,
            // parent items should never have run.* called on them. VS Code auto-rolls
            // up child statuses. Calling run.started() makes it a standalone result.
            run.appendOutput(
              `\r\n── ${suiteItem.label} (${event.check_count ?? "?"} checks) ──\r\n`,
            );
          }
          break;
        }
        case "check_enqueued": {
          const checkItem = this.checkItems.get(
            this.checkKey(event.lane, event.check_id),
          );
          if (checkItem) {
            run.enqueued(checkItem);
          }
          checkLabels.set(event.check_id, event.label);
          break;
        }
        case "check_started": {
          const checkItem = this.getCheckItem(event.lane, event.check_id);
          if (checkItem) {
            run.started(checkItem);
          }
          const label =
            event.label ?? checkLabels.get(event.check_id) ?? event.check_id;
          run.appendOutput(`  ▶ ${label}\r\n`);

          // Store scope info for display at completion time
          if (event.filters?.length || event.matched_files?.length) {
            checkScopes.set(event.check_id, {
              filters: event.filters ?? [],
              matchedFiles: event.matched_files ?? [],
            });
          }

          // Initialize the output buffer for this check
          outputBuffers.set(event.check_id, []);
          break;
        }
        case "check_output": {
          // Buffer output per-check — don't dump to shared terminal
          const buf = outputBuffers.get(event.check_id);
          if (buf) {
            buf.push(event.data);
          }
          break;
        }
        case "restage_failed": {
          const checkItem = this.getCheckItem(event.lane, event.check_id);
          if (checkItem && !completed.has(checkItem.id)) {
            completed.add(checkItem.id);
            run.failed(
              checkItem,
              new vscode.TestMessage(event.error || "Restage failed"),
            );
            const label = checkLabels.get(event.check_id) ?? event.check_id;
            run.appendOutput(`  ✗ ${label} (restage failed)\r\n`);
          }
          break;
        }
        case "check_completed": {
          const checkItem = this.getCheckItem(event.lane, event.check_id);
          const label = checkLabels.get(event.check_id) ?? event.check_id;
          const duration =
            event.duration_ms !== null && event.duration_ms !== undefined
              ? ` (${this.formatDuration(event.duration_ms)})`
              : "";

          if (checkItem && !completed.has(checkItem.id)) {
            completed.add(checkItem.id);

            // Build the detailed output from buffered chunks
            const buf = outputBuffers.get(event.check_id) ?? [];
            const bufferedOutput = buf.join("");

            // Attach buffered output to the test item so it shows in per-test results
            if (bufferedOutput.length > 0) {
              const normalized = bufferedOutput.replace(/\r?\n/g, "\r\n");
              run.appendOutput(normalized, undefined, checkItem);
            }

            // Attach scope info (filters + matched files) to per-item output
            const scope = checkScopes.get(event.check_id);
            if (scope) {
              let scopeOutput = "";
              if (scope.matchedFiles.length > 0) {
                scopeOutput += `\r\n── scope: ${scope.filters.join(", ")} ──\r\n`;
                for (const f of scope.matchedFiles) {
                  scopeOutput += `  ${f}\r\n`;
                }
              } else if (scope.filters.length > 0) {
                scopeOutput += `\r\n── scope: ${scope.filters.join(", ")} — no matching files ──\r\n`;
              }
              if (scopeOutput) {
                run.appendOutput(scopeOutput, undefined, checkItem);
              }
            }

            // Use buffered output for failure message, fall back to output_preview
            const detail =
              bufferedOutput.trim() || event.output_preview || undefined;

            this.applyStatus(
              run,
              checkItem,
              event.status,
              detail,
              event.skip_reason ?? undefined,
            );

            // Clean progress line in the shared terminal
            if (event.status === "success") {
              run.appendOutput(`  ✓ ${label}${duration}\r\n`);
            } else if (event.status === "skipped") {
              const reason = event.skip_reason ? ` — ${event.skip_reason}` : "";
              run.appendOutput(`  ⊘ ${label}${reason}\r\n`);
            } else {
              run.appendOutput(`  ✗ ${label}${duration}\r\n`);
            }
            // Show scope info in shared terminal too
            if (scope) {
              if (scope.matchedFiles.length > 0) {
                run.appendOutput(`    scope: ${scope.filters.join(", ")}\r\n`);
                for (const f of scope.matchedFiles) {
                  run.appendOutput(`      ${f}\r\n`);
                }
              } else if (scope.filters.length > 0) {
                run.appendOutput(
                  `    scope: ${scope.filters.join(", ")} \u2014 no matching files\r\n`,
                );
              }
              checkScopes.delete(event.check_id);
            }
          }

          // Clean up buffer
          outputBuffers.delete(event.check_id);
          break;
        }
        case "lane_completed": {
          if (suiteItem && !suiteCompleted) {
            suiteCompleted = true;

            const p = event.passed ?? 0;
            const f = event.failed ?? 0;
            const s = event.skipped ?? 0;
            const durationMs = event.duration_ms;
            const duration =
              durationMs !== null && durationMs !== undefined
                ? ` in ${this.formatDuration(durationMs)}`
                : "";
            const icon = event.status === "success" ? "✓" : "✗";

            // Summary line in shared terminal only — no run.* on suiteItem
            const summary = `${icon} ${p} passed, ${f} failed, ${s} skipped${duration}`;
            run.appendOutput(`── ${summary} ──\r\n\r\n`);
          }
          break;
        }
        default:
          break;
      }
    });

    // Stderr goes to shared terminal (these are exohook-level errors, not check output)
    child.stderr?.on("data", (data: Buffer) => {
      const normalized = data.toString().replace(/\r?\n/g, "\r\n");
      run.appendOutput(normalized);
    });

    await new Promise<void>((resolve) => {
      readline.on("close", () => resolve());
      child.on("error", (error) => {
        processError = error;
        resolve();
      });
    });

    cancelSubscription.dispose();

    if (this.currentProcess === child) {
      this.currentProcess = null;
    }

    if (cancelled || token.isCancellationRequested) {
      if (suiteItem && !suiteCompleted) {
        run.skipped(suiteItem);
      }
      for (const checkItem of laneChecks) {
        if (!completed.has(checkItem.id)) {
          run.skipped(checkItem);
        }
      }
      return;
    }

    for (const checkItem of laneChecks) {
      if (!completed.has(checkItem.id)) {
        run.skipped(checkItem);
      }
    }

    if (suiteItem && !suiteCompleted) {
      if (processError instanceof Error) {
        run.failed(suiteItem, new vscode.TestMessage(processError.message));
      } else {
        run.skipped(suiteItem);
      }
    }
  }

  private skipLane(run: vscode.TestRun, lane: string): void {
    // Only skip leaf check items — never call run.* on suite items
    for (const checkItem of this.getLaneChecks(lane)) {
      run.skipped(checkItem);
    }
  }

  private applyStatus(
    run: vscode.TestRun,
    item: vscode.TestItem,
    status: string,
    outputPreview?: string,
    skipReason?: string,
  ): void {
    const meta = this.itemMeta.get(item.id);
    const isCheck = meta?.kind === "check";

    switch (status) {
      case "success":
        if (isCheck) {
          this._checkResults.set(item.id, "pass");
        }
        run.passed(item);
        return;
      case "failure":
        if (isCheck) {
          this._checkResults.set(item.id, "fail");
        }
        run.failed(
          item,
          new vscode.TestMessage(outputPreview ?? "Check failed"),
        );
        return;
      case "timeout":
        if (isCheck) {
          this._checkResults.set(item.id, "fail");
        }
        run.failed(item, new vscode.TestMessage("Timed out"));
        return;
      case "cancelled":
      case "skipped":
        if (isCheck) {
          this._checkResults.set(item.id, "skip");
        }
        run.skipped(item);
        return;
      default:
        if (isCheck) {
          this._checkResults.set(item.id, "fail");
        }
        run.failed(item, new vscode.TestMessage(skipReason ?? status));
    }
  }

  /**
   * Snapshot current check results into ValidationService.
   * Counts only check items (not suite items) to avoid inflating counts.
   * Checks not yet in the results map count as pending.
   */
  private publishValidationSnapshot(): void {
    // TODO: validation summary needs a new home (not old ReactiveStateRegistry)
  }

  private getLaneChecks(lane: string): vscode.TestItem[] {
    const items: vscode.TestItem[] = [];
    for (const [key, item] of this.checkItems) {
      if (key.startsWith(`${lane}::`)) {
        items.push(item);
      }
    }
    return items;
  }

  private getCheckItem(
    lane: string,
    checkId: string,
  ): vscode.TestItem | undefined {
    return this.checkItems.get(this.checkKey(lane, checkId));
  }

  private checkKey(lane: string, checkId: string): string {
    return `${lane}::${checkId}`;
  }

  private suiteId(lane: string): string {
    return `suite:${lane}`;
  }

  private checkId(lane: string, checkId: string): string {
    return `check:${lane}:${checkId}`;
  }

  private resolveExohookBin(): string {
    return resolveExoBinary("exohook", this.workspaceRoot);
  }

  private formatDuration(ms: number): string {
    if (ms < 1000) {
      return `${ms}ms`;
    }
    const seconds = ms / 1000;
    if (seconds < 60) {
      return `${seconds.toFixed(1)}s`;
    }
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;
    return `${minutes}m ${remainingSeconds.toFixed(0)}s`;
  }

  private parseJsonLine<T>(line: string): T | null {
    const trimmed = line.trim();
    if (!trimmed) {
      return null;
    }

    try {
      return JSON.parse(trimmed) as T;
    } catch (error) {
      logger.warn("[ExohookTestController] invalid JSONL", {
        line: trimmed,
        error,
      });
      return null;
    }
  }

  private killProcess(reason: "cancel" | "dispose"): void {
    if (!this.currentProcess) {
      return;
    }

    try {
      this.currentProcess.kill("SIGINT");
    } catch (error) {
      logger.warn(
        `[ExohookTestController] failed to kill process (${reason})`,
        {
          error,
        },
      );
    }
  }
}
