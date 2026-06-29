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
      `${new Date().toISOString()} [IdleControl.test] ${msg}\n`,
    );
  } catch {
    // ignore
  }
};

trace("module evaluated (top-level)");

const enabled = process.env.EXOSUIT_ENABLE_IDLE_CONTROL === "true";
const suiteFn = enabled ? describe : describe.skip;

suiteFn("IdleControl Test Suite", () => {
  it("async wait does not block", async () => {
    trace("before 6000ms sleep");
    await new Promise<void>((resolve) => setTimeout(resolve, 6000));
    trace("after 6000ms sleep");
    assert.strictEqual(true, true);
  }, 15_000);
});
