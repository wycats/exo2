import * as vscode from "vscode";
import { randomUUID } from "node:crypto";

import { exoMachineChannel } from "../agent/lmtool/machineChannel";
import { getLogger } from "../logging";
import { getOperation, getRootOperation } from "./command-spec.types";
import {
  MACHINE_CHANNEL_PROTOCOL_VERSION,
  WORKFLOW_COMPLETION_CONFIRMATION_KIND,
  type MachineChannelAddress,
  type MachineChannelRequestEnvelope,
  type MachineChannelResponseEnvelope,
  type WorkflowCompletionConfirmation,
} from "../types/machineChannel";
import { selectCurrentWorkspaceRoot } from "../workspaceRoot";

export interface ExoRunInput {
  command: string;
  args?: string[];
  workflowConfirmation?: {
    kind: string;
    entityType: string;
    entityId: string;
    decision:
      | "yes_complete"
      | "revise_outcome"
      | "not_complete_yet"
      | "discuss";
    outcome: string;
  };
}

const OUTCOME_REVIEW_CONFIRMATION_ALIAS = "outcome_review";

const logger = getLogger("lmtool");
const ROOT_COMMANDS = new Set(["status", "version"]);

/**
 * Extract agent session identity from the tool invocation token.
 *
 * At runtime, the opaque ChatParticipantToolToken is an IToolInvocationContext
 * containing a sessionResource URI that uniquely identifies the chat conversation.
 * This is stable across tool calls within the same conversation.
 *
 * Returns undefined if the token is unavailable or doesn't have the expected shape.
 */
function extractAgentId(
  token: vscode.ChatParticipantToolToken | undefined,
): string | undefined {
  if (!token) {
    return undefined;
  }
  // Runtime shape: { sessionResource: URI }
  const candidate = token as unknown as {
    sessionResource?: { toString(): string };
  };
  if (
    candidate.sessionResource &&
    typeof candidate.sessionResource.toString === "function"
  ) {
    return candidate.sessionResource.toString();
  }
  return undefined;
}

/**
 * Enable pastTenseMessage (proposed API: chatParticipantPrivate).
 *
 * When true, prepareInvocation returns a pastTenseMessage that transitions
 * the collapsed title from present tense to past tense on completion.
 * Requires chatParticipantPrivate in enabledApiProposals.
 *
 * Set to true for Insiders/pre-release builds. Marketplace stable builds
 * should set this to false to avoid proposed API checks.
 */
const ENABLE_PROPOSED_CHAT_API = false;

function tokenize(command: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let inSingleQuote = false;
  let inDoubleQuote = false;

  for (let i = 0; i < command.length; i += 1) {
    const char = command[i];

    if (char === "'" && !inDoubleQuote) {
      inSingleQuote = !inSingleQuote;
      continue;
    }

    if (char === '"' && !inSingleQuote) {
      inDoubleQuote = !inDoubleQuote;
      continue;
    }

    if ((char === " " || char === "\t") && !inSingleQuote && !inDoubleQuote) {
      if (current.length > 0) {
        tokens.push(current);
        current = "";
      }
      continue;
    }

    if (char === "\\" && inDoubleQuote) {
      const next = command[i + 1];
      if (next !== undefined) {
        switch (next) {
          case "n":
            current += "\n";
            break;
          case "t":
            current += "\t";
            break;
          case "\\":
            current += "\\";
            break;
          case '"':
            current += '"';
            break;
          default:
            current += "\\" + next;
            break;
        }
        i += 1;
        continue;
      }
    }

    current += char;
  }

  if (inSingleQuote || inDoubleQuote) {
    throw new Error("Unterminated quoted string");
  }

  if (current.length > 0) {
    tokens.push(current);
  }

  return tokens;
}

