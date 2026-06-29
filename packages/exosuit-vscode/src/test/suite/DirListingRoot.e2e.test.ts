import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";
import * as fs from "node:fs/promises";
import * as path from "node:path";

type InspectedRootValue = {
  path: string;
  hash: string;
  entries: Array<{ name: string; kind: "file" | "dir" | "symlink" }>;
};

type InspectedRootSuccess = {
  id: string;
  digest: string;
  value: InspectedRootValue;
};

type InspectedRootError = { id: string; error: string };

type InspectedRoot = InspectedRootSuccess | InspectedRootError;

async function delay(ms: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

async function ensureActivated(): Promise<void> {
  const extension = vscode.extensions.getExtension("exosuit.exosuit-context");
  assert.ok(extension, "Expected dev extension exosuit.exosuit-context");
  await extension.activate();

  // The extension's activate() may return before all async activation work has
  // completed (including command registration). Wait for the debug command.
  for (let i = 0; i < 100; i++) {
    const commands = await vscode.commands.getCommands(true);
    if (commands.includes("exosuit.debug.dumpState")) {
      return;
    }
    await delay(50);
  }
  assert.fail("Timed out waiting for exosuit.debug.dumpState");
}

async function dumpState(): Promise<any> {
  return vscode.commands.executeCommand("exosuit.debug.dumpState");
}

async function dumpInspectedRoot(
  id: string,
  opts: { tries?: number; delayMs?: number } = {},
): Promise<InspectedRootSuccess> {
  const tries = opts.tries ?? 25;
  const delayMs = opts.delayMs ?? 100;

  let last: InspectedRoot | undefined;
  for (let i = 0; i < tries; i++) {
    const state = await dumpState();
    const inspected = state?.reactivity?.inspected?.[id] as
      | InspectedRoot
      | undefined;
    if (inspected) {
      last = inspected;
      if ("error" in inspected) {
        // If the runtime is still initializing, retry.
        if (
          inspected.error.includes("Engine not initialized") ||
          inspected.error.includes("Failed to initialize")
        ) {
          await delay(delayMs);
          continue;
        }
      } else if (
        !("error" in inspected) &&
        typeof inspected.digest === "string" &&
        inspected.value
      ) {
        return inspected as InspectedRootSuccess;
      }
    }
    await delay(delayMs);
  }

  assert.fail(
    `Timed out waiting for inspected root ${id}. Last=${JSON.stringify(last)}`,
  );
}

describe("DirListing Root (E2E)", () => {
  it("agent.rfcs.dir updates for nested and top-level changes", async () => {
    await ensureActivated();

    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    assert.ok(workspaceFolder, "Expected a workspace folder");
    const workspaceRoot = workspaceFolder.uri.fsPath;

    // Create a minimal docs structure in the test workspace root so
    // materializers don't error.
    const agentContextDir = path.join(workspaceRoot, "docs/agent-context");
    const agentCurrentDir = path.join(agentContextDir, "current");
    await fs.mkdir(agentCurrentDir, { recursive: true });
    await fs.writeFile(
      path.join(agentContextDir, "plan.toml"),
      "# test plan\n",
      "utf8",
    );
    await fs.writeFile(
      path.join(agentCurrentDir, "implementation-plan.toml"),
      "# test implementation plan\n",
      "utf8",
    );

    const rfcsRoot = path.join(workspaceRoot, "docs/rfcs");
    const stage0 = path.join(rfcsRoot, "stage-0");
    await fs.mkdir(stage0, { recursive: true });

    const nestedPath = path.join(stage0, "__exosuit_dirlisting_test__.md");
    const topLevelPath = path.join(rfcsRoot, "__exosuit_dirlisting_root__.md");

    try {
      await fs.writeFile(nestedPath, "initial\n", "utf8");

      const first = await dumpInspectedRoot("agent.rfcs.dir");
      assert.strictEqual(first.value.path, rfcsRoot);
      assert.ok(
        first.value.entries.some(
          (e: { name: string; kind: "file" | "dir" | "symlink" }) =>
            e.name === "stage-0" && e.kind === "dir",
        ),
        "Expected stage-0 to be listed at docs/rfcs top-level",
      );

      const firstDigest = first.digest;
      const firstHash = first.value.hash;
      const firstEntriesJson = JSON.stringify(first.value.entries);

      // Nested change: should change the directory hash/digest but not top-level entries.
      await fs.writeFile(nestedPath, `updated:${Date.now()}\n`, "utf8");
      const second = await dumpInspectedRoot("agent.rfcs.dir");
      assert.notStrictEqual(second.value.hash, firstHash);
      assert.notStrictEqual(second.digest, firstDigest);
      assert.strictEqual(
        JSON.stringify(second.value.entries),
        firstEntriesJson,
        "Expected top-level entries to remain stable after nested edit",
      );

      // Top-level change: should update entries.
      await fs.writeFile(topLevelPath, "root-file\n", "utf8");
      const third = await dumpInspectedRoot("agent.rfcs.dir");
      assert.ok(
        third.value.entries.some(
          (e: { name: string; kind: "file" | "dir" | "symlink" }) =>
            e.name === "__exosuit_dirlisting_root__.md" && e.kind === "file",
        ),
        "Expected new top-level file to appear in entries",
      );
    } finally {
      // Best-effort cleanup.
      await fs.rm(topLevelPath, { force: true });
      await fs.rm(nestedPath, { force: true });
    }
  }, 20_000);
});
