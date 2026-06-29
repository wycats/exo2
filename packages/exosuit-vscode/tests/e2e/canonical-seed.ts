import * as fs from "fs/promises";
import * as path from "path";
import { execFile } from "child_process";
import { promisify } from "util";
import { fileURLToPath } from "url";

const execFileAsync = promisify(execFile);
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const transientRetryDelaysMs = [50, 100, 200, 400, 800, 1200] as const;

export type CanonicalEntityStatus = "pending" | "in-progress" | "completed";
export type CanonicalPhaseKind = "regular" | "chore";

export interface CanonicalSeedEpochSpec {
  key: string;
  title: string;
  status?: CanonicalEntityStatus;
}

export interface CanonicalSeedPhaseSpec {
  key: string;
  title: string;
  epoch: string;
  status?: CanonicalEntityStatus;
  kind?: CanonicalPhaseKind;
  rfcs?: string[];
}

export interface CanonicalSeedGoalSpec {
  key: string;
  id: string;
  label: string;
  phase: string;
}

export interface CanonicalSeedTaskSpec {
  key: string;
  id: string;
  label: string;
  goal?: string;
  status?: CanonicalEntityStatus;
}

export interface CanonicalSeedIdeaSpec {
  key: string;
  title: string;
  description?: string;
  tags?: string[];
}

export interface CanonicalSeedInboxSpec {
  key: string;
  subject: string;
  body?: string;
  entityType?: "goal" | "task" | "rfc" | "phase" | "epoch" | "project";
  entity?: string;
  intent?: "claim" | "concern" | "inquiry" | "fyi";
  priority?: "immediate" | "next-touch" | "when-relevant";
}

export interface CanonicalSeedRfcSpec {
  key: string;
  id: string;
  title: string;
  stage?: 0 | 1 | 2 | 3 | 4;
  body?: string;
  feature?: string;
}

export interface CanonicalSeedResult {
  workspaceRoot: string;
  epochs: Record<string, { id: string; title: string }>;
  phases: Record<string, { id: string; title: string; epochId: string }>;
  goals: Record<string, { id: string; label: string; phaseId: string }>;
  tasks: Record<string, { id: string; label: string; goalId?: string }>;
  ideas: Record<string, { id?: string; title: string }>;
  inbox: Record<string, { id?: string; subject: string }>;
  rfcs: Record<string, { id: string; title: string; path?: string }>;
}

export interface CanonicalSeedCommandResult {
  status: "ok" | "error" | "needs_input" | "confirm_required";
  result?: unknown;
  display?: {
    summary?: string;
    body?: string;
  };
  error?: {
    code?: string;
    message?: string;
    details?: unknown;
  };
}

export interface CanonicalSeedCommandRunner {
  run(
    args: string[],
    options?: { input?: string },
  ): Promise<CanonicalSeedCommandResult>;
}

const defaultExosuitToml = `[storage]
backend = "sqlite"

[dev]
binary_dir = "target/debug"
`;

export function resolveExoSeedBinary(repoRoot: string): string {
  const envBin = process.env.EXO_BIN;
  if (envBin && envBin.trim().length > 0) {
    return envBin.trim();
  }

  return path.join(repoRoot, "target", "debug", "exo");
}

export class ExoCliSeedCommandRunner implements CanonicalSeedCommandRunner {
  readonly #exoBin: string;
  readonly #workspaceRoot: string;

  constructor(exoBin: string, workspaceRoot: string) {
    this.#exoBin = exoBin;
    this.#workspaceRoot = workspaceRoot;
  }

  get exoBin(): string {
    return this.#exoBin;
  }

  async run(args: string[]): Promise<CanonicalSeedCommandResult> {
    const stdout = await execFileForSeed(
      this.#exoBin,
      args,
      this.#workspaceRoot,
    );

    if (args.includes("--format") && args.includes("json")) {
      return parseJsonCommandResult(stdout, args);
    }

    return {
      status: "ok",
      result: stdout,
      display: { body: stdout },
    };
  }
}

