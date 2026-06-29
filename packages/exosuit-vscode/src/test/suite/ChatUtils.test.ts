import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";
import {
  formatHiddenToolOutput,
  createToolResponseMessages,
} from "../../agent/ChatUtils";

describe("ChatUtils Test Suite", () => {
  it("formatHiddenToolOutput returns MarkdownString with supportHtml=true", () => {
    const outputs = [{ name: "testTool", output: "some output" }];
    const result = formatHiddenToolOutput(outputs);

    assert.ok(
      result instanceof vscode.MarkdownString,
      "Result should be a MarkdownString"
    );
    assert.strictEqual(
      result.supportHtml,
      true,
      "supportHtml should be true to hide the comment"
    );
  });

  it("formatHiddenToolOutput escapes double hyphens", () => {
    const outputs = [
      { name: "testTool", output: "output with -- double dash" },
    ];
    const result = formatHiddenToolOutput(outputs);

    assert.ok(
      result.value.includes("&#45;&#45;"),
      "Double hyphens should be escaped"
    );
    assert.ok(
      !result.value.includes("output with --"),
      "Raw double hyphens in content should not be present"
    );
  });

  it("formatHiddenToolOutput escapes closing angle brackets", () => {
    const outputs = [{ name: "testTool", output: "output with > bracket" }];
    const result = formatHiddenToolOutput(outputs);

    assert.ok(
      result.value.includes("&gt;"),
      "Closing angle brackets should be escaped"
    );
  });

  it("formatHiddenToolOutput wraps content in XML comment", () => {
    const outputs = [{ name: "testTool", output: "simple output" }];
    const result = formatHiddenToolOutput(outputs);

    assert.ok(
      result.value.startsWith("\n<!-- EXO_TOOL_OUTPUT"),
      "Should start with comment tag"
    );
    assert.ok(result.value.endsWith("\n-->"), "Should end with comment tag");
  });

  it("createToolResponseMessages returns two messages", () => {
    const outputs = [{ name: "testTool", output: "some output" }];
    const messages = createToolResponseMessages(outputs);

    assert.strictEqual(messages.length, 2, "Should return exactly 2 messages");
  });

  it("createToolResponseMessages first message is Tool output", () => {
    const outputs = [{ name: "testTool", output: "some output" }];
    const messages = createToolResponseMessages(outputs);
    const toolMsg = messages[0];

    let contentStr = "";
    if (typeof toolMsg.content === "string") {
      contentStr = toolMsg.content;
    } else if (Array.isArray(toolMsg.content)) {
      contentStr = toolMsg.content.map((p: any) => p.value).join("");
    }

    assert.strictEqual(
      toolMsg.role,
      vscode.LanguageModelChatMessageRole.User,
      "Role should be User"
    );
    assert.strictEqual(toolMsg.name, "Tool", "Name should be Tool");
    assert.ok(
      contentStr.includes("Tool 'testTool' output:"),
      "Content should contain tool name"
    );
    assert.ok(
      contentStr.includes("some output"),
      "Content should contain output"
    );
  });

  it("createToolResponseMessages second message is System Kick", () => {
    const outputs = [{ name: "testTool", output: "some output" }];
    const messages = createToolResponseMessages(outputs);
    const systemMsg = messages[1];

    let contentStr = "";
    if (typeof systemMsg.content === "string") {
      contentStr = systemMsg.content;
    } else if (Array.isArray(systemMsg.content)) {
      contentStr = systemMsg.content.map((p: any) => p.value).join("");
    }

    assert.strictEqual(
      systemMsg.role,
      vscode.LanguageModelChatMessageRole.User,
      "Role should be User"
    );
    assert.strictEqual(systemMsg.name, "system", "Name should be system");
    assert.ok(
      contentStr.includes("Tool execution completed"),
      "Content should be the system directive"
    );
    assert.ok(
      contentStr.includes("Do NOT summarize"),
      "Content should include silence protocol"
    );
  });
});
