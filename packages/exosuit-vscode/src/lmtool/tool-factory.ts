/**
 * Tool Factory: Generate VS Code Language Model Tools from CommandSpec
 *
 * This module creates LM tools dynamically from the command-spec.json artifact,
 * enabling VS Code language models to interact with Exosuit CLI commands.
 *
 * @see RFC 0132 - CLI Patterns
 * @see docs/agent-context/current/map-north-star.md - LM Tool Strategy
 */

import * as vscode from "vscode";
import { randomUUID } from "node:crypto";
import type {
  ArgSpec,
  OperationSpec,
  Effect,
  ValueType,
} from "./command-spec.types";
import { getAllOperations } from "./command-spec.types";
import { getLogger } from "../logging";
import {
  MACHINE_CHANNEL_PROTOCOL_VERSION,
  type MachineChannelRequestEnvelope,
  type MachineChannelResponseEnvelope,
} from "../types/machineChannel";
import { exoMachineChannel } from "../agent/lmtool/machineChannel";
import { selectCurrentWorkspaceRoot } from "../workspaceRoot";

const logger = getLogger("lmtool");

/**
 * Base interface for tool input parameters.
 * Specific tools will extend this with their own parameter types.
 */
type ToolInput = Record<string, unknown>;

type ExoCommandResult = {
  stdout: string;
  stderr: string;
  exitedWithError: boolean;
  error?: MachineChannelResponseEnvelope["error"];
  steering?: MachineChannelResponseEnvelope["steering"];
};

/**
 * Execute an exo command with arguments and return the result.
 *
 * @param namespace - The command namespace (e.g., "phase", "task")
 * @param operation - The operation name (e.g., "start", "complete")
 * @param args - The parsed arguments for the operation
 * @param rootPath - The workspace root path
 * @returns The command output (stdout)
 */
async function executeExoCommand(
  namespace: string,
  operation: string,
  args: ArgSpec[],
  input: ToolInput,
  rootPath: string,
): Promise<ExoCommandResult> {
  const payload = buildMachineChannelInput(args, input);
  const request = buildMachineChannelRequest(namespace, operation, payload);

  logger.debug(
    `[tool-factory] Executing via machine channel: ${formatCommandLog(namespace, operation)}`,
  );

  try {
    let response = await exoMachineChannel(rootPath, request);
    response = await handleConfirmationIfNeeded(rootPath, request, response);
    return responseToCommandResult(response);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    return {
      stdout: "",
      stderr: message,
      exitedWithError: true,
    };
  }
}

/**
 * Build CLI arguments from ArgSpec and input parameters.
 *
 * This converts the structured input into CLI flag/option/positional format:
 * - Flags: --flag (if true) or omitted (if false)
 * - Options: --option value
 * - Positionals: value (in order)
 */
function buildMachineChannelInput(
  args: ArgSpec[],
  input: ToolInput,
): Record<string, unknown> {
  const payload: Record<string, unknown> = {};

  for (const arg of args) {
    const value = input[arg.id];

    const effectiveValue =
      value !== undefined
        ? value
        : arg.default !== undefined
          ? parseDefault(arg.default, arg.value_type)
          : undefined;

    if (effectiveValue === undefined) {
      if (arg.optional) {
        continue;
      }
      throw new Error(
        `Missing required argument: ${arg.name} (${arg.description})`,
      );
    }

    const normalizedValue = normalizeInputValue(effectiveValue, arg.value_type);

    if (arg.kind === "flag") {
      if (normalizedValue === true) {
        payload[arg.id] = true;
      }
      continue;
    }

    payload[arg.id] = normalizedValue;
  }

  return payload;
}

