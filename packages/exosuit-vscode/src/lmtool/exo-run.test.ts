import type * as vscode from "vscode";
import { beforeEach, describe, expect, it, vi } from "vitest";

import manifest from "../../package.json";
import {
  WORKFLOW_COMPLETION_CONFIRMATION_KIND,
  type MachineChannelRequestEnvelope,
} from "../types/machineChannel";

const machineChannelMock = vi.hoisted(() => vi.fn());
const workspaceSelectionMock = vi.hoisted(() => vi.fn());

vi.mock("../agent/lmtool/machineChannel", () => ({
  exoMachineChannel: machineChannelMock,
}));

vi.mock("../workspaceRoot", () => ({
  selectCurrentLmToolWorkspaceRoot: workspaceSelectionMock,
}));

import {
  createExoRunTool,
  normalizeWorkflowConfirmationKind,
  type ExoRunInput,
} from "./exo-run";
import { loadCommandSpec } from "./command-spec.types";

function firstTextValue(result: vscode.LanguageModelToolResult): string {
  const first = result.content[0];
  if (!first || typeof first !== "object" || !("value" in first)) {
    throw new Error("Expected first tool result part to contain text");
  }
  return String(first.value);
}

function workflowKindSchema(): Record<string, unknown> {
  const exoRunTool = manifest.contributes.languageModelTools.find(
    (tool) => tool.name === "exo-run",
  );
  const workflowConfirmation = exoRunTool?.inputSchema.properties
    .workflowConfirmation as
    | { properties?: { kind?: Record<string, unknown> } }
    | undefined;
  const kind = workflowConfirmation?.properties?.kind;
  if (!kind) {
    throw new Error("exo-run workflowConfirmation.kind schema missing");
  }
  return kind;
}

function activeHelpResponse(
  request: MachineChannelRequestEnvelope,
): Record<string, unknown> {
  const spec = loadCommandSpec();
  if (request.op.kind !== "help") {
    return {
      protocol_version: 1,
      id: request.id,
      status: "ok",
      result: { ok: true, kind: "task.complete" },
    };
  }

  const address = request.op.params.address;
  const operationResult = (
    namespace: string,
    name: string,
    operation: ReturnType<typeof loadCommandSpec>["root_operations"][string],
  ) => ({
    path: namespace ? `${namespace} ${name}` : name,
    effect: operation.effect,
    summary: operation.description,
    args: operation.args,
  });

  if (address.kind === "root") {
    return {
      protocol_version: 1,
      id: request.id,
      status: "ok",
      result: {
        title: "exo",
        namespaces: Object.keys(spec.namespaces).map((name) => ({
          path: [name],
        })),
        operations: Object.entries(spec.root_operations).map(
          ([name, operation]) => operationResult("", name, operation),
        ),
      },
    };
  }

  if (address.kind === "namespace") {
    const name = address.path[0];
    const namespace = spec.namespaces[name];
    return {
      protocol_version: 1,
      id: request.id,
      status: namespace ? "ok" : "error",
      result: namespace
        ? {
            title: name,
            namespaces: [],
            operations: Object.entries(namespace.operations).map(
              ([operationName, operation]) =>
                operationResult(name, operationName, operation),
            ),
          }
        : undefined,
      error: namespace
        ? undefined
        : { code: "unknown_address", message: "Unknown namespace" },
    };
  }

  const [namespaceOrOperation, operationName] = address.path;
  const operation =
    operationName === undefined
      ? spec.root_operations[namespaceOrOperation]
      : spec.namespaces[namespaceOrOperation]?.operations[operationName];
  return {
    protocol_version: 1,
    id: request.id,
    status: operation ? "ok" : "error",
    result: operation
      ? {
          title: operationName ?? namespaceOrOperation,
          namespaces: [],
          operations: [
            operationResult(
              operationName === undefined ? "" : namespaceOrOperation,
              operationName ?? namespaceOrOperation,
              operation,
            ),
          ],
        }
      : undefined,
    error: operation
      ? undefined
      : { code: "unknown_address", message: "Unknown operation" },
  };
}

