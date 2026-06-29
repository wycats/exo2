import * as vscode from "vscode";
import type { LogService } from "../LogService";
import type { ToolRegistry } from "./ToolRegistry";
import type { WorkspaceCache } from "../WorkspaceCache";
import { LiterateInterceptor } from "./LiterateInterceptor";
import { createToolResponseMessages } from "./ChatUtils";

export interface AgentConfig {
  toolRegistry: ToolRegistry;
  workspaceCache: WorkspaceCache;
  logger: LogService;
}

export interface RequestOptions {
  systemPrompt: string;
  model?: vscode.LanguageModelChat;
}

export class AgentRuntime {
  private static readonly MAX_AGENT_TURNS = 5;

  constructor(private config: AgentConfig) {}

  async handleRequest(
    request: vscode.ChatRequest,
    context: vscode.ChatContext,
    stream: vscode.ChatResponseStream,
    token: vscode.CancellationToken,
    options: RequestOptions
  ): Promise<vscode.ChatResult> {
    const { toolRegistry, workspaceCache, logger } = this.config;
    const { systemPrompt, model: modelOverride } = options;

    logger.logActivity({
      type: "system",
      label: "Processing Request",
      details: request.prompt,
      items: [
        {
          label: `${request.prompt.length} chars`,
          description: "Prompt Length",
          icon: "symbol-string",
        },
        {
          label: request.model ? request.model.name : "Auto",
          description: "User Model",
          icon: "server",
        },
      ],
      icon: "hubot",
    });

    const messages: vscode.LanguageModelChatMessage[] = [];

    // 1. Add System Prompt
    messages.push(vscode.LanguageModelChatMessage.User(systemPrompt));

    // 2. Add History
    messages.push(...this.reconstructHistory(context.history));

    // 3. Add Current Prompt
    messages.push(vscode.LanguageModelChatMessage.User(request.prompt));

    const model = modelOverride || request.model;
    let currentTurn = 0;
    let toolUsedInLastTurn = false;
    const allToolOutputs: { name: string; output: string }[] = [];

    try {
      while (currentTurn < AgentRuntime.MAX_AGENT_TURNS) {
        currentTurn++;
        toolUsedInLastTurn = false;

        const chatRequest = await model.sendRequest(messages, {}, token);
        const interceptor = new LiterateInterceptor(
          stream,
          workspaceCache,
          async (name, args) => {
            const tool = toolRegistry.get(name);
            if (tool) {
              return await tool.execute(args, stream);
            } else {
              stream.markdown(`\n> **Error**: Tool '${name}' not found.\n`);
              return undefined;
            }
          }
        );

        let fullResponseText = "";
        for await (const chunk of chatRequest.text) {
          fullResponseText += chunk;
          interceptor.feed(chunk);
        }

        // Ensure any buffered content or pending tools are processed
        interceptor.close();

        // Wait for any pending tools to finish before closing the stream
        await interceptor.waitForTools();

        // Add Assistant's response to history
        messages.push(
          vscode.LanguageModelChatMessage.Assistant(fullResponseText)
        );

        const toolOutputs = interceptor.getToolOutputs();
        if (toolOutputs.length > 0) {
          toolUsedInLastTurn = true;
          allToolOutputs.push(...toolOutputs);

          const responseMessages = createToolResponseMessages(toolOutputs);
          messages.push(...responseMessages);

          // Continue loop to let model react
        } else {
          // No tools used, we are done
          break;
        }
      }

      if (currentTurn >= AgentRuntime.MAX_AGENT_TURNS && toolUsedInLastTurn) {
        // Graceful Shutdown
        stream.progress("Summarizing progress...");
        messages.push(
          vscode.LanguageModelChatMessage.User(
            "System: Maximum iteration limit reached. Please summarize your progress and what remains to be done. Do not call any more tools.",
            "System"
          )
        );

        const finalRequest = await model.sendRequest(messages, {}, token);
        for await (const chunk of finalRequest.text) {
          stream.markdown(chunk);
        }
      }

      return { metadata: { command: "", toolOutputs: allToolOutputs } };
    } catch (err) {
      stream.markdown(
        `\n> **Error**: ${err instanceof Error ? err.message : String(err)}\n`
      );
      return { metadata: { command: "" } };
    }
  }

  private reconstructHistory(
    history: ReadonlyArray<vscode.ChatRequestTurn | vscode.ChatResponseTurn>
  ): vscode.LanguageModelChatMessage[] {
    const messages: vscode.LanguageModelChatMessage[] = [];

    for (const turn of history) {
      if (turn instanceof vscode.ChatRequestTurn) {
        messages.push(vscode.LanguageModelChatMessage.User(turn.prompt));
      } else if (turn instanceof vscode.ChatResponseTurn) {
        const parts: string[] = [];
        const metadata = turn.result.metadata as
          | { toolOutputs?: { name: string; output: string }[] }
          | undefined;

        for (const part of turn.response) {
          if (part instanceof vscode.ChatResponseMarkdownPart) {
            let text = part.value.value;
            // Restore hidden tool outputs for the model (Legacy Method)
            const toolOutputRegex = /<!-- EXO_TOOL_OUTPUT\n([\s\S]*?)\n-->/g;
            text = text.replace(toolOutputRegex, (_match, content) => {
              const unescaped = content
                .replace(/&#45;&#45;/g, "--")
                .replace(/&gt;/g, ">");
              return `\n[System: Tool Output]\n${unescaped}\n`;
            });
            parts.push(text);
          } else if (part instanceof vscode.ChatResponseFileTreePart) {
            parts.push(
              `\n[System: User viewed file tree: ${JSON.stringify(
                part.value,
                null,
                2
              )}]\n`
            );
          }
        }
        const fullText = parts.join("");
        messages.push(vscode.LanguageModelChatMessage.Assistant(fullText));

        // Inject Tool Outputs from Metadata (New Method)
        if (metadata?.toolOutputs) {
          const responseMessages = createToolResponseMessages(
            metadata.toolOutputs
          );
          messages.push(...responseMessages);
        }
      }
    }
    return messages;
  }
}
