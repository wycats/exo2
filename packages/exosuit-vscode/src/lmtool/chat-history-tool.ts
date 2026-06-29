import * as vscode from "vscode";
import { randomUUID } from "node:crypto";
import { exoMachineChannel } from "../agent/lmtool/machineChannel";
import {
  MACHINE_CHANNEL_PROTOCOL_VERSION,
  type MachineChannelRequestEnvelope,
} from "../types/machineChannel";
import {
  isAmbiguousResponse,
  isNoSummaryResponse,
  type AiChatHistoryOutput,
} from "../types/chatHistory";
import { selectCurrentWorkspaceRoot } from "../workspaceRoot";

/**
 * Input schema for the exo-ai-chat-history LM tool.
 *
 * Property names use kebab-case to match the package.json schema declaration
 * and the tool's public API surface.
 */
interface ChatHistoryToolInput {
  /** Number of recent turns (user + assistant pairs) to retrieve. Default: 10 */
  turns?: number;
  /** Whether to include extended thinking content. Default: false */
  "include-thinking"?: boolean;
  /** Whether to include tool invocations. Default: false */
  "include-tools"?: boolean;
  /** A snippet of text from a recent user message to identify the session. */
  "match-text"?: string;
  /** Exact workspace URI to match (e.g., file:///path/to/workspace) */
  "workspace-uri"?: string;
  /** Get turns before the last summarization (context that was just compacted). */
  "before-summary"?: boolean;
}

/**
 * Creates the exo-chat-history LM tool that reads recent conversation history.
 *
 * This tool helps agents recover context that may have been lost during
 * conversation summarization, particularly nuanced user feedback and decisions.
 *
 * Routes through `exo ai chat-history` CLI via machine channel for unified contract.
 */
export function createChatHistoryTool(): vscode.LanguageModelTool<ChatHistoryToolInput> {
  return {
    async invoke(
      options: vscode.LanguageModelToolInvocationOptions<ChatHistoryToolInput>,
      _token: vscode.CancellationToken,
    ): Promise<vscode.LanguageModelToolResult> {
      const input = options.input ?? {};

      const workspaceSelection = selectCurrentWorkspaceRoot();
      const workspacePath = workspaceSelection.rootPath;
      if (!workspacePath) {
        return new vscode.LanguageModelToolResult([
          new vscode.LanguageModelTextPart(
            JSON.stringify({
              error: `No usable Exosuit workspace root: ${workspaceSelection.reason}`,
              candidates: workspaceSelection.candidates,
            }),
          ),
        ]);
      }

      const currentWorkspaceFolder = vscode.workspace.workspaceFolders?.find(
        (folder) => folder.uri.fsPath === workspacePath,
      );
      const currentWorkspaceUri =
        currentWorkspaceFolder?.uri.toString() ??
        vscode.Uri.file(workspacePath).toString();

      // Build CLI args object (kebab-case keys match CLI flags)
      const cliArgs: Record<string, unknown> = {};

      if (input.turns !== undefined) {
        cliArgs.turns = Math.min(input.turns, 50);
      }
      // Always filter by workspace URI — use provided value or default to current workspace.
      // This ensures we get the most recent session for THIS workspace (likely the current chat)
      // rather than the most recent session across all workspaces.
      cliArgs["workspace-uri"] = input["workspace-uri"] ?? currentWorkspaceUri;

      if (input["match-text"]) {
        cliArgs["match-text"] = input["match-text"];
      }
      if (input["include-thinking"]) {
        cliArgs["include-thinking"] = true;
      }
      if (input["include-tools"]) {
        cliArgs["include-tools"] = true;
      }
      if (input["before-summary"]) {
        cliArgs["before-summary"] = true;
      }

      // Build machine channel request for `exo ai chat-history`
      const request: MachineChannelRequestEnvelope = {
        protocol_version: MACHINE_CHANNEL_PROTOCOL_VERSION,
        id: `chat-history-${randomUUID()}`,
        op: {
          kind: "call",
          params: {
            address: {
              kind: "operation",
              path: ["ai", "chat-history"],
            },
            input: cliArgs,
          },
        },
      };

      try {
        const response = await exoMachineChannel(workspacePath, request);

        if (response.status === "ok" && response.result) {
          // Extract the data field from CLI response (AiChatHistoryOutput)
          const result = response.result as AiChatHistoryOutput;
          if (result.ok && result.data) {
            // Check for ambiguous session selection
            if (isAmbiguousResponse(result.data)) {
              // Format a helpful message for the agent
              const ambiguous = result.data;
              const candidateList = ambiguous.candidates
                .map(
                  (c, i) =>
                    `  ${i + 1}. ${c.workspace ?? c.session_id} (${c.request_count} requests)`,
                )
                .join("\n");

              return new vscode.LanguageModelToolResult([
                new vscode.LanguageModelTextPart(
                  `AMBIGUOUS SESSION SELECTION\n\n` +
                    `${ambiguous.message}\n\n` +
                    `Candidates:\n${candidateList}\n\n` +
                    `${ambiguous.hint}\n\n` +
                    `To resolve: Call this tool again with the match-text parameter set to a distinctive phrase from a recent user message in the session you want to retrieve.`,
                ),
              ]);
            }

            // Check for no-summary response (when --before-summary was used but no summary found)
            if (isNoSummaryResponse(result.data)) {
              const noSummary = result.data;
              return new vscode.LanguageModelToolResult([
                new vscode.LanguageModelTextPart(
                  `NO SUMMARY FOUND\n\n` +
                    `${noSummary.message}\n\n` +
                    `${noSummary.hint}`,
                ),
              ]);
            }

            return new vscode.LanguageModelToolResult([
              new vscode.LanguageModelTextPart(
                JSON.stringify(result.data, null, 2),
              ),
            ]);
          } else if (result.error) {
            return new vscode.LanguageModelToolResult([
              new vscode.LanguageModelTextPart(
                JSON.stringify({ error: result.error }),
              ),
            ]);
          }
          // Fallback: return full result
          return new vscode.LanguageModelToolResult([
            new vscode.LanguageModelTextPart(
              JSON.stringify(response.result, null, 2),
            ),
          ]);
        } else {
          const errorMsg = response.error?.message ?? "Unknown error";
          return new vscode.LanguageModelToolResult([
            new vscode.LanguageModelTextPart(
              JSON.stringify({ error: errorMsg }),
            ),
          ]);
        }
      } catch (err) {
        const errorMessage = err instanceof Error ? err.message : String(err);
        return new vscode.LanguageModelToolResult([
          new vscode.LanguageModelTextPart(
            JSON.stringify({ error: errorMessage }),
          ),
        ]);
      }
    },
  };
}
