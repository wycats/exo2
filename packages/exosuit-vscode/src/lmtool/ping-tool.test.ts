import { describe, expect, it } from "vitest";
import type * as vscode from "vscode";

import { createPingTool } from "./ping-tool";

function firstTextValue(result: vscode.LanguageModelToolResult): string {
  const first = result.content[0];
  if (!first || typeof first !== "object" || !("value" in first)) {
    throw new Error("Expected first tool result part to contain text");
  }
  return String(first.value);
}

describe("createPingTool", () => {
  it("includes extension identity in the diagnostic payload", async () => {
    const tool = createPingTool({
      buildStamp: "2026-05-12T18:00:00Z",
      pid: 12345,
      extensionPath: "/workspace/packages/exosuit-vscode",
      extensionUri: "file:///workspace/packages/exosuit-vscode",
      extensionMode: "Development",
      extensionKind: "Workspace",
    });

    const result = await tool.invoke(
      { input: {}, toolInvocationToken: undefined },
      {} as never,
    );
    if (!result) {
      throw new Error("Expected ping tool to return a result");
    }
    const text = firstTextValue(result);

    expect(text).toContain("pong. build: 2026-05-12T18:00:00Z.");
    expect(text).toContain("pid: 12345.");
    expect(text).toContain(
      "extensionPath: /workspace/packages/exosuit-vscode.",
    );
    expect(text).toContain(
      "extensionUri: file:///workspace/packages/exosuit-vscode.",
    );
    expect(text).toContain("extensionMode: Development.");
    expect(text).toContain("extensionKind: Workspace.");
  });
});
