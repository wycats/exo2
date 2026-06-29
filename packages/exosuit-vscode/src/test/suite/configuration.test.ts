import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";

describe("Configuration Defaults", () => {
  it("files.readonlyInclude does not force repo-local agent-context projections read-only", () => {
    const config = vscode.workspace.getConfiguration();
    const readonlyInclude = config.inspect("files.readonlyInclude")
      ?.defaultValue as Record<string, boolean>;

    assert.strictEqual(
      readonlyInclude?.["docs/agent-context/**/*.sql"],
      undefined,
      "Repo-local agent-context SQL projections are policy-dependent and should not be forced read-only by the extension",
    );
  });
});