export async function runCanonicalSeedCommand(
  workspaceRoot: string,
  args: string[],
): Promise<CanonicalSeedCommandResult> {
  const repoRoot = path.resolve(__dirname, "../../../..");
  const runner = new ExoCliSeedCommandRunner(
    resolveExoSeedBinary(repoRoot),
    workspaceRoot,
  );
  const commandArgs = args.includes("--format")
    ? args
    : [...args, "--format", "json"];

  for (let attempt = 0; attempt <= transientRetryDelaysMs.length; attempt++) {
    const response = await runner.run(commandArgs);
    if (response.status === "ok") {
      return response;
    }
    if (
      attempt < transientRetryDelaysMs.length &&
      isTransientSeedError(response)
    ) {
      await delay(transientRetryDelaysMs[attempt]);
      continue;
    }
    throw new Error(formatSeedCommandError(commandArgs, response));
  }

  throw new Error(
    `Canonical seed command failed: exo ${commandArgs.join(" ")}`,
  );
}

class GitSeedCommandRunner implements CanonicalSeedCommandRunner {
  readonly #workspaceRoot: string;

  constructor(workspaceRoot: string) {
    this.#workspaceRoot = workspaceRoot;
  }

  async run(args: string[]): Promise<CanonicalSeedCommandResult> {
    const stdout = await execFileForSeed("git", args, this.#workspaceRoot);

    return {
      status: "ok",
      result: stdout,
      display: { body: stdout },
    };
  }
}

export interface CanonicalSeedBuilderOptions {
  workspaceRoot: string;
  runner: CanonicalSeedCommandRunner;
  gitRunner?: CanonicalSeedCommandRunner;
  exosuitToml?: string;
  reset?: boolean;
}

/**
 * Builder interface for canonical daemon/SQLite-backed E2E fixtures.
 *
 * This is intentionally separate from ScenarioBuilder. ScenarioBuilder writes
 * document fixtures directly; CanonicalSeedBuilder describes state that must be
 * created through `exo` commands so the daemon, SQLite store, and TraceCache
 * see the same state a real workspace would produce.
 */
export class CanonicalSeedBuilder {
  readonly #workspaceRoot: string;
  readonly #runner: CanonicalSeedCommandRunner;
  readonly #gitRunner: CanonicalSeedCommandRunner;
  readonly #exosuitToml: string;
  readonly #reset: boolean;
  readonly #epochs: CanonicalSeedEpochSpec[] = [];
  readonly #phases: CanonicalSeedPhaseSpec[] = [];
  readonly #goals: CanonicalSeedGoalSpec[] = [];
  readonly #tasks: CanonicalSeedTaskSpec[] = [];
  readonly #ideas: CanonicalSeedIdeaSpec[] = [];
  readonly #inbox: CanonicalSeedInboxSpec[] = [];
  readonly #rfcs: CanonicalSeedRfcSpec[] = [];

  constructor(options: CanonicalSeedBuilderOptions) {
    this.#workspaceRoot = options.workspaceRoot;
    this.#runner = options.runner;
    this.#gitRunner =
      options.gitRunner ?? new GitSeedCommandRunner(this.#workspaceRoot);
    this.#exosuitToml = options.exosuitToml ?? defaultExosuitToml;
    this.#reset = options.reset ?? true;
  }

  epoch(spec: CanonicalSeedEpochSpec): this {
    this.#epochs.push(spec);
    return this;
  }

  phase(spec: CanonicalSeedPhaseSpec): this {
    this.#phases.push(spec);
    return this;
  }

  goal(spec: CanonicalSeedGoalSpec): this {
    this.#goals.push(spec);
    return this;
  }

  task(spec: CanonicalSeedTaskSpec): this {
    this.#tasks.push(spec);
    return this;
  }

  idea(spec: CanonicalSeedIdeaSpec): this {
    this.#ideas.push(spec);
    return this;
  }

  inbox(spec: CanonicalSeedInboxSpec): this {
    this.#inbox.push(spec);
    return this;
  }

  rfc(spec: CanonicalSeedRfcSpec): this {
    this.#rfcs.push(spec);
    return this;
  }

  async apply(): Promise<CanonicalSeedResult> {
    const result: CanonicalSeedResult = {
      workspaceRoot: this.#workspaceRoot,
      epochs: {},
      phases: {},
      goals: {},
      tasks: {},
      ideas: {},
      inbox: {},
      rfcs: {},
    };

    await this.#initProject();

    const activeEpochKeys = new Set(
      this.#phases
        .filter((phase) => phase.status === "in-progress")
        .map((phase) => phase.epoch),
    );

    for (const epoch of this.#epochs) {
      const response = await this.#run([
        "epoch",
        "add",
        "--title",
        epoch.title,
      ]);
      const id = readStringField(response.result, "id");
      result.epochs[epoch.key] = { id, title: epoch.title };
      if (epoch.status === "in-progress" && !activeEpochKeys.has(epoch.key)) {
        await this.#run(["epoch", "start", id]);
      }
    }

