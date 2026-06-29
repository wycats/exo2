import * as vscode from "vscode";

/**
 * Input schema for the exo-diagnostics LM tool.
 */
interface DiagnosticsToolInput {
  file?: string;
  severity?: "error" | "warning" | "info" | "hint";
  source?: string;
  limit?: number;
}

interface DiagnosticEntry {
  file: string;
  line: number;
  column: number;
  severity: string;
  source: string;
  message: string;
  code?: string;
}

interface DiagnosticsResult {
  totalErrors: number;
  totalWarnings: number;
  totalInfo: number;
  totalHints: number;
  diagnostics: DiagnosticEntry[];
  truncated: boolean;
}

const SEVERITY_MAP: Record<number, string> = {
  [vscode.DiagnosticSeverity.Error]: "error",
  [vscode.DiagnosticSeverity.Warning]: "warning",
  [vscode.DiagnosticSeverity.Information]: "info",
  [vscode.DiagnosticSeverity.Hint]: "hint",
};

const SEVERITY_FILTER_MAP: Record<string, vscode.DiagnosticSeverity> = {
  error: vscode.DiagnosticSeverity.Error,
  warning: vscode.DiagnosticSeverity.Warning,
  info: vscode.DiagnosticSeverity.Information,
  hint: vscode.DiagnosticSeverity.Hint,
};

/**
 * Creates the exo-diagnostics LM tool that provides detailed workspace
 * diagnostic information.
 *
 * This is the "active query" complement to the passive exo-status enrichment.
 * While exo-status gives the agent a summary with every status check,
 * this tool lets the agent drill down into specific files, severities,
 * or sources when investigating issues.
 */
export function createDiagnosticsTool(): vscode.LanguageModelTool<DiagnosticsToolInput> {
  return {
    async invoke(
      options: vscode.LanguageModelToolInvocationOptions<DiagnosticsToolInput>,
      _token: vscode.CancellationToken,
    ): Promise<vscode.LanguageModelToolResult> {
      const input = options.input ?? {};
      const limit = Math.min(input.limit ?? 50, 200);
      const severityFilter =
        input.severity && input.severity in SEVERITY_FILTER_MAP
          ? SEVERITY_FILTER_MAP[input.severity]
          : undefined;

      const allDiagnostics = vscode.languages.getDiagnostics();
      const workspaceRoot =
        vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";

      let totalErrors = 0;
      let totalWarnings = 0;
      let totalInfo = 0;
      let totalHints = 0;
      const entries: DiagnosticEntry[] = [];

      for (const [uri, diagnostics] of allDiagnostics) {
        const filePath = workspaceRoot
          ? uri.fsPath.replace(workspaceRoot + "/", "")
          : uri.fsPath;

        // File filter
        if (input.file && !filePath.includes(input.file)) {
          continue;
        }

        for (const diag of diagnostics) {
          // Count all diagnostics (before severity/source filtering)
          switch (diag.severity) {
            case vscode.DiagnosticSeverity.Error:
              totalErrors++;
              break;
            case vscode.DiagnosticSeverity.Warning:
              totalWarnings++;
              break;
            case vscode.DiagnosticSeverity.Information:
              totalInfo++;
              break;
            case vscode.DiagnosticSeverity.Hint:
              totalHints++;
              break;
          }

          // Apply filters
          if (
            severityFilter !== undefined &&
            diag.severity !== severityFilter
          ) {
            continue;
          }
          if (input.source && diag.source !== input.source) {
            continue;
          }

          if (entries.length < limit) {
            const code = diag.code;
            const codeStr =
              code === undefined
                ? undefined
                : typeof code === "object"
                  ? String(code.value)
                  : String(code);

            entries.push({
              file: filePath,
              line: diag.range.start.line + 1,
              column: diag.range.start.character + 1,
              severity: SEVERITY_MAP[diag.severity] ?? "unknown",
              source: diag.source ?? "unknown",
              message: diag.message,
              ...(codeStr ? { code: codeStr } : {}),
            });
          }
        }
      }

      const result: DiagnosticsResult = {
        totalErrors,
        totalWarnings,
        totalInfo,
        totalHints,
        diagnostics: entries,
        truncated: entries.length >= limit,
      };

      return new vscode.LanguageModelToolResult([
        new vscode.LanguageModelTextPart(JSON.stringify(result)),
      ]);
    },
  };
}