function normalizeInputValue(value: unknown, valueType: ValueType): unknown {
  if (value === null || value === undefined) {
    return value;
  }

  if (typeof valueType === "object" && "enum" in valueType) {
    return value;
  }

  switch (valueType) {
    case "bool": {
      if (typeof value === "boolean") {
        return value;
      }
      if (typeof value === "string") {
        return value.toLowerCase() === "true";
      }
      if (typeof value === "number") {
        return value !== 0;
      }
      return Boolean(value);
    }
    case "int": {
      if (typeof value === "number") {
        return Math.trunc(value);
      }
      if (typeof value === "string") {
        const parsed = Number.parseInt(value, 10);
        return Number.isNaN(parsed) ? value : parsed;
      }
      return value;
    }
    case "float": {
      if (typeof value === "number") {
        return value;
      }
      if (typeof value === "string") {
        const parsed = Number.parseFloat(value);
        return Number.isNaN(parsed) ? value : parsed;
      }
      return value;
    }
    case "json": {
      if (typeof value === "string") {
        try {
          return JSON.parse(value);
        } catch {
          return value;
        }
      }
      return value;
    }
    case "string":
    case "path":
      return String(value);
    default:
      return value;
  }
}

function buildMachineChannelRequest(
  namespace: string,
  operation: string,
  input: Record<string, unknown>,
): MachineChannelRequestEnvelope {
  const path = namespace ? [namespace, operation] : [operation];

  return {
    protocol_version: MACHINE_CHANNEL_PROTOCOL_VERSION,
    id: `vscode.lmtool.${namespace}.${operation}.${randomUUID()}`,
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path },
        input,
      },
    },
  };
}

async function handleConfirmationIfNeeded(
  rootPath: string,
  request: MachineChannelRequestEnvelope,
  response: MachineChannelResponseEnvelope,
): Promise<MachineChannelResponseEnvelope> {
  if (response.status !== "confirm_required" || !response.ticket) {
    return response;
  }

  const confirmedRequest: MachineChannelRequestEnvelope = {
    ...request,
    id: `vscode.lmtool.confirm.${randomUUID()}`,
    auth: { ticket: response.ticket, confirm: true },
  };

  return exoMachineChannel(rootPath, confirmedRequest);
}

function responseToCommandResult(response: MachineChannelResponseEnvelope): {
  stdout: string;
  stderr: string;
  exitedWithError: boolean;
  error?: MachineChannelResponseEnvelope["error"];
  steering?: MachineChannelResponseEnvelope["steering"];
} {
  const output =
    response.reminders && response.reminders.length > 0
      ? { result: response.result ?? null, reminders: response.reminders }
      : (response.result ?? null);

  switch (response.status) {
    case "ok":
      return {
        stdout: JSON.stringify(output),
        stderr: "",
        exitedWithError: false,
      };
    case "needs_input":
      return {
        stdout: JSON.stringify(response.result ?? null),
        stderr:
          response.error?.message ??
          "Command requires additional input to proceed",
        exitedWithError: true,
      };
    case "confirm_required":
      return {
        stdout: JSON.stringify(response.result ?? null),
        stderr: "Command requires confirmation to proceed",
        exitedWithError: true,
      };
    case "error":
    default:
      return {
        stdout: JSON.stringify({
          result: response.result ?? null,
          error: response.error ?? null,
          steering: response.steering ?? null,
        }),
        stderr: response.error?.message ?? "Command failed",
        exitedWithError: true,
        error: response.error,
        steering: response.steering,
      };
  }
}

/**
 * JSON Schema type for tool input definitions.
 * Used for generating package.json languageModelTools contributions.
 */
export interface JsonSchema {
  type: string;
  description?: string;
  properties?: Record<string, JsonSchema>;
  required?: string[];
  additionalProperties?: boolean;
  enum?: string[];
  default?: unknown;
  format?: string;
}

/**
 * Convert ArgSpec to JSON Schema format.
 *
 * This builds the JSON Schema that defines what parameters the tool accepts.
 * Use this to generate package.json languageModelTools contributions.
 */
function buildInputSchema(args: ArgSpec[]): JsonSchema {
  const properties: Record<string, JsonSchema> = {};
  const required: string[] = [];

  for (const arg of args) {
    properties[arg.id] = argSpecToSchema(arg);

    if (!arg.optional && !arg.default) {
      required.push(arg.id);
    }
  }

  return {
    type: "object",
    properties,
    required: required.length > 0 ? required : undefined,
    additionalProperties: false,
  };
}

