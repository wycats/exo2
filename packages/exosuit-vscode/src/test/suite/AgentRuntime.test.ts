import { describe, it, beforeEach } from "./harness.js";
import * as assert from "assert";
import type * as vscode from "vscode";
import { AgentRuntime } from "../../agent/AgentRuntime";
import { ToolRegistry } from "../../agent/ToolRegistry";
import { WorkspaceCache } from "../../WorkspaceCache";
import type { LogService } from "../../LogService";

describe("AgentRuntime Test Suite", () => {
  let runtime: AgentRuntime;
  let toolRegistry: ToolRegistry;
  let workspaceCache: WorkspaceCache;
  let logger: LogService;

  beforeEach(() => {
    toolRegistry = new ToolRegistry();
    workspaceCache = new WorkspaceCache();
    logger = {
      logActivity: () => {},
      logError: () => {},
      logDebug: () => {},
      show: () => {},
    } as any;

    runtime = new AgentRuntime({
      toolRegistry,
      workspaceCache,
      logger,
    });
  });

  it("Instantiates correctly", () => {
    assert.ok(runtime);
  });

  it("handleRequest sends prompt to model", async () => {
    let receivedMessages: vscode.LanguageModelChatMessage[] = [];

    const mockModel: any = {
      name: "mock-model",
      sendRequest: async (
        messages: vscode.LanguageModelChatMessage[],
        _options: any,
        _token: any
      ) => {
        receivedMessages = [...messages];
        return {
          text: (async function* () {
            yield "Response";
          })(),
        };
      },
    };

    const request: any = {
      prompt: "Hello",
      model: mockModel,
      command: "",
      toolInvocationToken: {} as any,
    };

    const context: any = {
      history: [],
    };

    const stream: any = {
      markdown: () => {},
      progress: () => {},
    };

    const token: any = {
      isCancellationRequested: false,
      onCancellationRequested: () => ({ dispose: () => {} }),
    };

    await runtime.handleRequest(request, context, stream, token, {
      systemPrompt: "System Prompt",
    });

    assert.strictEqual(receivedMessages.length, 2); // System + User
    assert.strictEqual(
      (receivedMessages[0].content[0] as any).value,
      "System Prompt"
    );
    assert.strictEqual((receivedMessages[1].content[0] as any).value, "Hello");
  });
});
