import type * as vscode from "vscode";
import { beforeEach, describe, expect, it, vi } from "vitest";

import manifest from "../../package.json";

const machineChannelMock = vi.hoisted(() => vi.fn());
const workspaceSelectionMock = vi.hoisted(() => vi.fn());

vi.mock("../agent/lmtool/machineChannel", () => ({
  exoMachineChannel: machineChannelMock,
}));

vi.mock("../workspaceRoot", () => ({
  selectCurrentLmToolWorkspaceRoot: workspaceSelectionMock,
}));

import { createChatHistoryTool } from "./chat-history-tool";

describe("exo-ai-chat-history workspace selection", () => {
  beforeEach(() => {
    machineChannelMock.mockReset();
    machineChannelMock.mockResolvedValue({
      protocol_version: 1,
      id: "chat-history.response",
      status: "error",
      error: { message: "No history" },
    });
    workspaceSelectionMock.mockReset();
    workspaceSelectionMock.mockReturnValue({
      rootPath: "/workspace",
      reason: "test workspace",
      candidates: ["/workspace"],
    });
  });

  it("publishes an explicit workspaceRoot selector", () => {
    const tool = manifest.contributes.languageModelTools.find(
      (candidate) => candidate.name === "exo-ai-chat-history",
    );

    expect(tool?.inputSchema.properties.workspaceRoot).toMatchObject({
      type: "string",
    });
  });

  it("routes through an explicitly selected open workspace root", async () => {
    workspaceSelectionMock.mockReturnValueOnce({
      rootPath: "/workspace/two",
      reason: "requested open workspace folder",
      candidates: ["/workspace/one", "/workspace/two"],
    });
    const tool = createChatHistoryTool();

    await tool.invoke(
      {
        input: { workspaceRoot: "/workspace/two" },
        toolInvocationToken: undefined,
      } as vscode.LanguageModelToolInvocationOptions<{
        workspaceRoot?: string;
      }>,
      {} as never,
    );

    expect(workspaceSelectionMock).toHaveBeenCalledWith("/workspace/two");
    expect(machineChannelMock).toHaveBeenCalledWith(
      "/workspace/two",
      expect.any(Object),
    );
  });
});