function substitutePlaceholders(tokens: string[], args: string[]): string[] {
  return tokens.map((token) => {
    if (!token.startsWith("$")) {
      return token;
    }

    const rest = token.slice(1);
    // Must be all digits (e.g., "$1", "$12") — not "$1foo" or "$"
    if (!/^\d+$/.test(rest)) {
      return token;
    }

    const index = Number.parseInt(rest, 10);

    // $0 is invalid (1-indexed)
    if (index === 0) {
      throw new Error("Placeholder $0 is invalid (placeholders are 1-indexed)");
    }

    if (index > args.length) {
      throw new Error(`Placeholder $${index} has no corresponding arg`);
    }

    return args[index - 1];
  });
}

function buildHelpRequest(path: string[]): MachineChannelRequestEnvelope {
  let address: MachineChannelAddress;

  if (path.length === 0) {
    address = { kind: "root" };
  } else if (path.length === 1) {
    address = getRootOperation(path[0])
      ? { kind: "operation", path }
      : { kind: "namespace", path };
  } else {
    address = {
      kind: "operation",
      path: [path[0], path.slice(1).join(".")],
    };
  }

  return {
    protocol_version: MACHINE_CHANNEL_PROTOCOL_VERSION,
    id: `vscode.lmtool.exo-run.help.${randomUUID()}`,
    op: {
      kind: "help",
      params: { address },
    },
  };
}

function parseTokens(tokens: string[]): {
  namespace: string;
  operation: string;
  args: Record<string, unknown>;
} {
  if (tokens.length === 0) {
    throw new Error("Empty command");
  }

  const first = tokens[0];
  let namespace = "";
  let operation = "";
  let index = 0;

  if (tokens.length === 1) {
    operation = first;
    index = 1;
  } else if (first.startsWith("-")) {
    throw new Error("Empty command");
  } else if (ROOT_COMMANDS.has(first)) {
    operation = first;
    index = 1;
  } else if (tokens[1].startsWith("-")) {
    operation = first;
    index = 1;
  } else {
    namespace = first;
    operation = tokens[1];
    index = 2;
  }

  const args: Record<string, unknown> = {};
  const positional: string[] = [];

  while (index < tokens.length) {
    const token = tokens[index];

    if (token.startsWith("--") && token.length > 2) {
      const flag = token.slice(2);
      const equalsIndex = flag.indexOf("=");
      if (equalsIndex >= 0) {
        const name = flag.slice(0, equalsIndex);
        const value = flag.slice(equalsIndex + 1);
        args[name] = value;
      } else {
        const next = tokens[index + 1];
        if (next === undefined) {
          throw new Error(`Flag '${flag}' requires a value`);
        }
        if (next.startsWith("-")) {
          args[flag] = true;
        } else {
          args[flag] = next;
          index += 1;
        }
      }
    } else if (token.startsWith("-") && token.length > 1) {
      const flag = token.slice(1);
      const next = tokens[index + 1];
      if (next === undefined) {
        throw new Error(`Flag '${flag}' requires a value`);
      }
      if (next.startsWith("-")) {
        args[flag] = true;
      } else {
        args[flag] = next;
        index += 1;
      }
    } else {
      positional.push(token);
    }

    index += 1;
  }

  // Map positional args to their actual names from the command spec.
  // Previously hardcoded as "id" and "label", which broke any operation
  // where the positional arg has a different name (e.g., "title" for idea add).
  if (positional.length > 0) {
    const opSpec = namespace ? getOperation(namespace, operation) : undefined;
    const positionalSpecs =
      opSpec?.args.filter((a) => a.kind === "positional") ?? [];

    for (let i = 0; i < positional.length; i++) {
      const argName = positionalSpecs[i]?.id ?? (i === 0 ? "id" : "label");
      args[argName] = positional[i];
    }
  }

  return { namespace, operation, args };
}

