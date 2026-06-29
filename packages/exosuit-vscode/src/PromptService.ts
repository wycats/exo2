import * as vscode from "vscode";
import * as toml from "smol-toml";
import { interpolateStrict, validatePromptSpec } from "@exosuit/core";
import { getTraceCache } from "./services/TraceCache";
import { getLogger } from "./logging";

const logger = getLogger("extension");

export interface PromptConfig {
  global?: Record<string, string>;
  walkthrough?: Record<string, string>;
  chat?: Record<string, string>;
  [key: string]: any;
}

export class PromptService implements vscode.Disposable {
  private static _instance: PromptService;
  private _config: PromptConfig = {};
  private _reactivityDisposable: vscode.Disposable | undefined;
  private _defaults: PromptConfig = {
    global: {
      style: "Be concise and evidence-based.",
    },
    walkthrough: {
      assessTask: `Assess the status of the following task: "{task}"`,
    },
    chat: {
      systemInstruction: `You are Exosuit, a project assistant. Here is the current project context:

{context}

User Query: {query}`,
      defaultGuardrails: `=== Guardrails ===
1. **Terminal Discipline**: Always assume you are in the workspace root. If you generate commands, ensure they run from the root. If a command requires a subdirectory, use \`(cd path && cmd)\`.
2. **Context Awareness**: Use the provided Project Plan and Tasks to understand the current state.

=== Available Commands ===
You can suggest promoting content to persistent documentation using the following command links. Use URI-encoded JSON for arguments.
- To record a decision: \`[Promote to Decision](command:exosuit.promoteToDecision?%7B%22title%22%3A%22Title%22%2C%22content%22%3A%22Context%22%7D)\`
- To add a task: \`[Add Task](command:exosuit.promoteToTask?%7B%22task%22%3A%22Task%20Description%22%7D)\`
- To add an idea: \`[Add Idea](command:exosuit.promoteToIdea?%7B%22idea%22%3A%22Idea%20Description%22%7D)\`

Use these when the user makes a decision, suggests a task, or has an idea.`,
    },
  };

  private constructor() {
    this.loadConfig();
    this.watchConfig();
  }

  public static get instance(): PromptService {
    if (!PromptService._instance) {
      PromptService._instance = new PromptService();
    }
    return PromptService._instance;
  }

  private async loadConfig() {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders) {
      this._config = { ...this._defaults };
      return;
    }

    const configUri = vscode.Uri.joinPath(
      workspaceFolders[0].uri,
      ".config/exo/prompts.toml",
    );
    try {
      const document = await vscode.workspace.openTextDocument(configUri);
      const text = document.getText();
      const userConfig = toml.parse(text) as PromptConfig;

      const validation = validatePromptSpec(userConfig);
      if (!validation.ok) {
        const issues = validation.errors
          .map(
            (e: { path: string; message: string }) =>
              `- ${e.path}: ${e.message}`,
          )
          .join("\n");
        logger.warn(
          `Invalid prompt config at ${configUri.fsPath}. Keeping parsed values, but some prompts may not render correctly:\n${issues}`,
        );
      }

      this._config = this.deepMerge(this._defaults, userConfig);
    } catch (e) {
      // File doesn't exist or is invalid, use defaults
      this._config = { ...this._defaults };
    }
  }

  private watchConfig() {
    this._reactivityDisposable = getTraceCache().onDidWrite(() => {
      this.loadConfig();
    });
  }

  dispose() {
    this._reactivityDisposable?.dispose();
  }

  private deepMerge(target: any, source: any): any {
    const output = { ...target };
    if (isObject(target) && isObject(source)) {
      Object.keys(source).forEach((key) => {
        if (isObject(source[key])) {
          if (!(key in target)) {
            Object.assign(output, { [key]: source[key] });
          } else {
            output[key] = this.deepMerge(target[key], source[key]);
          }
        } else {
          Object.assign(output, { [key]: source[key] });
        }
      });
    }
    return output;
  }

  public render(key: string, variables: Record<string, string> = {}): string {
    return this.get(key, variables);
  }

  public get(key: string, variables: Record<string, string> = {}): string {
    const parts = key.split(".");
    let value: any = this._config;

    for (const part of parts) {
      if (value && typeof value === "object" && part in value) {
        value = value[part];
      } else {
        // Fallback to defaults if key missing in merged config (shouldn't happen due to merge, but safe)
        value = undefined;
        break;
      }
    }

    if (typeof value !== "string") {
      // Try defaults directly if not found
      let defaultValue: any = this._defaults;
      for (const part of parts) {
        if (
          defaultValue &&
          typeof defaultValue === "object" &&
          part in defaultValue
        ) {
          defaultValue = defaultValue[part];
        } else {
          return `[Missing Prompt: ${key}]`;
        }
      }
      value = defaultValue;
    }

    if (typeof value !== "string") {
      return `[Missing Prompt: ${key}]`;
    }

    // Interpolate using strict semantics (whitespace-tolerant {key}, preserves unknown tokens).
    const globalVars = this._config.global ?? {};
    return interpolateStrict(value, { ...globalVars, ...variables });
  }
}

function isObject(item: any) {
  return item && typeof item === "object" && !Array.isArray(item);
}
