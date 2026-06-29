import * as vscode from "vscode";
import { LogService } from "../LogService";
import {
  formatProjectionSection,
  stableJsonStringify,
  truncateWithNotice,
} from "@exosuit/core";
import { PromptService } from "../PromptService";
import { ToolRegistry } from "./ToolRegistry";
import { WorkspaceCache } from "../WorkspaceCache";
import { AgentRuntime } from "./AgentRuntime";
import { getLogger } from "../logging";
import { exoMachineChannel } from "./lmtool/machineChannel";
import { currentWorkspaceRoot } from "../workspaceRoot";

const logger = getLogger("extension");

export function registerExosuitChatParticipant(
  context: vscode.ExtensionContext,
) {
  const workspaceCache = new WorkspaceCache();
  context.subscriptions.push(workspaceCache);

  const handler: vscode.ChatRequestHandler = async (
    request: vscode.ChatRequest,
    chatContext: vscode.ChatContext,
    stream: vscode.ChatResponseStream,
    token: vscode.CancellationToken,
  ) => {
    stream.progress("Reading project context...");

    const rootPath = currentWorkspaceRoot();

    const toolRegistry = new ToolRegistry(rootPath);
    const toolDefinitions = toolRegistry.getToolDefinitions();

    const projectContext = await getProjectContext();

    const systemPrompt = PromptService.instance.render("system.chat", {
      toolDefinitions,
      projectContext,
    });

    const runtime = new AgentRuntime({
      toolRegistry,
      workspaceCache,
      logger: LogService.instance,
    });

    return await runtime.handleRequest(request, chatContext, stream, token, {
      systemPrompt,
    });
  };

  const participant = vscode.chat.createChatParticipant(
    "exosuit.chat",
    handler,
  );
  participant.iconPath = new vscode.ThemeIcon("hubot");
  context.subscriptions.push(participant);
}

async function getProjectContext(): Promise<string> {
  LogService.instance.logActivity({
    type: "context",
    label: "Building Context",
    icon: "layers",
  });

  const rootPath = currentWorkspaceRoot();
  if (!rootPath) {
    return "";
  }

  // Budgets are character-based (not tokens) and intentionally conservative.
  // They can be revisited once we have a first-class token budgeting story.
  const MAX_AGENT_INSTRUCTIONS_CHARS = 40_000;
  const MAX_PLAN_JSON_CHARS = 40_000;
  const MAX_PROJECT_CONTEXT_CHARS = 90_000;

  let planContent = "";
  try {
    const response = await exoMachineChannel(rootPath, {
      protocol_version: 1,
      id: `vscode.chat.plan.read.${Date.now()}`,
      op: {
        kind: "call",
        params: {
          address: { kind: "operation", path: ["plan", "read"] },
          input: {},
        },
      },
    });
    if (response.status === "ok" && response.result) {
      planContent = stableJsonStringify(response.result, 2);
      LogService.instance.logActivity({
        type: "context",
        label: "Read Project Plan (daemon)",
        icon: "file-text",
      });
    }
  } catch (e) {
    logger.error("Failed to read plan via daemon", e);
  }

  const agentsContext = await getAgentsContext();

  const agentInstructionsRaw = agentsContext
    ? agentsContext
    : PromptService.instance.get("chat.defaultGuardrails");

  const agentInstructions = truncateWithNotice(
    agentInstructionsRaw,
    MAX_AGENT_INSTRUCTIONS_CHARS,
    {
      notice: ({ originalLength, maxChars }) =>
        `\n\n[TRUNCATED Agent Instructions: ${originalLength} → ${maxChars} chars]`,
    },
  ).text;

  const planJson = planContent
    ? truncateWithNotice(planContent, MAX_PLAN_JSON_CHARS, {
        notice: ({ originalLength, maxChars }) =>
          `\n\n[TRUNCATED Project Plan: ${originalLength} → ${maxChars} chars]`,
      }).text
    : "";

  const sections: string[] = [];
  sections.push(
    formatProjectionSection({
      title: "Agent Instructions (docs/agent-context/EXOSUIT.md)",
      format: "text",
      content: agentInstructions,
    }),
  );

  if (planJson) {
    sections.push(
      formatProjectionSection({
        title: "Project Plan (SQLite state projected as JSON)",
        format: "json",
        content: planJson,
      }),
    );
  }

  const full = sections.join("\n");
  return truncateWithNotice(full, MAX_PROJECT_CONTEXT_CHARS, {
    notice: ({ originalLength, maxChars }) =>
      `\n\n[TRUNCATED Project Context: ${originalLength} → ${maxChars} chars]`,
  }).text;
}

async function getAgentsContext(): Promise<string> {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders) {
    return "";
  }

  const agentsUri = vscode.Uri.joinPath(
    workspaceFolders[0].uri,
    "docs/agent-context/EXOSUIT.md",
  );
  try {
    const document = await vscode.workspace.openTextDocument(agentsUri);
    LogService.instance.logActivity({
      type: "context",
      label: "Loaded Persona",
      items: [
        {
          label: "EXOSUIT.md",
          icon: "file-text",
          file: "docs/agent-context/EXOSUIT.md",
        },
      ],
      icon: "person",
    });
    return document.getText();
  } catch (e) {
    logger.warn("EXOSUIT.md not found, using default guardrails.");
    return "";
  }
}