function buildCallRequest(
  tokens: string[],
  agentId?: string,
  workflowConfirmation?: ExoRunInput["workflowConfirmation"],
): MachineChannelRequestEnvelope {
  const { namespace, operation, args } = parseTokens(tokens);
  const path = namespace ? [namespace, operation] : [operation];

  const request: MachineChannelRequestEnvelope = {
    protocol_version: MACHINE_CHANNEL_PROTOCOL_VERSION,
    id: `vscode.lmtool.exo-run.call.${randomUUID()}`,
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path },
        input: args,
      },
    },
    agent_id: agentId,
  };

  if (workflowConfirmation) {
    request.workflow_confirmation = {
      kind: normalizeWorkflowConfirmationKind(workflowConfirmation.kind),
      entity_type: workflowConfirmation.entityType,
      entity_id: workflowConfirmation.entityId,
      decision: workflowConfirmation.decision,
      outcome: workflowConfirmation.outcome,
    };
  }

  return request;
}

export function normalizeWorkflowConfirmationKind(
  kind: string,
): typeof WORKFLOW_COMPLETION_CONFIRMATION_KIND {
  if (kind === OUTCOME_REVIEW_CONFIRMATION_ALIAS) {
    return WORKFLOW_COMPLETION_CONFIRMATION_KIND;
  }

  return WORKFLOW_COMPLETION_CONFIRMATION_KIND;
}

function buildPreviewRequest(tokens: string[]): MachineChannelRequestEnvelope {
  const { namespace, operation, args } = parseTokens(tokens);
  const path = namespace ? [namespace, operation] : [operation];

  return {
    protocol_version: MACHINE_CHANNEL_PROTOCOL_VERSION,
    id: `vscode.lmtool.exo-run.preview.${randomUUID()}`,
    op: {
      kind: "preview",
      params: {
        address: { kind: "operation", path },
        input: args,
      },
    },
  };
}

function routeCommand(
  tokens: string[],
  agentId?: string,
  workflowConfirmation?: ExoRunInput["workflowConfirmation"],
): MachineChannelRequestEnvelope {
  if (tokens.length === 0) {
    throw new Error("Empty command");
  }

  if (tokens[0] === "help") {
    return buildHelpRequest(tokens.slice(1));
  }

  return buildCallRequest(tokens, agentId, workflowConfirmation);
}

// ── Response Formatting ──────────────────────────────────────────────

function textResult(
  text: string,
  dataPart?: vscode.LanguageModelDataPart,
): vscode.LanguageModelToolResult {
  const content: Array<
    vscode.LanguageModelTextPart | vscode.LanguageModelDataPart
  > = [new vscode.LanguageModelTextPart(text)];
  if (dataPart) {
    content.push(dataPart);
  }
  return new vscode.LanguageModelToolResult(content);
}

function getWorkflowConfirmation(
  details: unknown,
): WorkflowCompletionConfirmation | undefined {
  if (!details || typeof details !== "object") {
    return undefined;
  }

  const obj = details as Record<string, unknown>;
  const direct = obj.workflow_confirmation;
  const nestedDetails = obj.details;
  const nested =
    nestedDetails && typeof nestedDetails === "object"
      ? (nestedDetails as Record<string, unknown>).workflow_confirmation
      : undefined;
  const candidate = direct ?? nested;

  if (!candidate || typeof candidate !== "object") {
    return undefined;
  }

  const confirmation = candidate as Partial<WorkflowCompletionConfirmation>;
  if (
    confirmation.kind !== "workflow_completion_confirmation" ||
    typeof confirmation.header !== "string" ||
    typeof confirmation.question !== "string" ||
    typeof confirmation.message !== "string" ||
    typeof confirmation.readiness_rationale !== "string" ||
    typeof confirmation.proposed_outcome !== "string" ||
    !Array.isArray(confirmation.options)
  ) {
    return undefined;
  }

  return confirmation as WorkflowCompletionConfirmation;
}