    for (const rfc of this.#rfcs) {
      await this.#run([
        "rfc",
        "create",
        rfc.title,
        "--id",
        rfc.id,
        "--stage",
        String(rfc.stage ?? 0),
        "--feature",
        rfc.feature ?? "test",
        "--body",
        rfc.body ?? "Canonical E2E fixture RFC.",
      ]);
      result.rfcs[rfc.key] = { id: rfc.id, title: rfc.title };
    }

    for (const phase of this.#phases) {
      const epoch = result.epochs[phase.epoch];
      if (!epoch) {
        throw new Error(`Unknown canonical seed epoch key: ${phase.epoch}`);
      }

      const args = [
        "phase",
        "add",
        "--title",
        phase.title,
        "--epoch",
        epoch.id,
        "--kind",
        phase.kind ?? "regular",
      ];
      if (phase.rfcs && phase.rfcs.length > 0) {
        args.push(
          "--rfcs",
          phase.rfcs.map((key) => this.#rfcId(result, key)).join(","),
        );
      }

      const response = await this.#run(args);
      const id = readStringField(response.result, "id");
      result.phases[phase.key] = { id, title: phase.title, epochId: epoch.id };
      if (phase.status === "in-progress") {
        await this.#run(["phase", "start", id]);
      }
    }

    for (const goal of this.#goals) {
      const phase = result.phases[goal.phase];
      if (!phase) {
        throw new Error(`Unknown canonical seed phase key: ${goal.phase}`);
      }
      await this.#run([
        "goal",
        "add",
        goal.label,
        "--id",
        goal.id,
        "--phase",
        phase.id,
      ]);
      result.goals[goal.key] = {
        id: goal.id,
        label: goal.label,
        phaseId: phase.id,
      };
    }

    for (const task of this.#tasks) {
      const args = ["task", "add", task.label, "--id", task.id];
      const goalId = task.goal ? result.goals[task.goal]?.id : undefined;
      if (task.goal && !goalId) {
        throw new Error(`Unknown canonical seed goal key: ${task.goal}`);
      }
      if (goalId) {
        args.push("--goal", goalId);
      }
      await this.#run(args);
      if (task.status === "in-progress") {
        await this.#run(["task", "start", task.id]);
      }
      if (task.status === "completed") {
        await this.#run([
          "task",
          "complete",
          task.id,
          "--log",
          "Completed by canonical E2E seed.",
        ]);
      }
      result.tasks[task.key] = { id: task.id, label: task.label, goalId };
    }

    for (const idea of this.#ideas) {
      await this.#run([
        "idea",
        "add",
        idea.title,
        "--description",
        idea.description ?? "",
        "--tags",
        idea.tags?.join(",") ?? "",
      ]);
      result.ideas[idea.key] = { title: idea.title };
    }

    for (const item of this.#inbox) {
      const entityType = item.entityType ?? "project";
      const args = [
        "inbox",
        "add",
        item.subject,
        "--entity-type",
        entityType,
        "--intent",
        item.intent ?? "fyi",
        "--priority",
        item.priority ?? "next-touch",
        "--body",
        item.body ?? "",
      ];
      const entityId = item.entity
        ? this.#entityId(result, entityType, item.entity)
        : undefined;
      if (entityId) {
        args.push("--entity-id", entityId);
      }
      await this.#run(args);
      result.inbox[item.key] = { subject: item.subject };
    }

    await this.verify();
    return result;
  }

  async verify(): Promise<void> {
    await this.#run(["status"]);
    await this.#run(["plan", "review"]);
    await this.#run(["phase", "status"]);
    await this.#run(["goal", "list"]);
    await this.#run(["task", "list"]);
  }

  async #initProject(): Promise<void> {
    if (
      !this.#reset &&
      (await fileExists(
        path.join(this.#workspaceRoot, ".exo", "cache", "exo.db"),
      ))
    ) {
      return;
    }

    await fs.mkdir(this.#workspaceRoot, { recursive: true });
    await this.#gitRunner.run(["init"]);
    await fs.writeFile(
      path.join(this.#workspaceRoot, "exosuit.toml"),
      this.#exosuitToml,
    );
    await this.#runRaw(["init", "--defaults"]);
    await this.#installWorkspaceLocalExoBinary();
  }

  async #installWorkspaceLocalExoBinary(): Promise<void> {
    const exoBin =
      this.#runner instanceof ExoCliSeedCommandRunner
        ? this.#runner.exoBin
        : undefined;
    if (!exoBin) {
      return;
    }

    const workspaceBin = path.join(
      this.#workspaceRoot,
      "target",
      "debug",
      "exo",
    );
    await fs.mkdir(path.dirname(workspaceBin), { recursive: true });
    await fs.copyFile(exoBin, workspaceBin);
    await fs.chmod(workspaceBin, 0o755);
  }

  async #run(args: string[]): Promise<CanonicalSeedCommandResult> {
    return this.#runRaw([...args, "--format", "json"]);
  }

  async #runRaw(args: string[]): Promise<CanonicalSeedCommandResult> {
    for (let attempt = 0; attempt <= transientRetryDelaysMs.length; attempt++) {
      const response = await this.#runner.run(args);
      if (response.status === "ok") {
        return response;
      }

      if (
        attempt < transientRetryDelaysMs.length &&
        isTransientSeedError(response)
      ) {
        await delay(transientRetryDelaysMs[attempt]);
        continue;
      }

      throw new Error(formatSeedCommandError(args, response));
    }

    throw new Error(`Canonical seed command failed: exo ${args.join(" ")}`);
  }

  #rfcId(result: CanonicalSeedResult, key: string): string {
    const rfc = result.rfcs[key];
    if (!rfc) {
      throw new Error(`Unknown canonical seed RFC key: ${key}`);
    }
    return rfc.id;
  }

  #entityId(
    result: CanonicalSeedResult,
    entityType: NonNullable<CanonicalSeedInboxSpec["entityType"]>,
    key: string,
  ): string {
    switch (entityType) {
      case "epoch":
        return requiredEntity(result.epochs, key, entityType).id;
      case "phase":
        return requiredEntity(result.phases, key, entityType).id;
      case "goal":
        return requiredEntity(result.goals, key, entityType).id;
      case "task":
        return requiredEntity(result.tasks, key, entityType).id;
      case "rfc":
        return requiredEntity(result.rfcs, key, entityType).id;
      case "project":
        return key;
    }
  }
}

