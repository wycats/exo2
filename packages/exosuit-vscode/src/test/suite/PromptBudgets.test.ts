import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import type * as vscode from "vscode";
import { ToolRegistry } from "../../agent/ToolRegistry";

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

describe("Prompt Budgets", () => {
  it("readFile truncates very large files", async () => {
    const registry = new ToolRegistry();
    const stream = new MockChatResponseStream();

    const tool = registry.get("readFile");
    assert.ok(tool, "readFile should be registered");

    const original = "a".repeat(200_100);
    const tmpFile = path.join(os.tmpdir(), `exosuit-readfile-budget-${Date.now()}.txt`);
    fs.writeFileSync(tmpFile, original, "utf8");

    try {
      const out = await tool!.execute({ path: tmpFile }, stream as any);
      assert.ok(out, "Expected tool output");
      assert.ok(
        out!.includes("TRUNCATED readFile output"),
        "Expected truncation notice"
      );
      assert.ok(out!.length < original.length, "Expected output to be shorter");
    } finally {
      try {
        fs.unlinkSync(tmpFile);
      } catch {
        // ignore
      }
    }
  });
});