function formatWorkflowConfirmation(
  confirmation: WorkflowCompletionConfirmation,
): string {
  const entityType =
    confirmation.completion_input?.entity_type ??
    confirmation.entity_type ??
    "entity";
  const lines = [
    "Review outcome.",
    "",
    "Approve recording this outcome?",
    "",
    "Outcome:",
    confirmation.proposed_outcome,
  ];

  const evidenceText = formatWorkflowEvidence(confirmation.completion_digest);
  if (evidenceText) {
    lines.push("", evidenceText);
  }

  lines.push(
    "",
    "Follow-up to record:",
    "- State “None” if no follow-up remains, or name the tracked next task.",
    "",
    `Ask: “Record this ${entityType} outcome?”`,
    formatWorkflowOptions(confirmation.options),
    "",
    `Once approved, finish the ${entityType} with the approved outcome.`,
  );

  return lines.join("\n");
}

function formatWorkflowOptions(
  options: WorkflowCompletionConfirmation["options"],
): string {
  if (options.length === 0) {
    return "Options: None returned by the daemon";
  }

  return `Options: ${options.map(formatWorkflowOption).join(" / ")}`;
}

function formatWorkflowOption(
  option: WorkflowCompletionConfirmation["options"][number],
): string {
  if (option.description) {
    return `${option.label} — ${option.description}`;
  }

  return option.label;
}

function toExoRunWorkflowConfirmation(
  input: NonNullable<WorkflowCompletionConfirmation["completion_input"]>,
): NonNullable<ExoRunInput["workflowConfirmation"]> {
  return {
    kind: normalizeWorkflowConfirmationKind(input.kind),
    entityType: input.entity_type,
    entityId: input.entity_id,
    decision: input.decision,
    outcome: input.outcome,
  };
}

function formatHelpResult(result: Record<string, unknown>): string {
  const lines: string[] = [];
  const title = result.title as string | undefined;
  const summary = result.summary as string | undefined;

  if (title) {
    lines.push(`# ${title}`);
  }
  if (summary) {
    lines.push(summary);
  }

  const namespaces = result.namespaces as
    | Array<{ path: string[]; summary: string }>
    | undefined;
  if (namespaces && namespaces.length > 0) {
    lines.push("");
    lines.push("## Namespaces");
    for (const ns of namespaces) {
      const path = ns.path.join(".");
      lines.push(`- **${path}** — ${ns.summary}`);
    }
  }

  const operations = result.operations as
    | Array<{
        path: string;
        effect: string;
        summary: string;
        args?: Array<{
          name: string;
          description: string;
          kind: string;
          value_type: string | { enum: string[] };
          optional: boolean;
          default?: string;
          short?: string;
        }>;
      }>
    | undefined;
  if (operations && operations.length > 0) {
    lines.push("");
    lines.push("## Operations");
    for (const op of operations) {
      lines.push(`- **${op.path}** [${op.effect}] — ${op.summary}`);

      if (op.args && op.args.length > 0) {
        for (const arg of op.args) {
          const req = arg.optional ? "optional" : "required";
          const valueType =
            typeof arg.value_type === "object" && arg.value_type.enum
              ? `enum(${arg.value_type.enum.join("|")})`
              : String(arg.value_type);
          const defaultStr =
            arg.default !== undefined ? ` [default: ${arg.default}]` : "";
          const shortStr = arg.short ? ` (-${arg.short})` : "";
          lines.push(
            `  - \`${arg.name}\`${shortStr} (${arg.kind}, ${valueType}, ${req})${defaultStr} — ${arg.description}`,
          );
        }
      }
    }
  }

  if (lines.length === 0) {
    return JSON.stringify(result, null, 2);
  }

  return lines.join("\n");
}