/**
 * Convert a single ArgSpec to a JSON Schema property definition.
 */
function argSpecToSchema(arg: ArgSpec): JsonSchema {
  // Handle enum type
  if (typeof arg.value_type === "object" && "enum" in arg.value_type) {
    return {
      type: "string",
      enum: arg.value_type.enum,
      description: arg.description,
    };
  }

  // Map ValueType to JSON Schema type
  const typeMap: Record<string, string> = {
    bool: "boolean",
    int: "integer",
    float: "number",
    string: "string",
    path: "string",
    json: "object",
  };

  const schema: JsonSchema = {
    type: typeMap[arg.value_type] || "string",
    description: arg.description,
  };

  // Add default value if present
  if (arg.default !== undefined) {
    schema.default = parseDefault(arg.default, arg.value_type);
  }

  return schema;
}

/**
 * Parse a default value string based on its type.
 */
function parseDefault(defaultValue: string, valueType: ValueType): unknown {
  if (typeof valueType === "object" && "enum" in valueType) {
    return defaultValue;
  }

  switch (valueType) {
    case "bool":
      return defaultValue === "true";
    case "int":
      return parseInt(defaultValue, 10);
    case "float":
      return parseFloat(defaultValue);
    case "json":
      try {
        return JSON.parse(defaultValue);
      } catch {
        return defaultValue;
      }
    default:
      return defaultValue;
  }
}

/**
 * Create a human-readable summary of a tool invocation.
 */
function summarizeInvocation(
  namespace: string,
  operation: string,
  spec: OperationSpec,
  input: ToolInput,
): string {
  const parts = [formatCommandDisplay(namespace, operation)];

  // Add key parameter values to the summary
  for (const arg of spec.args) {
    const value = input[arg.id];
    if (value !== undefined && value !== null) {
      if (arg.kind === "flag" && value === true) {
        parts.push(`--${arg.name}`);
      } else if (arg.kind === "positional") {
        parts.push(String(value));
      } else if (arg.kind === "option") {
        parts.push(`--${arg.name}=${value}`);
      }
    }
  }

  return parts.join(" ");
}

function formatCommandDisplay(namespace: string, operation: string): string {
  return namespace ? `exo ${namespace} ${operation}` : `exo ${operation}`;
}

function formatCommandLog(namespace: string, operation: string): string {
  return namespace ? `${namespace}.${operation}` : `root.${operation}`;
}

/**
 * Generate an LM tool from a CommandSpec operation.
 *
 * This creates a VS Code LanguageModelTool that:
 * - Accepts parameters based on ArgSpec definitions
 * - Invokes `exo <namespace> <operation> [args...]`
 * - Returns the result in an appropriate format
 *
 * @param namespace - The command namespace (e.g., "phase", "task")
 * @param operationName - The operation name (e.g., "start", "complete")
 * @param operation - The operation specification
 * @returns A VS Code LanguageModelTool, or null if the operation shouldn't be exposed
 */
export function createToolFromSpec(
  namespace: string,
  operationName: string,
  operation: OperationSpec,
): vscode.LanguageModelTool<ToolInput> | null {
  // Don't create tools for operations that need upgrade gates
  // These should go through lifecycle tools which handle them explicitly
  if (operation.needs_upgrade_gate) {
    logger.trace(
      `[tool-factory] Skipping ${formatCommandLog(namespace, operationName)} - needs upgrade gate`,
    );
    return null;
  }

  return buildToolFromSpec(namespace, operationName, operation);
}

/**
 * Build a tool instance from a CommandSpec operation.
 * This is pure construction with no policy checks.
 *
 * Use createToolFromSpec() for the standard policy (skips upgrade-gated).
 * Use this directly when you have an explicit list of operations to expose
 * (e.g., lifecycle tools).
 *
 * @param namespace - The command namespace (e.g., "phase", "task")
 * @param operationName - The operation name (e.g., "start", "complete")
 * @param operation - The operation specification
 * @returns A VS Code LanguageModelTool
 */
