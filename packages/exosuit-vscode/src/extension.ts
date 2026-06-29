import * as vscode from "vscode";

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as cp from "child_process";
import * as util from "util";

import type { TraceCache } from "./services/TraceCache";
import { PromptService } from "./PromptService";
import { editorService } from "./services/EditorService";
import { documentService } from "./services/DocumentService";
import { fileSystemService } from "./services/FileSystemService";
import { configurationService } from "./services/ConfigurationService";
import { commandService } from "./services/CommandService";
import { LogService } from "./LogService";
import { ExosuitTreeProvider } from "./ExosuitTreeProvider";
import { DebugLogProvider } from "./DebugLogProvider";
import { createEpochContextProvider } from "./EpochContextProvider";
import { createRfcPipelineProvider } from "./RfcPipelineProvider";
import { TreeDecorationProvider } from "./TreeDecorationProvider";
import { createPhaseDetailsProvider } from "./PhaseDetailsProvider";
import { createIdeasTreeProvider } from "./IdeasTreeProvider";
import { createSidecarStatusProvider } from "./SidecarStatusProvider";

import { ExosuitNotebookSerializer } from "./notebook/serializer";
import { ExosuitNotebookController } from "./notebook/controller";
import { ExosuitCommentController } from "./notebook/comments";
import { PlanService } from "./PlanService";
import { createLogsTool } from "./lmtool/logs-tool";
import { createDiagnosticsTool } from "./lmtool/diagnostics-tool";
import { createChatHistoryTool } from "./lmtool/chat-history-tool";
import { createExoRunTool } from "./lmtool/exo-run";
import { createPingTool, createPingToolIdentity } from "./lmtool/ping-tool";
import { ExohookTestController } from "./services/ExohookTestController";
import { ExosuitTaskProvider } from "./tasks/ExosuitTaskProvider";
import { InboxStatusBarService } from "./services/InboxStatusBarService";

import { DiagnosticsStatusBarService } from "./services/DiagnosticsStatusBarService";
import { PhaseStatusBarService } from "./services/PhaseStatusBarService";
import { MachineChannelServer } from "./machine-channel/server";
import { getLogger, initializeLogging } from "./logging";
import type { SidecarPaneAction } from "./types/sidecarStatus";
import type { PlanItem } from "@exosuit/core";
import {
  queuePlanReorganizationRequest,
  resolvePlanEntityId,
  type PlanReorganizationAction,
} from "./services/PlanReorganizationRequests";

import { exoCommand } from "./exoBin";
import { selectCurrentWorkspaceRoot } from "./workspaceRoot";

declare const __BUILD_STAMP__: string;

const logger = getLogger("extension");

async function sidecarActionInput(
  action: SidecarPaneAction,
): Promise<{ path: string[]; input: Record<string, unknown> } | null> {
  switch (action.kind) {
    case "bootstrap":
      return { path: ["sidecar", "bootstrap"], input: { discover: true } };
    case "commit": {
      const match = action.command.match(/--message\s+"([^"]+)"/);
      const defaultMessage = match?.[1] ?? "Update sidecar state";
      const message = await vscode.window.showInputBox({
        title: "Commit sidecar changes",
        prompt: "Commit message for the sidecar repository",
        value: defaultMessage,
      });
      return message
        ? { path: ["sidecar", "repo"], input: { action: "commit", message } }
        : null;
    }
    case "configure-remote": {
      const match = action.command.match(/--url\s+(\S+)/);
      const defaultUrl = match?.[1] === "<url>" ? undefined : match?.[1];
      const url = await vscode.window.showInputBox({
        title: "Configure sidecar remote",
        prompt: "Remote URL for the sidecar repository",
        value: defaultUrl,
        placeHolder: "git@github.com:owner/repo-exosuit-state.git",
      });
      return url
        ? { path: ["sidecar", "repo"], input: { action: "remote", url } }
        : null;
    }
    case "push":
      return { path: ["sidecar", "repo"], input: { action: "push" } };
    case "inspect": {
      if (/\bsidecar\s+repo\s+status\b/.test(action.command)) {
        return { path: ["sidecar", "repo"], input: { action: "status" } };
      }
      if (/\bsidecar\s+status\b/.test(action.command)) {
        return { path: ["sidecar", "status"], input: {} };
      }
      return { path: ["sidecar", "discover"], input: {} };
    }
    case "repair":
      if (/\bsidecar\s+setup\b/.test(action.command)) {
        return { path: ["sidecar", "setup"], input: {} };
      }
      vscode.window.showInformationMessage(action.command);
      return null;
  }
}