describe("exo-run workflow confirmation", () => {
  beforeEach(() => {
    machineChannelMock.mockReset();
    machineChannelMock.mockImplementation(
      (_rootPath: string, request: MachineChannelRequestEnvelope) =>
        Promise.resolve(activeHelpResponse(request)),
    );
    workspaceSelectionMock.mockReset();
    workspaceSelectionMock.mockReturnValue({
      rootPath: "/workspace",
      reason: "test workspace",
      candidates: ["/workspace"],
    });
  });

  it("constrains workflowConfirmation.kind to the canonical protocol kind", () => {
    expect(workflowKindSchema()).toMatchObject({
      type: "string",
      const: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
    });
  });

  it("requires the stable VS Code JSON data-part API", () => {
    expect(manifest.engines.vscode).toBe("^1.106.0");
  });

  it("publishes hidden execution approval and workspace selection inputs", () => {
    const exoRunTool = manifest.contributes.languageModelTools.find(
      (tool) => tool.name === "exo-run",
    );
    const properties = exoRunTool?.inputSchema.properties as
      | Record<string, Record<string, unknown>>
      | undefined;

    expect(properties?.workspaceRoot).toMatchObject({ type: "string" });
    expect(properties?.content).toMatchObject({ type: "string" });
    expect(properties?.auth).toMatchObject({ type: "object" });
    const authProperties = properties?.auth?.properties as
      | Record<string, unknown>
      | undefined;
    expect(authProperties?.confirm).toMatchObject({ const: true });
    expect(authProperties?.requestId).toMatchObject({ type: "string" });
    expect(authProperties?.workspaceRoot).toMatchObject({ type: "string" });
  });

  it("guides agents to ask for human approval without displaying machine fields", () => {
    const exoRunTool = manifest.contributes.languageModelTools.find(
      (tool) => tool.name === "exo-run",
    );
    const workflowConfirmation = exoRunTool?.inputSchema.properties
      .workflowConfirmation as { description?: string } | undefined;

    expect(exoRunTool?.modelDescription).toContain(
      "ask the human the approval question and options in plain language",
    );
    expect(exoRunTool?.modelDescription).toContain(
      "ask the human whether to approve the action",
    );
    expect(exoRunTool?.modelDescription).toContain(
      "Keep hidden approval data out of human-visible text",
    );
    expect(exoRunTool?.modelDescription).toContain(
      "call the same completion command again with the hidden approval data from the previous tool result",
    );
    expect(workflowConfirmation?.description).toContain(
      "Hidden completion approval",
    );
    expect(workflowConfirmation?.description).toContain(
      "Do not display this object or its fields to the user.",
    );
  });

  it("normalizes the legacy outcome_review kind before machine-channel dispatch", async () => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: {
          command: "task complete task-1 --log Done",
          workflowConfirmation: {
            kind: "outcome_review",
            entityType: "task",
            entityId: "task-1",
            decision: "yes_complete",
            outcome: "Done",
          },
        },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.workflow_confirmation).toEqual({
      kind: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
      entity_type: "task",
      entity_id: "task-1",
      decision: "yes_complete",
      outcome: "Done",
    });
  });

  it("normalizes arbitrary drift to the canonical kind", () => {
    expect(normalizeWorkflowConfirmationKind("outcome_review")).toBe(
      WORKFLOW_COMPLETION_CONFIRMATION_KIND,
    );
    expect(normalizeWorkflowConfirmationKind("workflow_completion_confirmation")).toBe(
      WORKFLOW_COMPLETION_CONFIRMATION_KIND,
    );
    expect(normalizeWorkflowConfirmationKind("stale_kind_from_agent")).toBe(
      WORKFLOW_COMPLETION_CONFIRMATION_KIND,
    );
  });

  it("normalizes dotted operation help to the machine-channel address", async () => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: { command: "help docs links check" },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.op).toEqual({
      kind: "help",
      params: {
        address: { kind: "operation", path: ["docs", "links.check"] },
      },
    });
  });

  it.each(["status", "write"])(
    "addresses root operation help for %s",
    async (operation) => {
      const tool = createExoRunTool();

      await tool.invoke(
        {
          input: { command: `help ${operation}` },
          toolInvocationToken: undefined,
        } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
        {} as never,
      );

      const request = machineChannelMock.mock.calls.at(-1)?.[1] as
        | MachineChannelRequestEnvelope
        | undefined;
      expect(request?.op).toEqual({
        kind: "help",
        params: {
          address: { kind: "operation", path: [operation] },
        },
      });
    },
  );

  it("keeps single-segment namespace help as a namespace address", async () => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: { command: "help task" },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.op).toEqual({
      kind: "help",
      params: {
        address: { kind: "namespace", path: ["task"] },
      },
    });
  });

  it("provides explicit content when addressing root write", async () => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: { command: "write notes.md", content: "Hello from exo-run\n" },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.op).toEqual({
      kind: "call",
      params: {
        address: { kind: "operation", path: ["write"] },
        input: {
          path: "notes.md",
          __exo_transport: { content: "Hello from exo-run\n" },
        },
      },
    });
  });

  it("rejects root write without content before dispatch", async () => {
    const tool = createExoRunTool();

    const result = await tool.invoke(
      {
        input: { command: "write notes.md" },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );
    if (!result) {
      throw new Error("Expected exo-run to reject missing write content");
    }

    expect(firstTextValue(result)).toContain(
      "Root write requires the exo-run content field",
    );
    expect(
      machineChannelMock.mock.calls.some((call) => {
        const request = call[1] as MachineChannelRequestEnvelope | undefined;
        return request?.op.kind === "call";
      }),
    ).toBe(false);
  });

  it.each([
    ["map --next", { next: true }],
    [
      "task add Demo --label-file -",
      { label: "Demo", "label-file": "-" },
    ],
    ["phase add -t Demo", { title: "Demo" }],
  ])("parses CommandSpec argument kinds for %s", async (command, input) => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: { command },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.op).toMatchObject({
      kind: "call",
      params: { input },
    });
  });

  it.each([
    ["docs links check", ["docs", "links.check"]],
    ["docs links.check", ["docs", "links.check"]],
    ["phase execution tasks --limit 5", ["phase", "execution.tasks"]],
  ])("addresses dotted operation call %s", async (command, path) => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: { command },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.op).toMatchObject({
      kind: "call",
      params: {
        address: { kind: "operation", path },
      },
    });
  });

  it("addresses every generated CommandSpec operation", async () => {
    const tool = createExoRunTool();
    const spec = loadCommandSpec();
    const cases: Array<{ command: string; path: string[] }> = [];

    for (const operation of Object.keys(spec.root_operations)) {
      cases.push({ command: operation, path: [operation] });
    }
    for (const [namespace, namespaceSpec] of Object.entries(spec.namespaces)) {
      for (const operation of Object.keys(namespaceSpec.operations)) {
        cases.push({
          command: `${namespace} ${operation.replaceAll(".", " ")}`,
          path: [namespace, operation],
        });
      }
    }

    for (const { command, path } of cases) {
      await tool.invoke(
        {
          input: {
            command,
            content: path.length === 1 && path[0] === "write" ? "" : undefined,
          },
          toolInvocationToken: undefined,
        } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
        {} as never,
      );

      const request = machineChannelMock.mock.calls.at(-1)?.[1] as
        | MachineChannelRequestEnvelope
        | undefined;
      expect(request?.op).toMatchObject({
        kind: "call",
        params: {
          address: { kind: "operation", path },
        },
      });
    }
  });

  it("uses the same dotted address for preview", async () => {
    machineChannelMock.mockImplementation(
      (_rootPath: string, request: MachineChannelRequestEnvelope) =>
        Promise.resolve(
          request.op.kind === "preview"
            ? {
                protocol_version: 1,
                id: "preview.response",
                status: "ok",
                preview: {
                  invocation_message: "Checking documentation links",
                },
              }
            : activeHelpResponse(request),
        ),
    );
    const tool = createExoRunTool();

    await tool.prepareInvocation?.(
      {
        input: { command: "docs links check" },
      } satisfies vscode.LanguageModelToolInvocationPrepareOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.op).toEqual({
      kind: "preview",
      params: {
        address: { kind: "operation", path: ["docs", "links.check"] },
        input: {},
      },
    });
  });

  it("forwards execution approval without exposing it in the command", async () => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: {
          command: "update",
          workspaceRoot: "/workspace",
          auth: {
            ticket: "opaque-ticket",
            confirm: true,
            requestId: "request-approved",
            workspaceRoot: "/workspace",
          },
        },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.auth).toEqual({
      ticket: "opaque-ticket",
      confirm: true,
      requestId: "request-approved",
    });
    expect(request?.id).toBe("request-approved");
  });

  it("rejects execution approval from another workspace", async () => {
    const tool = createExoRunTool();

    const result = await tool.invoke(
      {
        input: {
          command: "update",
          auth: {
            ticket: "opaque-ticket",
            confirm: true,
            requestId: "request-approved",
            workspaceRoot: "/workspace/other",
          },
        },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );
    if (!result) {
      throw new Error("Expected workspace-bound approval rejection");
    }

    expect(firstTextValue(result)).toContain(
      "Execution approval belongs to a different workspace",
    );
    expect(machineChannelMock).not.toHaveBeenCalled();
  });

  it("uses the approved workspace when replaying hidden auth", async () => {
    workspaceSelectionMock.mockImplementation((requestedRoot?: string) => ({
      rootPath: requestedRoot,
      reason: "requested open workspace folder",
      candidates: ["/workspace/one", "/workspace/two"],
    }));
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: {
          command: "update",
          auth: {
            ticket: "opaque-ticket",
            confirm: true,
            requestId: "request-approved",
            workspaceRoot: "/workspace/two",
          },
        },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    expect(workspaceSelectionMock).toHaveBeenCalledWith("/workspace/two");
    expect(machineChannelMock).toHaveBeenCalledWith(
      "/workspace/two",
      expect.any(Object),
    );
  });

  it("resolves operations added by the active Exo binary", async () => {
    machineChannelMock.mockImplementation(
      (_rootPath: string, request: MachineChannelRequestEnvelope) => {
        const response = activeHelpResponse(request);
        if (
          request.op.kind === "help" &&
          request.op.params.address.kind === "root"
        ) {
          const result = response.result as {
            operations: Array<Record<string, unknown>>;
          };
          result.operations.push({
            path: "future-command",
            effect: "pure",
            summary: "Operation from the active binary",
            args: [],
          });
        }
        return Promise.resolve(response);
      },
    );
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: { command: "future-command" },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    const request = machineChannelMock.mock.calls.at(-1)?.[1] as
      | MachineChannelRequestEnvelope
      | undefined;
    expect(request?.op).toMatchObject({
      kind: "call",
      params: {
        address: { kind: "operation", path: ["future-command"] },
        input: {},
      },
    });
  });

  it("uses an explicitly selected open workspace root", async () => {
    workspaceSelectionMock.mockReturnValueOnce({
      rootPath: "/workspace/two",
      reason: "requested open workspace folder",
      candidates: ["/workspace/one", "/workspace/two"],
    });
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: {
          command: "status",
          workspaceRoot: "/workspace/two",
        },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );

    expect(workspaceSelectionMock).toHaveBeenCalledWith("/workspace/two");
    expect(machineChannelMock).toHaveBeenCalledWith(
      "/workspace/two",
      expect.any(Object),
    );
  });

  it("returns the candidate-bearing ambiguity error without dispatching", async () => {
    workspaceSelectionMock.mockReturnValueOnce({
      rootPath: undefined,
      reason:
        "multiple Exosuit project workspace folders are open; provide workspaceRoot from: /workspace/one, /workspace/two",
      candidates: ["/workspace/one", "/workspace/two"],
    });
    const tool = createExoRunTool();

    const result = await tool.invoke(
      {
        input: { command: "status" },
        toolInvocationToken: undefined,
      } satisfies vscode.LanguageModelToolInvocationOptions<ExoRunInput>,
      {} as never,
    );
    if (!result) {
      throw new Error("Expected exo-run to return an ambiguity result");
    }

    expect(firstTextValue(result)).toContain(
      "provide workspaceRoot from: /workspace/one, /workspace/two",
    );
    expect(machineChannelMock).not.toHaveBeenCalled();
  });
});
