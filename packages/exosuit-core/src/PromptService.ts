import * as fs from "fs";
import * as path from "path";
import * as toml from "smol-toml";
import { validatePromptSpec } from "./models/PromptSpec.ts";
import { interpolateStrict } from "./interpolation.ts";
import { type Logger, createNoopLogger } from "./Logger.ts";

export class PromptService {
  private prompts: Record<string, any> = {};
  private promptsPath: string;
  private logger: Logger;

  constructor(rootDir: string, logger?: Logger) {
    this.promptsPath = path.join(
      rootDir,
      "docs",
      "agent-context",
      "prompts.toml"
    );
    this.logger = logger ?? createNoopLogger("core");
    this.load();
  }

  public load(): void {
    if (fs.existsSync(this.promptsPath)) {
      try {
        const content = fs.readFileSync(this.promptsPath, "utf-8");
        const parsed = toml.parse(content);

        const validation = validatePromptSpec(parsed);
        if (!validation.ok) {
          const issues = validation.errors
            .map((e) => `- ${e.path}: ${e.message}`)
            .join("\n");
          this.logger.warn(
            `Invalid prompts.toml at ${this.promptsPath}. Keeping parsed values, but some prompts may not render correctly:\n${issues}`
          );
        }

        this.prompts = parsed;
      } catch (e) {
        this.logger.error(
          `Failed to parse prompts.toml at ${this.promptsPath}`,
          e,
        );
        this.prompts = {};
      }
    } else {
      this.prompts = {};
    }
  }

  public get(key: string): string | undefined {
    const parts = key.split(".");
    let current: any = this.prompts;

    for (const part of parts) {
      if (current === undefined || current === null) return undefined;
      current = current[part];
    }

    return typeof current === "string" ? current : undefined;
  }

  public render(key: string, variables: Record<string, string> = {}): string {
    const template = this.get(key);
    if (!template) {
      throw new Error(`Prompt not found: ${key}`);
    }

    return interpolateStrict(template, variables);
  }
}