export function formatCallResult(result: unknown): string {
  if (result === null || result === undefined) {
    return "OK";
  }

  if (typeof result !== "object") {
    return String(result);
  }

  const obj = result as Record<string, unknown>;

  const completionDigestText = formatCompletionDigestText(obj);
  if (completionDigestText && obj.steering) {
    return completionDigestText;
  }

  // Task list
  if (obj.kind === "task.list" && Array.isArray(obj.tasks)) {
    const tasks = obj.tasks as Array<{
      id: string;
      label: string;
      status: string;
    }>;
    if (tasks.length === 0) {
      return "No tasks.";
    }
    const lines = tasks.map((t) => {
      const icon =
        t.status === "completed"
          ? "✅"
          : t.status === "in-progress"
            ? "🔄"
            : "⏳";
      return `${icon} ${t.id} — ${t.label}`;
    });
    return lines.join("\n");
  }

  // Goal list
  if (obj.kind === "goal.list" && Array.isArray(obj.goals)) {
    const goals = obj.goals as Array<{
      id: string;
      label: string;
      status: string;
    }>;
    if (goals.length === 0) {
      return "No goals.";
    }
    const lines = goals.map((g) => {
      const icon =
        g.status === "completed"
          ? "✅"
          : g.status === "abandoned"
            ? "⛔"
            : g.status === "in-progress"
              ? "🔄"
              : "⏳";
      return `${icon} ${g.id} — ${g.label}`;
    });
    return lines.join("\n");
  }

  // Task complete / task start / simple ok
  if (obj.ok === true && obj.kind && typeof obj.kind === "string") {
    const parts: string[] = [`${obj.kind}: OK`];
    if (obj.task_id) {
      parts.push(`Task: ${obj.task_id}`);
    }
    if (obj.message) {
      parts.push(`${obj.message}`);
    }
    return parts.join("\n");
  }

  // RFC list
  if (obj.kind === "rfc.list" && Array.isArray(obj.rfcs)) {
    const rfcs = obj.rfcs as Array<{
      id: string;
      title: string;
      stage: number;
    }>;
    if (rfcs.length === 0) {
      return "No RFCs.";
    }
    return rfcs
      .map((r) => `- [Stage ${r.stage}] ${r.id}: ${r.title}`)
      .join("\n");
  }

  // Fallback: compact JSON
  return JSON.stringify(result, null, 2);
}

function formatCompletionDigestText(
  obj: Record<string, unknown>,
): string | undefined {
  const steering = obj.steering as Record<string, unknown> | undefined;
  const digests = steering?.completion_digests;
  if (!Array.isArray(digests) || digests.length === 0) {
    return undefined;
  }

  return formatCompletionDigests(digests as Array<Record<string, unknown>>);
}

function formatWorkflowEvidence(
  digest: WorkflowCompletionConfirmation["completion_digest"],
): string | undefined {
  if (!digest || !Array.isArray(digest.claims) || digest.claims.length === 0) {
    return undefined;
  }

  const lines = ["Verification / evidence:"];
  for (const claim of digest.claims.slice(0, 3)) {
    const subject = claim.subject || "Completed outcome";
    const body = claim.body ? ` — ${claim.body}` : "";
    lines.push(`- ${subject}${body}`);
  }

  if (digest.claims.length > 3) {
    lines.push(`- …and ${digest.claims.length - 3} more`);
  }

  return lines.join("\n");
}

function formatCompletionDigests(
  digests: Array<Record<string, unknown>>,
): string | undefined {
  const lines = ["Completed outcomes to review:"];
  for (const digest of digests) {
    const entityType = String(digest.entity_type ?? "entity");
    const entityId = String(digest.entity_id ?? "?");
    const claims = digest.claims;
    if (!Array.isArray(claims)) {
      continue;
    }
    for (const claim of claims as Array<Record<string, unknown>>) {
      const subject = String(claim.subject ?? "Completed outcome");
      const body = typeof claim.body === "string" ? claim.body : "";
      lines.push(`• ${entityType} ${entityId}: ${subject}`);
      if (body.length > 0) {
        lines.push(`  ${body}`);
      }
    }
  }

  return lines.length > 1 ? lines.join("\n") : undefined;
}

