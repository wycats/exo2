import { describe, it, beforeEach } from "./harness.js";
import * as assert from "assert";
import { ToolRegistry } from "../../agent/ToolRegistry";

describe("ToolRegistry Test Suite", () => {
  let registry: ToolRegistry;

  beforeEach(() => {
    registry = new ToolRegistry();
  });

  it("Resolves standard tool names", () => {
    assert.ok(registry.get("listDirectory"), "listDirectory should be found");
    assert.ok(registry.get("readFile"), "readFile should be found");
  });

  it("Resolves aliases", () => {
    // Test camelCase (Primary)
    const listDir = registry.get("listDirectory");
    assert.ok(listDir, "listDirectory should resolve");
    assert.strictEqual(
      listDir?.name,
      "listDirectory",
      "listDirectory should be the primary name",
    );

    // Test snake_case (Legacy Alias)
    const listFiles = registry.get("list_files");
    assert.ok(listFiles, "list_files alias should resolve");
    assert.strictEqual(
      listFiles?.name,
      "listDirectory",
      "list_files should resolve to listDirectory",
    );

    const readFile = registry.get("read_file");
    assert.ok(readFile, "read_file alias should resolve");
    assert.strictEqual(
      readFile?.name,
      "readFile",
      "read_file should resolve to readFile",
    );
  });

  it("Returns undefined for unknown tools", () => {
    assert.strictEqual(registry.get("unknown_tool"), undefined);
  });
});

// Tests for LM tool factory (exo-goal-list, etc.)
import { createZeroArgTools, getToolMetadata } from "../../lmtool/tool-factory";

describe("LM Tool Factory", () => {
  it("exo-goal-list is created as a zero-arg tool", () => {
    const tools = createZeroArgTools();
    assert.ok(
      tools.has("exo-goal-list"),
      "exo-goal-list should be created as a zero-arg tool",
    );
  });

  it("exo-goal-list metadata shows isZeroArg=true", () => {
    const metadata = getToolMetadata();
    const goalListMeta = metadata.find((m) => m.name === "exo-goal-list");
    assert.ok(goalListMeta, "exo-goal-list should exist in tool metadata");
    assert.strictEqual(
      goalListMeta?.isZeroArg,
      true,
      "exo-goal-list should be marked as zero-arg",
    );
    assert.strictEqual(
      goalListMeta?.effect,
      "pure",
      "exo-goal-list should have pure effect",
    );
    assert.strictEqual(
      goalListMeta?.argumentCount,
      0,
      "exo-goal-list should have no arguments",
    );
  });
});
