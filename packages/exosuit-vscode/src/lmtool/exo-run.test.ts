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

describe("exo-run workflow confirmation", () => {
  beforeEach(() => {
    machineChannelMock.mockReset();
    machineChannelMock.mockResolvedValue({
      protocol_version: 1,
      id: "test.response",
      status: "ok",
      result: { ok: true, kind: "task.complete" },
    });
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

  it("publishes hidden execution approval and workspace selection inputs", () => {
    const exoRunTool = manifest.contributes.languageModelTools.find(
      (tool) => tool.name === "exo-run",
    );
    const properties = exoRunTool?.inputSchema.properties as
      | Record<string, Record<string, unknown>>
      | undefined;

    expect(properties?.workspaceRoot).toMatchObject({ type: "string" });
    expect(properties?.auth).toMatchObject({ type: "object" });
    expect(
      (properties?.auth?.properties as Record<string, unknown>)?.confirm,
    ).toMatchObject({ const: true });
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

    expect(machineChannelMock).toHaveBeenCalledTimes(1);
    const request = machineChannelMock.mock.calls[0]?.[1] as
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

    expect(machineChannelMock).toHaveBeenCalledTimes(1);
    const request = machineChannelMock.mock.calls[0]?.[1] as
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

  it("addresses root operations from CommandSpec", async () => {
    const tool = createExoRunTool();

    await tool.invoke(
      {
        input: { command: "write notes.md" },
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
        input: { path: "notes.md" },
      },
    });
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
    }
  });

  it("uses the same dotted address for preview", async () => {
    machineChannelMock.mockResolvedValueOnce({
      protocol_version: 1,
      id: "preview.response",
      status: "ok",
      preview: { invocation_message: "Checking documentation links" },
    });
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
          auth: { ticket: "opaque-ticket", confirm: true },
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