export function buildToolFromSpec(
  namespace: string,
  operationName: string,
  operation: OperationSpec,
): vscode.LanguageModelTool<ToolInput> {
  // Create the tool instance
  // Note: inputSchema is defined in package.json contributes.languageModelTools,
  // not on the tool instance itself. Use buildInputSchema() to generate
  // the package.json contribution.
  const tool: vscode.LanguageModelTool<ToolInput> = {
    prepareInvocation(
      options: vscode.LanguageModelToolInvocationPrepareOptions<ToolInput>,
    ): vscode.PreparedToolInvocation {
      const summary = summarizeInvocation(
        namespace,
        operationName,
        operation,
        options.input,
      );

      const needsConfirmation = operation.effect !== "pure";

      return {
        invocationMessage: summary,
        confirmationMessages: needsConfirmation
          ? {
              title: `Exo: ${formatCommandDisplay(namespace, operationName)}`,
              message: new vscode.MarkdownString(
                `Allow this operation?\n\n**Command**: \`${formatCommandDisplay(namespace, operationName)}\`\n\n` +
                  `**Effect**: ${operation.effect}\n\n` +
                  `${operation.description}`,
              ),
            }
          : undefined,
      };
    },

    async invoke(
      options: vscode.LanguageModelToolInvocationOptions<ToolInput>,
      _token: vscode.CancellationToken,
    ): Promise<vscode.LanguageModelToolResult> {
      const workspaceSelection = selectCurrentWorkspaceRoot();
      const rootPath = workspaceSelection.rootPath;
      if (!rootPath) {
        return new vscode.LanguageModelToolResult([
          new vscode.LanguageModelTextPart(
            JSON.stringify({
              status: "error",
              code: "NO_WORKSPACE",
              message: `No usable Exosuit workspace root: ${workspaceSelection.reason}`,
              candidates: workspaceSelection.candidates,
            }),
          ),
        ]);
      }

      try {
        const { stdout, stderr, exitedWithError, error, steering } =
          await executeExoCommand(
            namespace,
            operationName,
            operation.args,
            options.input,
            rootPath,
          );

        // If CLI exited with error, report as error (not warning)
        if (exitedWithError) {
          return new vscode.LanguageModelToolResult([
            new vscode.LanguageModelTextPart(
              JSON.stringify({
                status: "error",
                code: error?.code ?? "COMMAND_FAILED",
                message: stderr || "Command failed with no error message",
                details: error?.details,
                steering,
                output: stdout,
              }),
            ),
          ]);
        }

        // If we have stderr but command succeeded, include as warning
        if (stderr && stderr.trim().length > 0) {
          return new vscode.LanguageModelToolResult([
            new vscode.LanguageModelTextPart(
              JSON.stringify({
                status: "warning",
                code: "STDERR_PRESENT",
                message: "Command completed with warnings",
                output: stdout,
                warnings: stderr,
              }),
            ),
          ]);
        }

        // Try to parse as JSON first (many exo commands support --format json)
        try {
          const parsed = JSON.parse(stdout);
          return new vscode.LanguageModelToolResult([
            new vscode.LanguageModelTextPart(JSON.stringify(parsed, null, 2)),
          ]);
        } catch {
          // Not JSON, return as plain text
          return new vscode.LanguageModelToolResult([
            new vscode.LanguageModelTextPart(stdout),
          ]);
        }
      } catch (error: unknown) {
        const message = error instanceof Error ? error.message : String(error);
        return new vscode.LanguageModelToolResult([
          new vscode.LanguageModelTextPart(
            JSON.stringify({
              status: "error",
              code: "COMMAND_FAILED",
              message,
            }),
          ),
        ]);
      }
    },
  };

  return tool;
}

// Close buildToolFromSpec - note: buildToolFromSpec always returns a tool,
// unlike createToolFromSpec which may return null for policy reasons

/**
 * Create only "zero-arg" tools - pure operations with no required arguments.
 * These are ideal for orientation/context gathering.
 *
 * @returns Map of tool name to tool instance
 */
