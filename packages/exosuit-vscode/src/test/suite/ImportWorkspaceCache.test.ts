import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const activationTracePath = path.join(
  os.tmpdir(),
  "exosuit-vscode-test-activation.log",
);
const trace = (msg: string) => {
  if (process.env.EXOSUIT_TEST_WRAPPER !== "true") {
    return;
  }
  try {
    fs.appendFileSync(
      activationTracePath,
      `${new Date().toISOString()} [ImportWorkspaceCache.test] ${msg}\n`,
    );
  } catch {
    // ignore
  }
};

trace("module evaluated (top-level)");

const enabled = process.env.EXOSUIT_ENABLE_IMPORT_CONTROL_TESTS === "true";
const suiteFn = enabled ? describe : describe.skip;

suiteFn("ImportWorkspaceCache Test Suite", () => {
  it("dynamic import('../../WorkspaceCache') completes", async () => {
    trace("before import('../../WorkspaceCache')");
    const cacheModule = await import("../../WorkspaceCache");
    trace("after import('../../WorkspaceCache')");
    assert.ok(cacheModule.WorkspaceCache);
  });
});