function parseJsonCommandResult(
  stdout: string,
  args: string[],
): CanonicalSeedCommandResult {
  try {
    return JSON.parse(stdout) as CanonicalSeedCommandResult;
  } catch (error) {
    throw new Error(
      `Failed to parse JSON from canonical seed command: exo ${args.join(
        " ",
      )}\n${error instanceof Error ? error.message : String(error)}\n${stdout}`,
    );
  }
}

async function execFileForSeed(
  file: string,
  args: string[],
  cwd: string,
): Promise<string> {
  try {
    const { stdout } = await execFileAsync(file, args, {
      cwd,
      maxBuffer: 10 * 1024 * 1024,
    });
    return stdout;
  } catch (error) {
    const maybeOutput = error as { stdout?: unknown };
    if (
      typeof maybeOutput.stdout === "string" &&
      maybeOutput.stdout.length > 0
    ) {
      return maybeOutput.stdout;
    }
    throw error;
  }
}

function requiredEntity<T extends { id: string }>(
  entities: Record<string, T>,
  key: string,
  entityType: string,
): T {
  const entity = entities[key];
  if (!entity) {
    throw new Error(`Unknown canonical seed ${entityType} key: ${key}`);
  }
  return entity;
}

function readStringField(value: unknown, field: string): string {
  if (!value || typeof value !== "object") {
    throw new Error(
      `Expected command result object with string field '${field}'`,
    );
  }

  const fieldValue = (value as Record<string, unknown>)[field];
  if (typeof fieldValue !== "string" || fieldValue.length === 0) {
    throw new Error(
      `Expected command result field '${field}' to be a non-empty string`,
    );
  }
  return fieldValue;
}

function formatSeedCommandError(
  args: string[],
  response: CanonicalSeedCommandResult,
): string {
  return `Canonical seed command failed: exo ${args.join(" ")}\n${
    response.error?.message ?? "Unknown error"
  }`;
}

function isTransientSeedError(response: CanonicalSeedCommandResult): boolean {
  const message = response.error?.message ?? "";
  return (
    message.includes("SQLite database") ||
    message.includes("open database") ||
    message.includes("database is locked") ||
    message.includes("Failed to open SQLite")
  );
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fileExists(filePath: string): Promise<boolean> {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}
