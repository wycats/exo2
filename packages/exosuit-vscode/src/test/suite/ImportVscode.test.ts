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
      `${new Date().toISOString()} [ImportVscode.test] ${msg}\n`,
    );
  } catch {
    // ignore
  }
};

trace("module evaluated (top-level)");

const enabled = process.env.EXOSUIT_ENABLE_IMPORT_CONTROL_TESTS === "true";
const suiteFn = enabled ? describe : describe.skip;

suiteFn("ImportVscode Test Suite", () => {
  it("dynamic import('vscode') completes", async () => {
    trace("before import('vscode')");
    const vscodeModule = await import("vscode");
    trace("after import('vscode')");
    assert.ok(vscodeModule);
  });
});
