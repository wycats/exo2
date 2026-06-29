import { describe, it, beforeEach } from "./harness.js";
import * as assert from "assert";
import type * as vscode from "vscode";
import { ToolRegistry } from "../../agent/ToolRegistry";

// Mock Stream
class MockChatResponseStream implements vscode.ChatResponseStream {
  public output: string = "";
  public parts: any[] = [];

  markdown(value: string | vscode.MarkdownString): void {
    const text = typeof value === "string" ? value : value.value;
    this.output += text;
    this.parts.push({ type: "markdown", value: text });
  }

  progress(value: string): void {
    this.parts.push({ type: "progress", value });
  }

  push(part: vscode.ChatResponsePart): void {
    this.parts.push(part);
  }

  reference(value: vscode.Uri | vscode.Location): void {
    this.parts.push({ type: "reference", value });
  }

  text(value: string): void {
    this.output += value;
    this.parts.push({ type: "text", value });
  }

  anchor(value: vscode.Uri | vscode.Location, title?: string): void {
    this.parts.push({ type: "anchor", value, title });
  }

  button(command: vscode.Command): void {
    this.parts.push({ type: "button", command });
  }

  filetree(tree: vscode.ChatResponseFileTree[], baseUri: vscode.Uri): void {
    this.parts.push({ type: "filetree", tree, baseUri });
  }
}

describe("SysHelp Test Suite", () => {
  let registry: ToolRegistry;
  let stream: MockChatResponseStream;

  beforeEach(() => {
    registry = new ToolRegistry();
    stream = new MockChatResponseStream();
  });

  it("sys.help returns tool definitions", async () => {
    const tool = registry.get("sys.help");
    assert.ok(tool, "sys.help should be registered");

    const output = await tool!.execute({}, stream as any);
    assert.ok(output, "Output should not be empty");
    assert.ok(output!.includes("sys.help"), "Output should include sys.help");
    assert.ok(
      output!.includes("readFile"),
      "Output should include other tools"
    );
  });
});