export async function activate(context: vscode.ExtensionContext) {
  // ============================================================================
  // DEFERRED FEATURES (RFC 0096)
  // ============================================================================
  // The following features are declared in package.json but not yet activated:
  //
  // Chat Participants:
  // - @exosuit: General-purpose chat interface (waiting for LM tool maturity)
  // - @exosuit-triage: Specialized triage workflow (waiting for triage RFC)
  //
  // These are intentionally NOT activated. See docs/rfcs/stage-0/0096-*.md
  // ============================================================================

  const tracePath = path.join(
    os.tmpdir(),
    "exosuit-vscode-test-activation.log",
  );
  const trace = (msg: string) => {
    try {
      if (context.extensionMode === vscode.ExtensionMode.Test) {
        fs.appendFileSync(tracePath, `${new Date().toISOString()} ${msg}\n`);
      }
    } catch {
      // ignore
    }
  };

  const workspaceSelection = selectCurrentWorkspaceRoot();
  const workspaceRoot = workspaceSelection.rootPath;
  const pingToolIdentity = createPingToolIdentity(context);
  const dogfoodStatus = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    91,
  );
  dogfoodStatus.name = "Exosuit Dogfood Activation";
  dogfoodStatus.command = "exosuit.verifyDogfoodActivation";
  dogfoodStatus.text = "$(sync~spin) Exosuit dogfood";
  dogfoodStatus.tooltip = "Checking Exosuit dogfood activation";
  context.subscriptions.push(dogfoodStatus);
  if (workspaceRoot) {
    dogfoodStatus.show();
  }

  type DogfoodVerifyResult = {
    ok?: boolean;
    receipt_path?: string;
    receipt_skipped?: boolean;
    receipt?: { present?: boolean; matches?: boolean; mismatches?: Array<{ field?: string }> };
    split_brain?: { errors?: number; warnings?: number };
    portability?: {
      errors?: number;
      warnings?: number;
      sidecar_git?: { severity?: string; issue?: string | null };
    };
    repair?: { preview_command?: string | null };
  };

  const dogfoodIssue = (result: DogfoodVerifyResult | undefined): string => {
    if (!result) {
      return "activation check failed";
    }
    if (result.receipt?.present === false) {
      return "activation baseline missing";
    }
    if (result.receipt?.matches === false) {
      const field = result.receipt.mismatches?.[0]?.field;
      return field ? `activation baseline changed: ${field}` : "activation baseline changed";
    }
    if ((result.split_brain?.errors ?? 0) > 0) {
      return "split-brain repair required";
    }
    if ((result.portability?.errors ?? 0) > 0) {
      return result.portability?.sidecar_git?.issue ?? "sidecar portability failed";
    }
    return "activation mismatch";
  };

  const updateDogfoodStatus = (
    result: DogfoodVerifyResult | undefined,
    error?: unknown,
  ) => {
    if (!workspaceRoot) {
      dogfoodStatus.hide();
      return;
    }
    if (error) {
      dogfoodStatus.text = "$(error) Exosuit dogfood";
      dogfoodStatus.tooltip = `Dogfood activation failed: ${String(error)}`;
      dogfoodStatus.show();
      return;
    }
    if (!result?.receipt?.present) {
      dogfoodStatus.text = "$(circle-slash) Exosuit dogfood";
      dogfoodStatus.tooltip = "No saved activation baseline found for this workspace";
      dogfoodStatus.show();
      return;
    }
    if (result.ok) {
      dogfoodStatus.text = "$(verified) Exosuit dogfood";
      dogfoodStatus.tooltip = `Dogfood activation verified: ${result.receipt_path ?? "saved activation baseline present"}`;
    } else {
      dogfoodStatus.text = "$(warning) Exosuit dogfood";
      dogfoodStatus.tooltip = `Dogfood activation failed: ${dogfoodIssue(result)}`;
    }
    dogfoodStatus.show();
  };

  const verifyDogfoodActivation = async (showWhenNoReceipt: boolean) => {
    if (!workspaceRoot) {
      updateDogfoodStatus(undefined);
      if (showWhenNoReceipt) {
        vscode.window.showWarningMessage("Exosuit dogfood: no workspace folder open.");
      }
      return null;
    }

    const response = await MachineChannelServer.getInstance(workspaceRoot).request({
      protocol_version: 1,
      id: `vscode.dogfood.verify.${Date.now()}`,
      op: {
        kind: "call",
        params: {
          address: { kind: "operation", path: ["dogfood", "verify"] },
          input: {
            "require-daemon": true,
            "extension-build-stamp": __BUILD_STAMP__,
            "extension-path": context.extensionUri.fsPath,
          },
        },
      },
    });

    if (response.status !== "ok") {
      const message =
        response.error?.message ?? "dogfood activation check failed";
      updateDogfoodStatus(undefined, message);
      if (showWhenNoReceipt) {
        vscode.window.showErrorMessage(`Exosuit dogfood failed: ${message}`);
      }
      throw new Error(message);
    }

    const result = response.result as DogfoodVerifyResult | undefined;
    updateDogfoodStatus(result);
    const hasReceipt = result?.receipt?.present === true;
    if (!hasReceipt && !showWhenNoReceipt) {
      return result ?? null;
    }

    if (!showWhenNoReceipt) {
      return result ?? null;
    }

    if (result?.ok) {
      vscode.window.showInformationMessage(
        `Exosuit dogfood activation verified: ${result.receipt_path ?? "saved activation baseline present"}`,
      );
    } else {
      const errors = result?.split_brain?.errors ?? 0;
      const warnings = result?.split_brain?.warnings ?? 0;
      const repair = result?.repair?.preview_command;
      const issue = dogfoodIssue(result);
      vscode.window.showErrorMessage(
        [
          "Exosuit dogfood activation failed.",
          issue ? `${issue}.` : undefined,
          `split-brain errors=${errors}, warnings=${warnings}.`,
          repair ? `Run ${repair} for a repair preview.` : undefined,
        ]
          .filter(Boolean)
          .join(" "),
      );
    }

    return result ?? null;
  };

  // Hoisted so command handlers below can access it after initialization.
  let getTraceCache: (() => TraceCache) | undefined;
  let handleStaleFocusedPhase: (() => void) | undefined;
  let projectPlanProviderForCommands: ExosuitTreeProvider | undefined;

  if (workspaceRoot) {
    const server = MachineChannelServer.getInstance(workspaceRoot);
    // Proactive connection: give the daemon time to spawn before the
    // sidebar asks for data. Fire-and-forget — failures are absorbed;
    // the sidebar retry (TracedProvider) handles the fallback.
    server.warmup().catch(() => {});
    // File-save activity events (Agent Activity Model, RFC 10183).
    // Fire-and-forget notification to daemon — no response expected.
    context.subscriptions.push(
      vscode.workspace.onDidSaveTextDocument((doc) => {
        const rel = vscode.workspace.asRelativePath(doc.uri, false);
        if (rel === doc.uri.fsPath) {
          return;
        }
        if (
          rel.startsWith(".git/") ||
          rel.startsWith("node_modules/") ||
          rel.startsWith(".runtime/") ||
          rel.startsWith(".cache/") ||
          rel.startsWith("target/")
        ) {
          return;
        }
        server
          .notify({
            kind: "activity_event",
            event_type: "file_save",
            summary: `file saved: ${rel}`,
          })
          .catch(() => {}); // fire-and-forget
      }),
    );

    context.subscriptions.push({
      dispose: () => {
        MachineChannelServer.disposeAll();
      },
    });

    // Initialize TraceCache for daemon-trace-based reactive invalidation
    const tc = await import("./services/TraceCache");
    getTraceCache = tc.getTraceCache;
    const traceCache = tc.getTraceCache();
    traceCache.setWorkspaceRoot(workspaceRoot);

    // Register roots that the sidebar cares about
    traceCache.registerRoot("context-snapshot", {
      namespace: "context",
      operation: "snapshot",
    });
    traceCache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });
    traceCache.registerRoot("status", {
      namespace: "",
      operation: "status",
    });
    traceCache.registerRoot("sidecar-status", {
      namespace: "sidecar",
      operation: "status",
    });
    traceCache.registerRoot("sidecar-repo-status", {
      namespace: "sidecar",
      operation: "repo",
      input: { action: "status" },
    });
    traceCache.registerRoot("plan-read", {
      namespace: "plan",
      operation: "read",
    });
    traceCache.registerRoot("rfc-pipeline", {
      namespace: "rfc",
      operation: "pipeline",
    });

    const dogfoodActivationTimer = setTimeout(() => {
      verifyDogfoodActivation(false).catch((error) => {
        logger.warn(`Exosuit: Dogfood activation check failed: ${error}`);
      });
    }, 0);
    context.subscriptions.push({
      dispose: () => clearTimeout(dogfoodActivationTimer),
    });

    context.subscriptions.push(
      traceCache.onDidDiagnosticChange((rootId) => {
        if (rootId !== "phase-details") {
          return;
        }

        const diagnostic = traceCache.getDiagnostic(rootId);
        if (
          diagnostic?.status === "empty" &&
          diagnostic.explicitInput &&
          typeof diagnostic.input.id === "string"
        ) {
          handleStaleFocusedPhase?.();
        }
      }),
    );

    context.subscriptions.push({
      dispose: () => tc.disposeTraceCache(),
    });
  }

  // The Playwright Holodeck E2E tests poll for these marker files inside the
  // Holodeck workspace directory.
  //
  // IMPORTANT: Don't write these into a real repo workspace during normal
  // development or unit tests.
  const activationMarkersRoot =
    workspaceRoot && process.env.EXOSUIT_TEST_ID ? workspaceRoot : undefined;

  const progressFilePath = activationMarkersRoot
    ? path.join(activationMarkersRoot, "activation-progress.txt")
    : undefined;

  const outputChannel = vscode.window.createOutputChannel("Exosuit");
  // outputChannel.show(true);
  initializeLogging(outputChannel);
  // Build stamp bypasses logger level filtering — always visible.
  outputChannel.appendLine(
    `[exosuit] Build stamp: ${typeof __BUILD_STAMP__ !== "undefined" ? __BUILD_STAMP__ : "dev"}`,
  );
  logger.info("Exosuit: Activation Started");
  logger.info(
    `[workspace-root] selected=${workspaceRoot ?? "<none>"}; reason=${workspaceSelection.reason}; candidates=${workspaceSelection.candidates.join(",") || "<none>"}`,
  );

  // Provide VS Code tasks from exosuit.toml (and .config/exo/exosuit.toml).
  // These tasks execute via `exo run <task>` to dogfood the CLI as the canonical runner.
  context.subscriptions.push(
    vscode.tasks.registerTaskProvider(
      ExosuitTaskProvider.type,
      new ExosuitTaskProvider(),
    ),
  );
  logger.debug(
    `Exosuit: Registered TaskProvider type: ${ExosuitTaskProvider.type}`,
  );
  trace(`activation: registered task provider: ${ExosuitTaskProvider.type}`);

  trace(
    `activate() entered; extensionMode=${
      vscode.ExtensionMode[context.extensionMode]
    }; EXOSUIT_TEST_WRAPPER=${process.env.EXOSUIT_TEST_WRAPPER ?? "<unset>"}`,
  );

  const logProgress = async (msg: string) => {
    if (!progressFilePath) {
      return;
    }

    try {
      fs.appendFileSync(progressFilePath, `${msg}\n`, "utf8");
    } catch {
      // ignore
    }
  };

  // Test-critical debug commands must be registered synchronously, before any
  // awaited activation work yields back to the event loop. The VS Code test
  // host can begin executing tests quickly after activation starts.
  //
  // These handlers are intentionally tolerant of early activation state.
  const debugDumpState = async () => {
    logger.info("Exosuit: Dumping State...");
    if (!workspaceRoot) {
      return { error: "No workspace folder" };
    }

    const state = {
      plan: undefined as unknown,
      rfcs: undefined as unknown,
      timestamp: Date.now(),
    };

    // Write to disk for the test runner to pick up
    const debugDir = path.join(workspaceRoot, ".debug");
    await fs.promises.mkdir(debugDir, { recursive: true });
    const dumpPath = path.join(debugDir, "debug-state.json");
    await fs.promises.writeFile(
      dumpPath,
      JSON.stringify(state, null, 2),
      "utf8",
    );

    logger.info(`Exosuit: State dumped to ${dumpPath}`);
    return state;
  };

  const debugListExtensions = () => {
    const extensions = vscode.extensions.all.map((ext) => ({
      id: ext.id,
      packageJSON: ext.packageJSON,
    }));
    return extensions;
  };

  context.subscriptions.push(
    vscode.commands.registerCommand("exosuit.debug.dumpState", debugDumpState),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "exosuit.debug.listExtensions",
      debugListExtensions,
    ),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "exosuit.restartMachineChannel",
      async () => {
        await MachineChannelServer.restartAll({ restartDaemon: true });
        vscode.window.showInformationMessage(
          "Exo daemon restarted. Next tool call will reconnect through Exo daemon lifecycle.",
        );
      },
    ),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "exosuit.verifyDogfoodActivation",
      async () => {
        try {
          return await verifyDogfoodActivation(true);
        } catch (error) {
          logger.warn(
            `Exosuit: Dogfood activation verification failed: ${error}`,
          );
          return null;
        }
      },
    ),
  );
  if (process.env.EXOSUIT_TEST_ID) {
    context.subscriptions.push(
      vscode.commands.registerCommand("exosuit.test.notifyWrite", async () => {
        MachineChannelServer.notifyWriteAll();
        PlanService.instance.invalidate();
        projectPlanProviderForCommands?.refresh();
      }),
    );
  }
  trace("activation: registered debug commands (early)");

  try {
    trace("activation: try block entered");
    await logProgress("Starting Activation");
    trace("activation: after logProgress(Starting Activation)");

    const declaredLmTools = new Set(
      (context.extension.packageJSON?.contributes?.languageModelTools ?? [])
        .map((tool: { name?: string }) => tool.name)
        .filter((name: unknown): name is string => typeof name === "string"),
    );

    // Factories keyed by canonical tool name. Used for initial registration
    // and for the `exosuit.resetLmTools` command, which disposes all active
    // tool registrations and re-creates them. Re-registration is a first
    // attempt at working around VS Code issue #295683, where a tool can
    // report itself disabled mid-session despite the picker showing it
    // enabled. It is not guaranteed to work (the corruption lives in the
    // chat layer, above the tool provider), but it's cheaper than a window
    // reload.
    const toolFactories = new Map<
      string,
      () => vscode.LanguageModelTool<unknown>
    >();
    const activeToolDisposables = new Map<string, vscode.Disposable>();

    const registerLmTool = <T>(
      toolName: string,
      factory: () => vscode.LanguageModelTool<T>,
    ): boolean => {
      const canonicalName = toolName;
      if (!declaredLmTools.has(canonicalName)) {
        return false;
      }
      if (activeToolDisposables.has(canonicalName)) {
        return false;
      }

      toolFactories.set(
        canonicalName,
        factory as () => vscode.LanguageModelTool<unknown>,
      );
      const disposable = vscode.lm.registerTool(canonicalName, factory());
      context.subscriptions.push(disposable);
      activeToolDisposables.set(canonicalName, disposable);
      return true;
    };

    // =========================================================================
    // LM Tool Registration
    // =========================================================================
    // 1. Extension-native tools (logs, diagnostics, chat-history, ping)
    // 2. exo-run: universal CLI delegation tool
    // =========================================================================

    // 1. Extension-native tools (not routed through machine channel)
    // These tools access VS Code extension internals directly
    try {
      if (registerLmTool("exo-logs", createLogsTool)) {
        logger.debug("Exosuit: Registered exo-logs LM tool");
        trace("activation: registered exo-logs tool");
      }
    } catch (logsToolError) {
      logger.warn(`Exosuit: Failed to create logs tool: ${logsToolError}`);
      trace(`activation: logs tool creation failed: ${logsToolError}`);
    }

    try {
      if (registerLmTool("exo-diagnostics", createDiagnosticsTool)) {
        logger.debug("Exosuit: Registered exo-diagnostics LM tool");
        trace("activation: registered exo-diagnostics tool");
      }
    } catch (diagnosticsToolError) {
      logger.warn(
        `Exosuit: Failed to create diagnostics tool: ${diagnosticsToolError}`,
      );
      trace(
        `activation: diagnostics tool creation failed: ${diagnosticsToolError}`,
      );
    }

    try {
      if (registerLmTool("exo-ai-chat-history", createChatHistoryTool)) {
        logger.debug("Exosuit: Registered exo-ai-chat-history LM tool");
        trace("activation: registered exo-ai-chat-history tool");
      }
    } catch (chatHistoryError) {
      logger.warn(
        `Exosuit: Failed to create chat-history tool: ${chatHistoryError}`,
      );
      trace(
        `activation: chat-history tool creation failed: ${chatHistoryError}`,
      );
    }

    try {
      if (registerLmTool("exo-ping", () => createPingTool(pingToolIdentity))) {
        logger.debug("Exosuit: Registered exo-ping LM tool");
        trace("activation: registered exo-ping tool");
      }
    } catch (pingToolError) {
      logger.warn(`Exosuit: Failed to create ping tool: ${pingToolError}`);
      trace(`activation: ping tool creation failed: ${pingToolError}`);
    }

    try {
      if (registerLmTool("exo-run", createExoRunTool)) {
        logger.debug("Exosuit: Registered exo-run LM tool");
        trace("activation: registered exo-run tool");
      }
    } catch (exoRunError) {
      logger.warn(`Exosuit: Failed to create exo-run tool: ${exoRunError}`);
      trace(`activation: exo-run tool creation failed: ${exoRunError}`);
    }

    // Reset command: dispose all tool registrations and re-create them.
    // First attempt at working around VS Code #295683 (tool disabled
    // mid-session despite picker showing enabled).
    context.subscriptions.push(
      vscode.commands.registerCommand("exosuit.resetLmTools", async () => {
        const names = [...activeToolDisposables.keys()];
        for (const name of names) {
          const disposable = activeToolDisposables.get(name);
          disposable?.dispose();
          activeToolDisposables.delete(name);
        }
        for (const name of names) {
          const factory = toolFactories.get(name);
          if (!factory) {
            continue;
          }
          try {
            const disposable = vscode.lm.registerTool(name, factory());
            context.subscriptions.push(disposable);
            activeToolDisposables.set(name, disposable);
          } catch (resetError) {
            logger.warn(
              `Exosuit: Failed to re-register ${name}: ${resetError}`,
            );
          }
        }
        vscode.window.showInformationMessage(
          `Exosuit: Reset ${names.length} LM tool(s). If the picker still shows them enabled but chat reports them disabled, also toggle them off/on in the Configure Tools dialog.`,
        );
      }),
    );

    // Note: debug commands are registered synchronously at the top of activate().

    // 0. Register Notebook Serializer IMMEDIATELY
    context.subscriptions.push(
      vscode.workspace.registerNotebookSerializer(
        "exosuit-plan",
        new ExosuitNotebookSerializer(),
        { transientOutputs: true },
      ),
    );
    await logProgress("Registered Notebook Serializer");
    trace("activation: registered notebook serializer");
    logger.debug("Exosuit: Registered Notebook Serializer");

    // 1. Register Epoch Context Provider (RFC 00232)
    const epochContextProvider = createEpochContextProvider();
    context.subscriptions.push(epochContextProvider);
    context.subscriptions.push(
      vscode.window.registerTreeDataProvider(
        "exosuit.epochContext",
        epochContextProvider,
      ),
    );
    logger.debug("Exosuit: Registered EpochContextProvider");
    trace("activation: registered epoch context provider");

    // 1b. Register RFC Pipeline Provider (RFC 00239)
    const rfcPipelineProvider = createRfcPipelineProvider();
    context.subscriptions.push(rfcPipelineProvider);
    context.subscriptions.push(
      vscode.window.registerTreeDataProvider(
        "exosuit.pipeline",
        rfcPipelineProvider,
      ),
    );
    logger.debug("Exosuit: Registered RfcPipelineProvider");
    trace("activation: registered rfc pipeline provider");

    const debugLogProvider = new DebugLogProvider(context.extensionUri);
    context.subscriptions.push(
      vscode.window.registerWebviewViewProvider(
        DebugLogProvider.viewType,
        debugLogProvider,
        { webviewOptions: { retainContextWhenHidden: true } },
      ),
    );
    logger.debug("Exosuit: Registered DebugLogProvider");
    trace("activation: registered debug log provider");

    // 2. Register Tree Data Providers
    // Container: exosuit-sidebar
    const phaseDetailsProvider = createPhaseDetailsProvider();
    context.subscriptions.push(phaseDetailsProvider);
    context.subscriptions.push(
      vscode.window.registerTreeDataProvider(
        "exosuit.phaseDetails",
        phaseDetailsProvider,
      ),
    );
    logger.warn("Exosuit: Registered phaseDetails provider");
    trace("activation: registered phaseDetails tree provider");

    // Register FileDecoration-based tree item styling (RFC 10169)
    const treeDecorationProvider = new TreeDecorationProvider();
    context.subscriptions.push(treeDecorationProvider);
    context.subscriptions.push(
      vscode.window.registerFileDecorationProvider(treeDecorationProvider),
    );
    logger.debug("Exosuit: Registered TreeDecorationProvider");

    // Container: exosuit-plan
    const projectPlanProvider = new ExosuitTreeProvider("project-plan");
    projectPlanProviderForCommands = projectPlanProvider;
    const projectPlanTreeView = vscode.window.createTreeView(
      "exosuit.projectPlan",
      {
        treeDataProvider: projectPlanProvider,
        // We use our own "Collapse to Current" button instead of the built-in one
        showCollapseAll: false,
      },
    );
    context.subscriptions.push(projectPlanProvider, projectPlanTreeView);

    const clearFocusedPhaseState = (phaseId?: string) => {
      if (phaseId) {
        logger.debug(`[focusPhase] Clearing focused phase state: ${phaseId}`);
      }
      context.workspaceState.update("exosuit.focusedPhaseId", undefined);
      vscode.commands.executeCommand(
        "setContext",
        "exosuit.hasFocusedPhase",
        false,
      );
      getTraceCache?.().updateRootInput("phase-details", undefined);
      projectPlanProvider.setFocus(undefined);
    };

    handleStaleFocusedPhase = () => {
      clearFocusedPhaseState(
        context.workspaceState.get<string>("exosuit.focusedPhaseId"),
      );
      phaseDetailsProvider.refresh();
      projectPlanProvider.refresh();
    };

    // Restore focused phase from workspace state. This updates both the legacy
    // project plan provider and the TraceCache-backed phase-details root.
    const savedFocusId = context.workspaceState.get<string>(
      "exosuit.focusedPhaseId",
    );
    // Do not restore a persisted focused phase on activation. A focused phase is
    // view state, and stale focus IDs make all TraceCache-backed Run views render
    // as "No active phase" even when the project has a real active phase.
    if (savedFocusId) {
      context.workspaceState.update("exosuit.focusedPhaseId", undefined);
    }
    vscode.commands.executeCommand(
      "setContext",
      "exosuit.hasFocusedPhase",
      false,
    );

    // Auto-reveal active phase when sidebar becomes visible
    context.subscriptions.push(
      projectPlanTreeView.onDidChangeVisibility(async (e) => {
        if (e.visible) {
          const activePhaseId = await PlanService.instance.getActivePhaseId();
          if (activePhaseId) {
            const activeItem = await projectPlanProvider.getItem(activePhaseId);
            if (activeItem) {
              // Delay slightly to let the tree render
              setTimeout(async () => {
                try {
                  await projectPlanTreeView.reveal(activeItem, {
                    select: false,
                    focus: false,
                    expand: 2,
                  });
                } catch {
                  // Ignore reveal errors (item may not exist yet)
                }
              }, 100);
            }
          }
        }
      }),
    );

    logger.debug("Exosuit: Registered projectPlan provider");
    trace("activation: registered projectPlan tree provider");

    // Ideas Backlog view
    const {
      provider: ideasTreeProvider,
      connectTreeView: connectIdeasTreeView,
    } = createIdeasTreeProvider();
    const ideasTreeView = vscode.window.createTreeView("exosuit.ideasBacklog", {
      treeDataProvider: ideasTreeProvider,
      showCollapseAll: true,
    });
    connectIdeasTreeView(ideasTreeView);
    context.subscriptions.push(ideasTreeView, ideasTreeProvider);
    logger.debug("Exosuit: Registered ideasBacklog provider");
    trace("activation: registered ideasBacklog tree provider");

    // Sidecar Status view
    const sidecarStatusProvider = createSidecarStatusProvider();
    context.subscriptions.push(sidecarStatusProvider);
    context.subscriptions.push(
      vscode.window.registerTreeDataProvider(
        "exosuit.sidecarStatus",
        sidecarStatusProvider,
      ),
    );
    logger.debug("Exosuit: Registered sidecarStatus provider");
    trace("activation: registered sidecarStatus tree provider");

    context.subscriptions.push(
      vscode.commands.registerCommand(
        "exosuit.sidecar.runAction",
        async (action?: SidecarPaneAction) => {
          if (!workspaceRoot) {
            vscode.window.showErrorMessage("No workspace folder open");
            return;
          }
          if (!action) {
            vscode.window.showErrorMessage("No sidecar action provided");
            return;
          }

          const call = await sidecarActionInput(action);
          if (!call) {
            return;
          }

          try {
            const { exoMachineChannel } =
              await import("./agent/lmtool/machineChannel");
            const response = await exoMachineChannel(workspaceRoot, {
              protocol_version: 1,
              id: `vscode.sidecar.${action.kind}.${Date.now()}`,
              op: {
                kind: "call",
                params: {
                  address: { kind: "operation", path: call.path },
                  input: call.input,
                },
              },
            });

            if (response.status === "ok") {
              vscode.window.showInformationMessage(
                `Sidecar action complete: ${action.label}`,
              );
              MachineChannelServer.notifyWriteAll();
              sidecarStatusProvider.refresh();
              return;
            }

            if (response.status === "confirm_required" && response.ticket) {
              const approved = await vscode.window.showWarningMessage(
                `Approve sidecar action: ${action.label}?`,
                { modal: true },
                "Approve",
              );
              if (approved !== "Approve") {
                return;
              }
              const confirmed = await exoMachineChannel(workspaceRoot, {
                protocol_version: 1,
                id: `vscode.sidecar.${action.kind}.confirmed.${Date.now()}`,
                op: {
                  kind: "call",
                  params: {
                    address: { kind: "operation", path: call.path },
                    input: call.input,
                  },
                },
                auth: { ticket: response.ticket, confirm: true },
              });
              if (confirmed.status === "ok") {
                vscode.window.showInformationMessage(
                  `Sidecar action complete: ${action.label}`,
                );
                MachineChannelServer.notifyWriteAll();
                sidecarStatusProvider.refresh();
                return;
              }
              const message =
                confirmed.error?.message ??
                `Sidecar action failed: ${action.label}`;
              vscode.window.showErrorMessage(message);
              return;
            }

            const message =
              response.error?.message ??
              `Sidecar action failed: ${action.label}`;
            vscode.window.showErrorMessage(message);
          } catch (error) {
            const message =
              error instanceof Error ? error.message : String(error);
            vscode.window.showErrorMessage(`Sidecar action failed: ${message}`);
          }
        },
      ),
    );

    // Register Notebook Controller
    context.subscriptions.push(new ExosuitNotebookController());
    logger.debug("Exosuit: Registered Notebook Controller");
    trace("activation: registered notebook controller");

    // Register Comment Controller
    context.subscriptions.push(new ExosuitCommentController(context));
    logger.debug("Exosuit: Registered Comment Controller");
    trace("activation: registered comment controller");

    // Register Open Source Command
    context.subscriptions.push(
      vscode.commands.registerCommand(
        "exosuit.openSource",
        async (uri?: vscode.Uri) => {
          // When called from the editor title menu, uri is passed.
          let targetUri = uri;

          if (!targetUri) {
            // If called from command palette, try to find the active custom editor
            const tab = vscode.window.tabGroups.activeTabGroup.activeTab;
            if (tab && tab.input instanceof vscode.TabInputCustom) {
              targetUri = tab.input.uri;
            }
          }

          if (targetUri) {
            await vscode.commands.executeCommand(
              "vscode.openWith",
              targetUri,
              "default",
            );
          } else {
            vscode.window.showErrorMessage(
              "No active file to open source for.",
            );
          }
        },
      ),
    );
    logger.debug("Exosuit: Registered openSource command");

    // Clicking a phase in the tree calls this.
    context.subscriptions.push(
      vscode.commands.registerCommand(
        "exosuit.focusPhase",
        async (phaseId?: string) => {
          const resolvedPhaseId =
            phaseId || (await PlanService.instance.getActivePhaseId());
          if (!resolvedPhaseId) {
            vscode.window.showWarningMessage("No active phase found to focus.");
            return;
          }

          logger.debug(`[focusPhase] Setting state to: ${resolvedPhaseId}`);
          context.workspaceState.update(
            "exosuit.focusedPhaseId",
            resolvedPhaseId,
          );
          vscode.commands.executeCommand(
            "setContext",
            "exosuit.hasFocusedPhase",
            true,
          );
          projectPlanProvider.setFocus(resolvedPhaseId);
          getTraceCache?.().updateRootInput("phase-details", {
            id: resolvedPhaseId,
          });
        },
      ),
    );

    // "Focus Current Phase" button in the Project Plan view title.
    context.subscriptions.push(
      vscode.commands.registerCommand("exosuit.resetFocus", async () => {
        const activePhaseId = await PlanService.instance.getActivePhaseId();
        if (!activePhaseId) {
          vscode.window.showWarningMessage("No active phase found to focus.");
          return;
        }

        logger.debug(
          `[resetFocus] Resetting state to null (active: ${activePhaseId})`,
        );
        clearFocusedPhaseState(activePhaseId);
        projectPlanProvider.setFocus(activePhaseId);
      }),
    );

    context.subscriptions.push(
      vscode.commands.registerCommand("exosuit.resetSidebarState", async () => {
        await MachineChannelServer.restartAll();
        const activePhaseId = await PlanService.instance.getActivePhaseId();
        clearFocusedPhaseState(activePhaseId);
        getTraceCache?.().revalidateAll();
        phaseDetailsProvider.refresh();
        epochContextProvider.refresh();
        vscode.window.showInformationMessage(
          "Exosuit sidebar state and runtime connection reset.",
        );
      }),
    );

    // "Collapse to Current" - Collapse all, then reveal only the current phase
    context.subscriptions.push(
      vscode.commands.registerCommand("exosuit.collapseToCurrent", async () => {
        const activePhaseId = await PlanService.instance.getActivePhaseId();
        if (!activePhaseId) {
          vscode.window.showWarningMessage("No active phase found.");
          return;
        }

        // First, collapse all nodes using VS Code's auto-generated command
        await vscode.commands.executeCommand(
          "workbench.actions.treeView.exosuit.projectPlan.collapseAll",
        );

        // Small delay to let the UI finish collapsing before we reveal
        await new Promise((resolve) => setTimeout(resolve, 50));

        // Then reveal and expand to the active phase
        const activeItem = await projectPlanProvider.getItem(activePhaseId);
        if (activeItem) {
          await projectPlanTreeView.reveal(activeItem, {
            select: true,
            focus: false,
            expand: 2, // Expand the item and its immediate children
          });
        }
      }),
    );

    // "Expand All" - Trigger the built-in command for tree expansion
    context.subscriptions.push(
      vscode.commands.registerCommand("exosuit.expandAll", async () => {
        // Refresh the tree to reset to default expanded state
        projectPlanProvider.refresh();
      }),
    );

    // "Phase Status" - Focus the phase details view
    context.subscriptions.push(
      vscode.commands.registerCommand("exosuit.phaseStatus", async () => {
        await vscode.commands.executeCommand("exosuit.phaseDetails.focus");
      }),
    );

    // Open RFC by ID - uses machine channel to find and open the RFC file
    context.subscriptions.push(
      vscode.commands.registerCommand(
        "exosuit.openRfc",
        async (rfcId: string) => {
          if (!workspaceRoot) {
            vscode.window.showErrorMessage("No workspace folder open");
            return;
          }

          try {
            const { exoMachineChannel } =
              await import("./agent/lmtool/machineChannel");
            const response = await exoMachineChannel(workspaceRoot, {
              protocol_version: 1,
              id: `vscode.openRfc.${rfcId}.${Date.now()}`,
              op: {
                kind: "call",
                params: {
                  address: { kind: "operation", path: ["rfc", "show"] },
                  input: { id: rfcId },
                },
              },
            });

            if (response.status === "ok" && response.result) {
              const result = response.result as {
                stage: number;
                filename: string;
              };
              const rfcPath = path.join(
                workspaceRoot,
                "docs",
                "rfcs",
                `stage-${result.stage}`,
                result.filename,
              );

              if (fs.existsSync(rfcPath)) {
                const doc = await vscode.workspace.openTextDocument(rfcPath);
                await vscode.window.showTextDocument(doc);
                return;
              }
            }

            vscode.window.showWarningMessage(`RFC ${rfcId} not found`);
          } catch (err) {
            logger.error(`[openRfc] Failed to open RFC ${rfcId}:`, err);
            vscode.window.showErrorMessage(
              `Failed to open RFC ${rfcId}: ${err instanceof Error ? err.message : String(err)}`,
            );
          }
        },
      ),
    );

    // When the user opens a plan view, auto-reveal the active phase.
    // DISABLED: This conflicts with the Custom Editor (Studio) by forcing the text editor to open.
    // context.subscriptions.push(
    //   vscode.workspace.onDidOpenTextDocument(async (doc) => {
    //     if (!planUri || doc.uri.toString() !== planUri.toString()) {
    //       return;
    //     }

    //     // Avoid re-entrancy loops where revealing opens/reopens the same document.
    //     const key = doc.uri.toString();
    //     if (autoRevealInProgress.has(key)) {
    //       return;
    //     }

    //     autoRevealInProgress.add(key);
    //     try {
    //       const activePhaseId = await PlanService.instance.getActivePhaseId();
    //       if (activePhaseId) {
    //         await revealPhaseInPlanToml(activePhaseId, doc);
    //       }
    //     } finally {
    //       autoRevealInProgress.delete(key);
    //     }
    //   })
    // );

    // 4. Initialize Services
    logger.info("Exosuit: Initializing Services...");
    trace("activation: initializing services");
    editorService.init();
    documentService.init();
    configurationService.init();

    context.subscriptions.push(editorService);
    context.subscriptions.push(documentService);
    context.subscriptions.push(fileSystemService);
    context.subscriptions.push(configurationService);
    context.subscriptions.push(commandService);

    logger.info("Exosuit: Services Initialized");
    trace("activation: services initialized");

    // 4b. Initialize Inbox Service (RFC 0050: Async Intent Channel)
    if (workspaceRoot) {
      const inboxStatusBarService = new InboxStatusBarService(workspaceRoot);
      context.subscriptions.push(inboxStatusBarService);
      logger.info("Exosuit: Inbox Status Bar Initialized");

      // Initialize Phase Status Bar (Sidebar Current State Visibility)
      const phaseStatusBarService = new PhaseStatusBarService();
      phaseStatusBarService.setTreeView(projectPlanTreeView);
      context.subscriptions.push(phaseStatusBarService);
      logger.info("Exosuit: Phase Status Bar Initialized");

      // Initialize Diagnostics Status Bar (RFC 00225: Shared Perception Channel)
      const diagnosticsStatusBarService = new DiagnosticsStatusBarService();
      context.subscriptions.push(diagnosticsStatusBarService);
      logger.info("Exosuit: Diagnostics Status Bar Initialized");

      // "Reveal Active Phase" - Open Project Plan and reveal the active phase
      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.revealActivePhase",
          async () => {
            const activePhaseId = await PlanService.instance.getActivePhaseId();

            // First, focus the Project Plan view
            await vscode.commands.executeCommand("exosuit.projectPlan.focus");

            if (activePhaseId) {
              // Reveal the active phase
              const activeItem =
                await projectPlanProvider.getItem(activePhaseId);
              if (activeItem) {
                await projectPlanTreeView.reveal(activeItem, {
                  select: true,
                  focus: true,
                  expand: 2,
                });
              }
            }
          },
        ),
      );

      // "Initialize Project" - Run exo init for welcome content
      context.subscriptions.push(
        vscode.commands.registerCommand("exosuit.initProject", async () => {
          const terminal = vscode.window.createTerminal({
            name: "Exosuit",
            cwd: workspaceRoot,
          });
          terminal.sendText(exoCommand("init", workspaceRoot));
          terminal.show();
        }),
      );

      // "Start Next Phase" - Run exo phase start for welcome content
      context.subscriptions.push(
        vscode.commands.registerCommand("exosuit.startNextPhase", async () => {
          const terminal = vscode.window.createTerminal({
            name: "Exosuit",
            cwd: workspaceRoot,
          });
          terminal.sendText(exoCommand("phase start", workspaceRoot));
          terminal.show();
          // Refresh the status bar after starting
          phaseStatusBarService.refresh();
        }),
      );

      async function addInboxIntent(
        workspaceRoot: string,
        options: {
          subject: string;
          entityType: string;
          entityId?: string;
          intent?: string;
          priority?: string;
        },
      ): Promise<void> {
        const exec = util.promisify(cp.exec);
        const escapedSubject = options.subject.replace(/"/g, '\\"');
        const intent = options.intent || "claim";
        const priority = options.priority || "immediate";
        const entityIdArg = options.entityId
          ? ` --entity-id "${options.entityId}"`
          : "";

        await exec(
          exoCommand(
            `inbox add "${escapedSubject}" --entity-type "${options.entityType}" ${entityIdArg} --intent "${intent}" --priority "${priority}"`,
            workspaceRoot,
          ),
          { cwd: workspaceRoot },
        );
      }

      type TreeCommandItem =
        | string
        | { id?: string; label?: string | vscode.TreeItemLabel };

      type PlanChoice = {
        id: string;
        title: string;
        parentId?: string;
        parentTitle?: string;
        children?: PlanItem[];
      };

      const treeItemLabel = (item: TreeCommandItem | undefined): string => {
        if (!item || typeof item === "string") {
          return "";
        }
        if (typeof item.label === "string") {
          return item.label;
        }
        return item.label?.label ?? "";
      };

      const refreshAfterReorganizationRequest = () => {
        PlanService.instance.invalidate();
        MachineChannelServer.notifyWriteAll();
        getTraceCache?.().revalidateAll();
        projectPlanProvider.refresh();
        phaseDetailsProvider.refresh();
        epochContextProvider.refresh();
      };

      const queueReorganizationAction = async (
        action: PlanReorganizationAction,
      ) => {
        if (!workspaceRoot) {
          vscode.window.showErrorMessage("No workspace folder open");
          return;
        }

        try {
          await queuePlanReorganizationRequest(workspaceRoot, action);
          vscode.window.showInformationMessage(
            "Reorganization request queued for agent",
          );
          refreshAfterReorganizationRequest();
        } catch (e: unknown) {
          const message = e instanceof Error ? e.message : String(e);
          vscode.window.showErrorMessage(
            `Failed to queue reorganization request: ${message}`,
          );
        }
      };

      const readPlan = async (): Promise<PlanItem[]> =>
        PlanService.instance.getPlan();

      const phaseChoices = async (): Promise<PlanChoice[]> => {
        const epochs = await readPlan();
        return epochs.flatMap((epoch) =>
          (epoch.children ?? []).map((phase) => ({
            id: phase.id,
            title: phase.title,
            parentId: epoch.id,
            parentTitle: epoch.title,
            children: phase.children,
          })),
        );
      };

      const pickPosition = async (
        entityLabel: string,
        movingId: string,
        siblings: PlanChoice[],
      ): Promise<string | undefined> => {
        const siblingItems = siblings
          .filter((item) => item.id !== movingId)
          .flatMap((item) => [
            {
              label: `Before ${item.title}`,
              description: item.id,
              value: `before:${item.id}`,
            },
            {
              label: `After ${item.title}`,
              description: item.id,
              value: `after:${item.id}`,
            },
          ]);

        const picked = await vscode.window.showQuickPick(
          [
            { label: "Top", value: "top" },
            { label: "Bottom", value: "bottom" },
            ...siblingItems,
          ],
          {
            title: `Recommend Move ${entityLabel}`,
            placeHolder: "Choose the requested position",
          },
        );
        return picked?.value;
      };

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.reorg.recommendRenameEpoch",
          async (itemOrId: TreeCommandItem) => {
            const epochId = resolvePlanEntityId(itemOrId);
            if (!epochId) {
              vscode.window.showErrorMessage("Invalid epoch ID");
              return;
            }

            const title = await vscode.window.showInputBox({
              title: "Recommend Rename Epoch",
              prompt: "New epoch title",
              value: treeItemLabel(itemOrId),
            });
            if (!title) {
              return;
            }

            await queueReorganizationAction({
              type: "epoch.update",
              epoch_id: epochId,
              title,
            });
          },
        ),
      );

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.reorg.recommendRenamePhase",
          async (itemOrId: TreeCommandItem) => {
            const phaseId = resolvePlanEntityId(itemOrId);
            if (!phaseId) {
              vscode.window.showErrorMessage("Invalid phase ID");
              return;
            }

            const title = await vscode.window.showInputBox({
              title: "Recommend Rename Phase",
              prompt: "New phase title",
              value: treeItemLabel(itemOrId),
            });
            if (!title) {
              return;
            }

            await queueReorganizationAction({
              type: "phase.update",
              phase_id: phaseId,
              title,
            });
          },
        ),
      );

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.reorg.recommendMoveEpoch",
          async (itemOrId: TreeCommandItem) => {
            const epochId = resolvePlanEntityId(itemOrId);
            if (!epochId) {
              vscode.window.showErrorMessage("Invalid epoch ID");
              return;
            }

            const epochs = (await readPlan()).map((epoch) => ({
              id: epoch.id,
              title: epoch.title,
            }));
            const position = await pickPosition("Epoch", epochId, epochs);
            if (!position) {
              return;
            }

            await queueReorganizationAction({
              type: "epoch.reorder",
              epoch_id: epochId,
              position,
            });
          },
        ),
      );

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.reorg.recommendMovePhase",
          async (itemOrId: TreeCommandItem) => {
            const phaseId = resolvePlanEntityId(itemOrId);
            if (!phaseId) {
              vscode.window.showErrorMessage("Invalid phase ID");
              return;
            }

            const epochs = await readPlan();
            const target = await vscode.window.showQuickPick(
              epochs.map((epoch) => ({
                label: epoch.title,
                description: epoch.id,
                epoch,
              })),
              {
                title: "Recommend Move Phase",
                placeHolder: "Choose the target epoch",
              },
            );
            if (!target) {
              return;
            }

            const siblings = (target.epoch.children ?? []).map((phase) => ({
              id: phase.id,
              title: phase.title,
            }));
            const position = await pickPosition("Phase", phaseId, siblings);
            if (!position) {
              return;
            }

            await queueReorganizationAction({
              type: "phase.move",
              phase_id: phaseId,
              epoch_id: target.epoch.id,
              position,
            });
          },
        ),
      );

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.reorg.recommendMoveGoal",
          async (itemOrId: TreeCommandItem) => {
            const goalId = resolvePlanEntityId(itemOrId);
            if (!goalId) {
              vscode.window.showErrorMessage("Invalid goal ID");
              return;
            }

            const phases = await phaseChoices();
            const target = await vscode.window.showQuickPick(
              phases.map((phase) => ({
                label: phase.title,
                description: phase.parentTitle
                  ? `${phase.id} in ${phase.parentTitle}`
                  : phase.id,
                phase,
              })),
              {
                title: "Recommend Move Goal",
                placeHolder: "Choose the target phase",
              },
            );
            if (!target) {
              return;
            }

            const siblings = (target.phase.children ?? []).map((goal) => ({
              id: goal.id,
              title: goal.title,
            }));
            const position = await pickPosition("Goal", goalId, siblings);
            if (!position) {
              return;
            }

            await queueReorganizationAction({
              type: "goal.move",
              goal_id: goalId,
              phase_id: target.phase.id,
              position,
            });
          },
        ),
      );

      // Register Add Feedback Command (goals + tasks)
      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.addFeedback",
          async (itemOrId: string | { id?: string }) => {
            let itemId: string;
            if (typeof itemOrId === "string") {
              itemId = itemOrId;
            } else if (itemOrId && itemOrId.id) {
              itemId = itemOrId.id;
            } else {
              vscode.window.showErrorMessage("Invalid item ID");
              return;
            }

            // Determine item type and clean ID
            const isGoal = itemId.startsWith("goal:");
            const cleanId = itemId.replace(/^(goal|exec|task):/, "");
            const subjectRef = isGoal ? `goal:${cleanId}` : `task:${cleanId}`;

            const note = await vscode.window.showInputBox({
              title: "Add Note",
              prompt: "Enter a note for this item",
              placeHolder: "e.g. Discovered edge case, needs follow-up",
            });

            if (note) {
              if (!workspaceRoot) {
                vscode.window.showErrorMessage("No workspace folder open");
                return;
              }

              try {
                const [entityType, entityId] = subjectRef.split(":");
                await addInboxIntent(workspaceRoot, {
                  subject: note,
                  entityType,
                  entityId,
                  intent: "fyi",
                  priority: "immediate",
                });
                vscode.window.showInformationMessage("Note queued for agent");

                // Refresh views
                PlanService.instance.invalidate();
                phaseDetailsProvider.refresh();
              } catch (e: unknown) {
                const message = e instanceof Error ? e.message : String(e);
                vscode.window.showErrorMessage(
                  `Failed to add note: ${message}`,
                );
              }
            }
          },
        ),
      );

      // Register "Mark Outcome Ready" Command (goals + tasks)
      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.claimDone",
          async (itemOrId: string | { id?: string }) => {
            let itemId: string;
            if (typeof itemOrId === "string") {
              itemId = itemOrId;
            } else if (itemOrId && itemOrId.id) {
              itemId = itemOrId.id;
            } else {
              vscode.window.showErrorMessage("Invalid item ID");
              return;
            }

            // Determine entity type and clean ID
            const isGoal = itemId.startsWith("goal:");
            const cleanId = itemId.replace(/^(goal|exec|task):/, "");
            const entityType = isGoal ? "goal" : "task";

            const log = await vscode.window.showInputBox({
              title: "Mark Outcome Ready",
              prompt: "What outcome should be reviewed? (optional)",
              placeHolder: "e.g. Tests passing and flow verified",
            });

            // User cancelled the input
            if (log === undefined) {
              return;
            }

            if (!workspaceRoot) {
              vscode.window.showErrorMessage("No workspace folder open");
              return;
            }

            try {
              await addInboxIntent(workspaceRoot, {
                subject: log || "Outcome ready for review",
                entityType,
                entityId: cleanId,
                intent: "claim",
                priority: "immediate",
              });
              vscode.window.showInformationMessage("Outcome queued for review");

              // Refresh views
              PlanService.instance.invalidate();
              phaseDetailsProvider.refresh();
            } catch (e: unknown) {
              const message = e instanceof Error ? e.message : String(e);
              vscode.window.showErrorMessage(
                `Failed to queue outcome review: ${message}`,
              );
            }
          },
        ),
      );

      // Register "Outcome Looks Right" Command (approve the reviewed outcome)
      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.acknowledgeClaim",
          async (itemOrId: string | { id?: string }) => {
            let itemId: string;
            if (typeof itemOrId === "string") {
              itemId = itemOrId;
            } else if (itemOrId && itemOrId.id) {
              itemId = itemOrId.id;
            } else {
              vscode.window.showErrorMessage("Invalid item ID");
              return;
            }

            // Determine entity type and clean ID
            const isGoal = itemId.startsWith("goal:");
            const cleanId = itemId.replace(/^(goal|exec|task):/, "");
            const entityType = isGoal ? "goal" : "task";

            if (!workspaceRoot) {
              vscode.window.showErrorMessage("No workspace folder open");
              return;
            }

            try {
              // Create human outcome approval — this satisfies the guard.
              await addInboxIntent(workspaceRoot, {
                subject: "Outcome looks right",
                entityType,
                entityId: cleanId,
                intent: "claim",
                priority: "immediate",
              });
              vscode.window.showInformationMessage(
                "Outcome approved — agent can record it",
              );

              // Refresh views
              PlanService.instance.invalidate();
              phaseDetailsProvider.refresh();
            } catch (e: unknown) {
              const message = e instanceof Error ? e.message : String(e);
              vscode.window.showErrorMessage(
                `Failed to approve outcome: ${message}`,
              );
            }
          },
        ),
      );

      // Register "Not Sure About This" Command (completed goals)
      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.notSure",
          async (itemOrId: string | { id?: string }) => {
            let itemId: string;
            if (typeof itemOrId === "string") {
              itemId = itemOrId;
            } else if (itemOrId && itemOrId.id) {
              itemId = itemOrId.id;
            } else {
              vscode.window.showErrorMessage("Invalid item ID");
              return;
            }

            const isGoal = itemId.startsWith("goal:");
            const cleanId = itemId.replace(/^(goal|exec|task):/, "");
            const entityType = isGoal ? "goal" : "task";

            if (!workspaceRoot) {
              vscode.window.showErrorMessage("No workspace folder open");
              return;
            }

            try {
              await addInboxIntent(workspaceRoot, {
                subject: "Not sure this is complete",
                entityType,
                entityId: cleanId,
                intent: "concern",
                priority: "immediate",
              });
              vscode.window.showInformationMessage(
                "Concern queued — agent will re-examine",
              );

              PlanService.instance.invalidate();
              phaseDetailsProvider.refresh();
            } catch (e: unknown) {
              const message = e instanceof Error ? e.message : String(e);
              vscode.window.showErrorMessage(
                `Failed to add concern: ${message}`,
              );
            }
          },
        ),
      );

      const resolveInboxItemId = (
        itemOrId: string | { id?: string; contextValue?: string } | undefined,
      ): string | null => {
        if (!itemOrId) {
          return null;
        }
        const raw = typeof itemOrId === "string" ? itemOrId : itemOrId.id;
        if (!raw) {
          return null;
        }
        if (raw.startsWith("inbox-item-")) {
          return raw.replace(/^inbox-item-/, "");
        }
        if (raw.startsWith("intent-")) {
          return raw;
        }
        return null;
      };

      // Register inbox item actions (RFC 0124: Inbox System)
      // NOTE: These actions should create new intents targeting the original item,
      // not directly mutate state. See RFC 0124 section "Actions on Inbox Items".
      // For now, show informative messages until the CLI supports intent-based actions.

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.inboxPromoteToGoal",
          async (
            _itemOrId: string | { id?: string; contextValue?: string },
          ) => {
            // Per RFC 0124: This should create an intent with action.type = "promote-to-goal"
            // targeting the original inbox item, so the agent can process it.
            vscode.window.showInformationMessage(
              "Promote to Goal: Not yet implemented. Per RFC 0124, this should create an intent for the agent to process.",
            );
          },
        ),
      );

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.inboxConvertToIdea",
          async (
            _itemOrId: string | { id?: string; contextValue?: string },
          ) => {
            // Per RFC 0124: This should create an intent with action.type = "convert-to-idea"
            // targeting the original inbox item, so the agent can process it.
            vscode.window.showInformationMessage(
              "Convert to Idea: Not yet implemented. Per RFC 0124, this should create an intent for the agent to process.",
            );
          },
        ),
      );

      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.inboxDismiss",
          async (itemOrId: string | { id?: string; contextValue?: string }) => {
            // Per RFC 0124: This should create an intent with action.type = "dismiss"
            // targeting the original inbox item. For now, we archive directly as a
            // temporary measure, but this bypasses agent awareness.
            const inboxId = resolveInboxItemId(itemOrId);
            if (!inboxId) {
              vscode.window.showErrorMessage("Invalid inbox item ID");
              return;
            }

            if (!workspaceRoot) {
              vscode.window.showErrorMessage("No workspace folder open");
              return;
            }

            try {
              const execPromise = util.promisify(cp.exec);
              await execPromise(
                exoCommand(`inbox archive ${inboxId}`, workspaceRoot),
                {
                  cwd: workspaceRoot,
                },
              );
              vscode.window.showInformationMessage(
                "Inbox item archived (note: agent not notified - see RFC 0124)",
              );
              PlanService.instance.invalidate();
              phaseDetailsProvider.refresh();
            } catch (e: unknown) {
              const message = e instanceof Error ? e.message : String(e);
              vscode.window.showErrorMessage(
                `Failed to dismiss inbox item: ${message}`,
              );
            }
          },
        ),
      );

      // Register captureIntent command
      context.subscriptions.push(
        vscode.commands.registerCommand("exosuit.captureIntent", async () => {
          const editor = vscode.window.activeTextEditor;
          const selectedText = editor?.document.getText(editor.selection);

          // 1. Subject input (pre-fill with selected text)
          const subject = await vscode.window.showInputBox({
            title: "Capture Intent - Subject",
            prompt: "Brief subject line for this intent",
            value: selectedText ? selectedText.substring(0, 50).trim() : "",
            placeHolder: "e.g., Use snake_case for Rust identifiers",
          });

          if (!subject) {
            return;
          }

          // 2. Intent selection
          const intent = await vscode.window.showQuickPick(
            [
              {
                label: "FYI",
                value: "fyi",
                description: "Just be aware of this",
              },
              {
                label: "Concern",
                value: "concern",
                description: "Something worries me",
              },
              {
                label: "Inquiry",
                value: "inquiry",
                description: "Need a status update",
              },
              {
                label: "Claim",
                value: "claim",
                description: "I believe something about this entity",
              },
            ],
            {
              title: "Capture Intent - Intent",
              placeHolder: "What are you communicating?",
            },
          );

          if (!intent) {
            return;
          }

          // 3. Priority selection
          const priority = await vscode.window.showQuickPick(
            [
              {
                label: "Next Touch",
                value: "next-touch",
                description: "Surface on next interaction with this entity",
              },
              {
                label: "When Relevant",
                value: "when-relevant",
                description: "Surface when contextually appropriate",
              },
              {
                label: "Immediate",
                value: "immediate",
                description: "Surface in next steering response",
              },
            ],
            {
              title: "Capture Intent - Priority",
              placeHolder: "When should this be surfaced?",
            },
          );

          if (!priority) {
            return;
          }

          // 4. Optional body text
          const body = await vscode.window.showInputBox({
            title: "Capture Intent - Details (Optional)",
            prompt: "Additional context or explanation",
            placeHolder: "Optional detailed description...",
          });

          // Execute via CLI
          try {
            const execPromise = util.promisify(cp.exec);
            const escapedSubject = subject.replace(/"/g, '\\"');
            const escapedBody = (body || "").replace(/"/g, '\\"');
            await execPromise(
              exoCommand(
                `inbox add --intent "${intent.value}" --priority "${priority.value}" --subject "${escapedSubject}" --body "${escapedBody}"`,
                workspaceRoot,
              ),
              { cwd: workspaceRoot },
            );
            vscode.window.showInformationMessage(`Intent captured: ${subject}`);
          } catch (e: any) {
            vscode.window.showErrorMessage(
              `Failed to capture intent: ${e.message}`,
            );
          }
        }),
      );
      logger.debug("Exosuit: Registered captureIntent command");

      // Register openInboxQuickPick command
      context.subscriptions.push(
        vscode.commands.registerCommand(
          "exosuit.openInboxQuickPick",
          async () => {
            vscode.window.showInformationMessage(
              "Inbox is now managed through SQLite. Use 'exo inbox list' in the terminal.",
            );
          },
        ),
      );
      logger.debug("Exosuit: Registered openInboxQuickPick command");
    }

    if (workspaceRoot) {
      const exohookTestController = new ExohookTestController(workspaceRoot);
      context.subscriptions.push(exohookTestController);
    }

    // Log activation to Activity Log
    LogService.instance.logActivity({
      type: "system",
      label: "Exosuit Activated",
      details: "All services and providers have been registered successfully.",
      icon: "rocket",
    });
    trace("activation: logged Exosuit Activated");

    logger.debug("Exosuit: Debug commands already registered (early)");

    // Register Test Root Command (for E2E tests)
    // Write success marker for Playwright E2E tests (Holodeck)
    if (activationMarkersRoot) {
      trace("activation: before writing activation-success marker");
      try {
        fs.writeFileSync(
          path.join(activationMarkersRoot, "activation-success.txt"),
          "Activation Complete",
          "utf8",
        );
      } catch {
        // ignore
      }
      trace("activation: wrote activation-success marker");
    }

    return {};
  } catch (e: any) {
    logger.error("Exosuit Activation Failed:", e);
    logger.error(`Exosuit Activation Failed: ${e.message}`);
    vscode.window.showErrorMessage(`Exosuit Activation Failed: ${e.message}`);

    // Write error marker for Playwright E2E tests (Holodeck)
    if (activationMarkersRoot) {
      try {
        fs.writeFileSync(
          path.join(activationMarkersRoot, "activation-error.txt"),
          e.stack || e.message,
          "utf8",
        );
      } catch {
        // ignore
      }
    }

    // Return undefined on error (TypeScript requires all paths return)
    return undefined;
  }
}

export function deactivate() {
  // Dispose singleton services that have cleanup requirements
  PromptService.instance.dispose();

  // Dispose Machine Channel servers (RFC 0097)
  MachineChannelServer.disposeAll();
}
// test
