import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { readExosuitTaskConfig } from "../../tasks/exosuitTaskConfig";
import { ExosuitTaskProvider } from "../../tasks/ExosuitTaskProvider";

describe("Exosuit TaskProvider", () => {
  it("readExosuitTaskConfig reads tasks from exosuit.toml", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "exosuit-taskcfg-"));

    try {
      fs.writeFileSync(
        path.join(root, "exosuit.toml"),
        `[tasks]\nhello = { cmd = \"echo hi\", desc = \"Hello\", cwd = \"root\" }\nbye = { cmd = \"echo bye\" }\n`,
        "utf8"
      );

      const tasks = readExosuitTaskConfig(root);
      assert.deepStrictEqual(
        tasks.map((t) => t.id),
        ["bye", "hello"]
      );
      assert.strictEqual(tasks.find((t) => t.id === "hello")?.desc, "Hello");
      assert.strictEqual(tasks.find((t) => t.id === "bye")?.desc, undefined);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  it("TaskProvider surfaces exosuit.toml tasks in workspace", async () => {
    const folder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(folder, "Expected a workspace folder for extension-host tests");

    const provider = new ExosuitTaskProvider();
    const tasks = await provider.provideTasks();

    // The repo should define build-ext in exosuit.toml.
    assert.ok(tasks.some((t) => (t.definition as any).task === "build-ext"));
  });

  it("resolveTask creates an execution that runs exo", async () => {
    const folder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(folder, "Expected a workspace folder for extension-host tests");

    const provider = new ExosuitTaskProvider();

    // Simulate a tasks.json entry of type exosuit.
    const unresolved = new vscode.Task(
      { type: "exosuit", task: "build-ext" },
      folder,
      "build-ext",
      "exosuit"
    );

    const resolved = await provider.resolveTask(unresolved);
    assert.ok(resolved);
    assert.ok(resolved.execution);
    assert.strictEqual((resolved.execution as any).process, "exo");
  });
});
