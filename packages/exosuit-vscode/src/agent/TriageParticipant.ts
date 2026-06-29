import * as vscode from "vscode";
import { AgentRuntime } from "./AgentRuntime";
import { ToolRegistry } from "./ToolRegistry";
import { WorkspaceCache } from "../WorkspaceCache";
import { LogService } from "../LogService";
import { exoMachineChannel } from "./lmtool/machineChannel";
import { currentWorkspaceRoot } from "../workspaceRoot";

export function registerTriageParticipant(context: vscode.ExtensionContext) {
  const workspaceCache = new WorkspaceCache();
  context.subscriptions.push(workspaceCache);

  const handler: vscode.ChatRequestHandler = async (
    request: vscode.ChatRequest,
    chatContext: vscode.ChatContext,
    stream: vscode.ChatResponseStream,
    token: vscode.CancellationToken,
  ) => {
    const rootPath = currentWorkspaceRoot();
    if (!rootPath) {
      stream.markdown("No workspace open.");
      return;
    }
    const toolRegistry = new ToolRegistry(rootPath);

    // Register Idea Tools
    toolRegistry.register({
      name: "idea.list",
      description:
        "List ideas. Args: { status?: 'new' | 'accepted' | 'rejected' | 'deferred' }",
      execute: async (args: { status?: string }, stream) => {
        stream.progress("Listing ideas...");
        try {
          const response = await exoMachineChannel(rootPath, {
            protocol_version: 1,
            id: `triage.idea.list.${Date.now()}`,
            op: {
              kind: "list",
              params: {
                address: { kind: "namespace", path: ["idea"] },
                kind: "ideas",
                page: { cursor: null, limit: 50 },
              },
            },
          });
          if (response.status === "ok" && response.result) {
            const items = (response.result as any).items ?? [];
            const filtered = args.status
              ? items.filter((i: any) => i.status === args.status)
              : items;
            return JSON.stringify(filtered, null, 2);
          }
          return "Failed to list ideas via daemon.";
        } catch (e) {
          return `Error listing ideas: ${e}`;
        }
      },
    });

    toolRegistry.register({
      name: "idea.get",
      description: "Get an idea by ID. Args: { id: string }",
      execute: async (args: { id: string }, stream) => {
        stream.progress(`Fetching idea ${args.id}...`);
        try {
          const response = await exoMachineChannel(rootPath, {
            protocol_version: 1,
            id: `triage.idea.get.${Date.now()}`,
            op: {
              kind: "call",
              params: {
                address: { kind: "operation", path: ["idea", "get"] },
                input: { id: args.id },
              },
            },
          });
          if (response.status === "ok" && response.result) {
            return JSON.stringify(response.result, null, 2);
          }
          return "Idea not found";
        } catch (e) {
          return `Error fetching idea: ${e}`;
        }
      },
    });

    toolRegistry.register({
      name: "idea.update",
      description:
        "Update an idea. Args: { id: string, status?: string, title?: string, description?: string }",
      execute: async (
        args: {
          id: string;
          status?: string;
          title?: string;
          description?: string;
        },
        stream,
      ) => {
        stream.progress(`Updating idea ${args.id}...`);
        try {
          const response = await exoMachineChannel(rootPath, {
            protocol_version: 1,
            id: `triage.idea.update.${Date.now()}`,
            op: {
              kind: "call",
              params: {
                address: { kind: "operation", path: ["idea", "update"] },
                input: {
                  id: args.id,
                  ...(args.status && { status: args.status }),
                  ...(args.title && { title: args.title }),
                  ...(args.description && { description: args.description }),
                },
              },
            },
          });
          if (response.status === "ok") {
            return `Idea ${args.id} updated successfully.`;
          }
          return `Failed to update idea: ${JSON.stringify(response)}`;
        } catch (e) {
          return `Error updating idea: ${e}`;
        }
      },
    });

    const toolDefinitions = toolRegistry.getToolDefinitions();

    const systemPrompt = `<system_instructions>
  <meta>
    <role>Triage Agent</role>
    <objective>Manage and Triage Project Ideas</objective>
  </meta>

  <philosophy>
    You are the Triage Agent. Your job is to help the user manage ideas in SQLite.
    You can list, read, and update ideas.
    When the user asks to "triage", you should list "new" ideas and ask for a decision.
  </philosophy>

  <constraints>
    1. Use \`<exo-tool>\` to perform actions.
    2. Be concise.
  </constraints>

  <available_tools>
${toolDefinitions}
  </available_tools>
</system_instructions>`;

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
    "exosuit.triage",
    handler,
  );
  participant.iconPath = new vscode.ThemeIcon("checklist");
  context.subscriptions.push(participant);
}
