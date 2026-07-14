import type * as vscode from "vscode";
import { beforeEach, describe, expect, it, vi } from "vitest";

import manifest from "../../package.json";
import {
  WORKFLOW_COMPLETION_CONFIRMATION_KIND,
  type MachineChannelRequestEnvelope,
} from "../types/machineChannel";

const machineChannelMock = vi.hoisted(() => vi.fn());

vi.mock("../agent/lmtool/machineChannel", () => ({
  exoMachineChannel: machineChannelMock,
}));

vi.mock("../workspaceRoot", () => ({
  selectCurrentWorkspaceRoot: () => ({
    rootPath: "/workspace",
    reason: "test workspace",
    candidates: ["/workspace"],
  }),
}));

import {
  createExoRunTool,
  normalizeWorkflowConfirmationKind,
  type ExoRunInput,
} from "./exo-run";

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
  });

  it("constrains workflowConfirmation.kind to the canonical protocol kind", () => {
    expect(workflowKindSchema()).toMatchObject({
      type: "string",
      const: WORKFLOW_COMPLETION_CONFIRMATION_KIND,
    });
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
});