export function createZeroArgTools(): Map<
  string,
  vscode.LanguageModelTool<ToolInput>
> {
  const tools = new Map<string, vscode.LanguageModelTool<ToolInput>>();

  for (const [nsName, opName, op] of getAllOperations()) {
    // Zero-arg criteria from RFC 0132:
    // - Pure effect (read-only)
    // - All arguments are optional or have defaults
    const isZeroArg =
      op.effect === "pure" &&
      op.args.every((arg) => arg.optional || arg.default !== undefined);

    if (isZeroArg) {
      const tool = createToolFromSpec(nsName, opName, op);
      if (tool) {
        const toolName = nsName ? `exo-${nsName}-${opName}` : `exo-${opName}`;
        tools.set(toolName, tool);
        logger.trace(`[tool-factory] Created zero-arg tool: ${toolName}`);
      }
    }
  }

  logger.trace(
    `[tool-factory] Created ${tools.size} zero-arg tools from command spec`,
  );
  return tools;
}

/**
 * Alias map for backward-compatible tool names.
 * Maps short names (used in ZeroArgTools) to full factory names.
 *
 * The factory generates names like `exo-{namespace}-{operation}`, but
 * legacy tools used simpler names. This provides both for UX continuity.
 */
export const TOOL_ALIASES: ReadonlyMap<string, string> = new Map([
  // Simple aliases for common operations
  ["exo-phase", "exo-phase-status"],
  ["exo-plan", "exo-plan-review"],
  ["exo-inbox", "exo-inbox-list"],
  ["exo-context", "exo-ai-context"],
  ["exo-steering", "exo-map"],
]);

/**
 * Create zero-arg tools with backward-compatible aliases.
 * Registers with package.json names FIRST, then adds full names as aliases.
 *
 * VS Code requires the registered tool name to exactly match the package.json
 * declaration. This function ensures the short names (exo-phase) are the primary
 * registrations, with full names (exo-phase-status) available as aliases.
 *
 * @returns Map of tool name to tool instance (includes aliases)
 */
export function createZeroArgToolsWithAliases(): Map<
  string,
  vscode.LanguageModelTool<ToolInput>
> {
  const tools = createZeroArgTools();

  // Register package.json names FIRST, then add full names as aliases
  // This fixes the "disabled by user" issue where VS Code couldn't find
  // the tool by its package.json name.
  for (const [shortName, fullName] of TOOL_ALIASES) {
    const tool = tools.get(fullName);
    if (tool) {
      // Remove the full name from the map
      tools.delete(fullName);
      // Register with short name (package.json contract)
      tools.set(shortName, tool);
      // Add full name back as an alias for backward compatibility
      tools.set(fullName, tool);
      logger.trace(
        `[tool-factory] Registered ${shortName} with alias ${fullName}`,
      );
    } else {
      logger.warn(`[tool-factory] Alias target not found: ${fullName}`);
    }
  }

  return tools;
}

/**
 * Get metadata about available tools without creating the tool instances.
 * Useful for debugging and documentation.
 */
export interface ToolMetadata {
  name: string;
  namespace: string;
  operation: string;
  description: string;
  effect: Effect;
  argumentCount: number;
  requiredArguments: string[];
  isZeroArg: boolean;
}

export function getToolMetadata(): ToolMetadata[] {
  const metadata: ToolMetadata[] = [];

  for (const [nsName, opName, op] of getAllOperations()) {
    const requiredArgs = op.args
      .filter((arg) => !arg.optional && !arg.default)
      .map((arg) => arg.name);

    const isZeroArg =
      op.effect === "pure" &&
      op.args.every((arg) => arg.optional || arg.default !== undefined);

    metadata.push({
      name: nsName ? `exo-${nsName}-${opName}` : `exo-${opName}`,
      namespace: nsName || "root",
      operation: opName,
      description: op.description,
      effect: op.effect,
      argumentCount: op.args.length,
      requiredArguments: requiredArgs,
      isZeroArg,
    });
  }

  return metadata;
}

// Export buildInputSchema for testing
export { buildInputSchema };
