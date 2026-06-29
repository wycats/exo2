import { describe, expect, it } from "vitest";
import { Linter } from "eslint";
import tsParser from "@typescript-eslint/parser";

import rule from "./no-agent-context-toml-writes.js";

type LintMessage = {
  ruleId: string | null;
  message: string;
};

function lint(code: string, filename = "/tmp/file.ts"): LintMessage[] {
  const linter = new Linter();

  // ESLint's Linter needs a named parser.
  linter.defineParser("@typescript-eslint/parser", tsParser as any);
  linter.defineRule("exosuit/no-agent-context-toml-writes", rule as any);

  return linter.verify(
    code,
    {
      parser: "@typescript-eslint/parser",
      parserOptions: {
        ecmaVersion: 2020,
        sourceType: "module",
      },
      rules: {
        "exosuit/no-agent-context-toml-writes": "error",
      },
    },
    { filename }
  ) as unknown as LintMessage[];
}

describe("exosuit/no-agent-context-toml-writes", () => {
  it("flags direct writes to feedback.toml (named import)", () => {
    const messages = lint(
      [
        'import { writeFileSync } from "node:fs";',
        'writeFileSync("docs/agent-context/feedback.toml", "threads = []\\n");',
      ].join("\n")
    );

    expect(messages.length).toBeGreaterThan(0);
    expect(messages[0]?.ruleId).toBe("exosuit/no-agent-context-toml-writes");
    expect(messages[0]?.message).toContain("STEERING");
    expect(messages[0]?.message).toContain("feedback");
  });

  it("flags fs.promises.writeFile to feedback.toml (namespace import)", () => {
    const messages = lint(
      [
        'import * as fs from "fs";',
        'await fs.promises.writeFile("docs/agent-context/feedback.toml", "x");',
      ].join("\n")
    );

    expect(messages.length).toBeGreaterThan(0);
    expect(messages[0]?.message).toContain("STEERING");
  });

  it("flags aliased import of writeFile", () => {
    const messages = lint(
      [
        'import { writeFile as wf } from "fs";',
        'wf("docs/agent-context/feedback.toml", "x", () => {});',
      ].join("\n")
    );

    expect(messages.length).toBeGreaterThan(0);
    expect(messages[0]?.message).toContain("STEERING");
  });

  it("flags vscode.workspace.fs.writeFile when writing agent-context TOML", () => {
    const messages = lint(
      [
        'import * as vscode from "vscode";',
        "const bytes = new Uint8Array();",
        'await vscode.workspace.fs.writeFile(vscode.Uri.file("docs/agent-context/feedback.toml"), bytes);',
      ].join("\n")
    );

    expect(messages.length).toBeGreaterThan(0);
    expect(messages[0]?.message).toContain("STEERING");
  });

  it("does not flag non-agent-context TOML writes", () => {
    const messages = lint(
      [
        'import { writeFileSync } from "fs";',
        'writeFileSync("docs/not-agent-context.toml", "x");',
      ].join("\n")
    );

    expect(messages.length).toBe(0);
  });

  it("allows the controlled bootstrap initializer exception", () => {
    const messages = lint(
      [
        'import * as fs from "fs";',
        "export function initializeProject() {",
        '  fs.writeFileSync("docs/agent-context/plan.toml", "x");',
        "}",
      ].join("\n"),
      "/home/dev/src/exo2/packages/exosuit-vscode/src/DashboardProvider.ts"
    );

    expect(messages.length).toBe(0);
  });

  it("flags CommonJS require() namespace usage", () => {
    const messages = lint(
      [
        'const fs = require("fs");',
        'fs.writeFileSync("docs/agent-context/feedback.toml", "x");',
      ].join("\n")
    );

    expect(messages.length).toBeGreaterThan(0);
    expect(messages[0]?.message).toContain("STEERING");
  });
});