export function formatErrorResponse(response: MachineChannelResponseEnvelope): {
  text: string;
  workflowConfirmation?: WorkflowCompletionConfirmation;
} {
  const lines: string[] = [];
  const error = response.error as Record<string, unknown> | undefined;
  const workflowConfirmation = getWorkflowConfirmation(error?.details);
  if (workflowConfirmation) {
    return {
      text: formatWorkflowConfirmation(workflowConfirmation),
      workflowConfirmation,
    };
  }

  const message = (error?.message ?? "Command failed") as string;
  lines.push(`Error: ${message}`);

  // Extract diagnostics with suggestions
  const details = error?.details as Record<string, unknown> | undefined;
  const diagnostics = details?.diagnostics as
    | Array<{
        code: string;
        message: string;
        suggestions?: Array<{ label: string; replacement: string }>;
      }>
    | undefined;

  if (diagnostics && diagnostics.length > 0) {
    for (const diag of diagnostics) {
      if (diag.suggestions && diag.suggestions.length > 0) {
        for (const s of diag.suggestions) {
          lines.push(`Suggestion: ${s.label} → ${s.replacement}`);
        }
      }
    }
  }

  // Steering hint
  const steering = response.steering as Record<string, unknown> | undefined;
  if (steering?.next_call) {
    const next = steering.next_call as {
      kind: string;
      params?: Record<string, unknown>;
    };
    if (next.kind === "help") {
      const addr = next.params?.address as
        | { kind: string; path?: string[] }
        | undefined;
      if (addr?.kind === "root") {
        lines.push("\nTry: help");
      } else if (addr?.path) {
        lines.push(`\nTry: help ${addr.path.join(" ")}`);
      }
    }
  }

  return { text: lines.join("\n"), workflowConfirmation };
}

export function formatMachineChannelResponse(
  response: MachineChannelResponseEnvelope,
  isHelp: boolean,
): vscode.LanguageModelToolResult {
  if (response.status === "ok") {
    let text: string;

    // Prefer server-generated display metadata when available
    if (response.display) {
      text = response.display.body ?? response.display.summary;
    } else if (
      isHelp &&
      response.result &&
      typeof response.result === "object"
    ) {
      text = formatHelpResult(response.result as Record<string, unknown>);
    } else {
      text = formatCallResult(response.result ?? null);
    }

    // Append reminders if present
    const reminders = response.reminders;
    if (reminders && reminders.length > 0) {
      text +=
        "\n\n---\nReminders:\n" +
        reminders.map((r) => `- [${r.severity}] ${r.message}`).join("\n");
    }

    // Pass through full steering data when present
    const steering = response.steering;
    let steeringPart: vscode.LanguageModelDataPart | undefined;

    if (steering) {
      if (typeof vscode.LanguageModelDataPart?.json === "function") {
        steeringPart = vscode.LanguageModelDataPart.json(steering);
      } else {
        // Fallback: append as JSON code block if data parts unavailable
        const steeringJson = JSON.stringify(steering, null, 2);
        text += `\n\n---\nSteering JSON:\n\`\`\`json\n${steeringJson}\n\`\`\``;
      }
    }

    return textResult(text, steeringPart);
  }

  const { text, workflowConfirmation } = formatErrorResponse(response);
  let workflowPart: vscode.LanguageModelDataPart | undefined;
  if (
    workflowConfirmation &&
    typeof vscode.LanguageModelDataPart?.json === "function"
  ) {
    workflowPart = vscode.LanguageModelDataPart.json({
      workflow_confirmation: workflowConfirmation,
      workflowConfirmation: workflowConfirmation.completion_input
        ? toExoRunWorkflowConfirmation(workflowConfirmation.completion_input)
        : undefined,
    });
  }
  return textResult(text, workflowPart);
}

function errorResult(message: string): vscode.LanguageModelToolResult {
  return textResult(`Error: ${message}\n\nTry: help`);
}

