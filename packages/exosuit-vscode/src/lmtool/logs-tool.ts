import * as vscode from "vscode";
import { getRecentLogs, type LogEntry } from "../logging";
import type { LogLevel, LogComponent } from "@exosuit/core";

/**
 * Input schema for the exo-logs LM tool.
 */
interface LogsToolInput {
  lines?: number;
  level?: LogLevel;
  component?: LogComponent;
}

/**
 * Creates the exo-logs LM tool that provides access to extension logs.
 *
 * This tool enables the agent to self-diagnose issues by reading recent
 * log entries from the Exosuit output channel.
 */
export function createLogsTool(): vscode.LanguageModelTool<LogsToolInput> {
  return {
    async invoke(
      options: vscode.LanguageModelToolInvocationOptions<LogsToolInput>,
      _token: vscode.CancellationToken,
    ): Promise<vscode.LanguageModelToolResult> {
      const input = options.input ?? {};
      const lines = Math.min(input.lines ?? 50, 500);
      const filter: { level?: LogLevel; component?: LogComponent } = {};

      if (input.level) {
        filter.level = input.level;
      }
      if (input.component) {
        filter.component = input.component;
      }

      const entries = getRecentLogs(lines, filter);

      if (entries.length === 0) {
        return new vscode.LanguageModelToolResult([
          new vscode.LanguageModelTextPart(
            "No log entries found matching the criteria.",
          ),
        ]);
      }

      const formatted = formatLogEntries(entries);
      return new vscode.LanguageModelToolResult([
        new vscode.LanguageModelTextPart(formatted),
      ]);
    },
  };
}

/**
 * Format log entries for display to the agent.
 */
function formatLogEntries(entries: LogEntry[]): string {
  const lines = entries.map((entry) => {
    const ts = entry.timestamp.slice(11, 23); // HH:mm:ss.mmm
    return `[${ts}] [${entry.level}] [${entry.component}] ${entry.message}`;
  });

  return `# Exosuit Extension Logs (${entries.length} entries)\n\n\`\`\`\n${lines.join("\n")}\n\`\`\``;
}
