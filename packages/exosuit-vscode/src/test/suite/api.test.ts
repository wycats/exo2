import { describe, it } from "./harness.js";
import * as vscode from "vscode";
import * as assert from "assert";

describe("API Check", () => {
  it("Check ChatResponse parts", () => {
    const parts = Object.keys(vscode).filter((k) =>
      k.startsWith("ChatResponse"),
    );
    assert.ok(Array.isArray(parts));
  });
});