export function createExoRunTool(): vscode.LanguageModelTool<ExoRunInput> {
  return {
    async prepareInvocation(
      options: vscode.LanguageModelToolInvocationPrepareOptions<ExoRunInput>,
      _token: vscode.CancellationToken,
    ): Promise<vscode.PreparedToolInvocation> {
      const command = options.input?.command?.trim() ?? "";
      const fallbackMessage = command
        ? `Running: ${command}`
        : "Running exo command...";

      // Skip preview for help commands — they're just documentation lookups
      if (!command || command.startsWith("help")) {
        return { invocationMessage: fallbackMessage };
      }

      const rootPath = selectCurrentWorkspaceRoot().rootPath;
      if (!rootPath) {
        return { invocationMessage: fallbackMessage };
      }

      try {
        const tokens = tokenize(command);
        if (tokens.length === 0) {
          return { invocationMessage: fallbackMessage };
        }

        const substituted = substitutePlaceholders(
          tokens,
          options.input?.args ?? [],
        );
        const request = buildPreviewRequest(substituted);

        logger.debug(`[exo-run] Requesting preview for: ${command}`);
        const response = await exoMachineChannel(rootPath, request);

        if (response.status === "ok" && response.preview?.invocation_message) {
          const invocationMessage = new vscode.MarkdownString(
            response.preview.invocation_message,
          );
          invocationMessage.supportThemeIcons = true;

          const result: vscode.PreparedToolInvocation = { invocationMessage };

          // Progressive enhancement: pastTenseMessage transitions the collapsed
          // title from present tense ("Completing task...") to past tense
          // ("Completed task...") when the tool finishes. Requires the
          // chatParticipantPrivate proposed API (Insiders only).
          if (ENABLE_PROPOSED_CHAT_API && response.preview.past_tense_message) {
            const pastTenseMessage = new vscode.MarkdownString(
              response.preview.past_tense_message,
            );
            pastTenseMessage.supportThemeIcons = true;
            // pastTenseMessage is a proposed API property — not in stable typings.
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
            (result as any).pastTenseMessage = pastTenseMessage;
          }

          if (response.preview.confirmation) {
            result.confirmationMessages = {
              title: response.preview.confirmation.title,
              message: new vscode.MarkdownString(
                response.preview.confirmation.message,
              ),
            };
          }

          return result;
        }

        // Server returned ok but no preview — fall back
        return { invocationMessage: fallbackMessage };
      } catch (err) {
        // Preview failure must not block invocation — fall back silently
        logger.debug(`[exo-run] Preview failed, using fallback: ${err}`);
        return { invocationMessage: fallbackMessage };
      }
    },

    async invoke(
      options: vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      _token: vscode.CancellationToken,
    ): Promise<vscode.LanguageModelToolResult> {
      const input = options.input;
      if (!input?.command || input.command.trim().length === 0) {
        return errorResult("Missing command string");
      }

      const workspaceSelection = selectCurrentWorkspaceRoot();
      const rootPath = workspaceSelection.rootPath;
      if (!rootPath) {
        return errorResult(
          `No usable Exosuit workspace root: ${workspaceSelection.reason}`,
        );
      }

      try {
        const tokens = tokenize(input.command);
        if (tokens.length === 0) {
          return errorResult("Empty command");
        }

        const agentId = extractAgentId(options.toolInvocationToken);
        const substituted = substitutePlaceholders(tokens, input.args ?? []);
        const request = routeCommand(
          substituted,
          agentId,
          input.workflowConfirmation,
        );

        logger.debug(
          `[exo-run] Dispatching machine channel request: ${request.op.kind}`,
        );

        const response = await exoMachineChannel(rootPath, request);
        const isHelp = request.op.kind === "help";
        return formatMachineChannelResponse(response, isHelp);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        return errorResult(message);
      }
    },
  };
}
