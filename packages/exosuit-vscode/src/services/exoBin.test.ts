import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { describe, expect, it } from "vitest";

import { exoCommand, resolveExoBinary } from "../exoBin";

describe("exo binary resolution", () => {
  it("prefers the workspace-local binary from exosuit.toml", () => {
    const root = mkdtempSync(join(tmpdir(), "exo-bin-test-"));
    mkdirSync(join(root, "target/debug"), { recursive: true });
    writeFileSync(join(root, "target/debug/exo"), "");
    writeFileSync(
      root + "/exosuit.toml",
      '[dev]\nbinary_dir = "target/debug"\n',
    );

    expect(resolveExoBinary("exo", root)).toBe(join(root, "target/debug/exo"));
    expect(exoCommand("status --format json", root)).toBe(
      `${JSON.stringify(join(root, "target/debug/exo"))} status --format json`,
    );
  });

  it("prefers workspace-local exo over EXO_BIN when both are present", () => {
    const previous = process.env.EXO_BIN;
    const root = mkdtempSync(join(tmpdir(), "exo-bin-test-"));
    mkdirSync(join(root, "target/debug"), { recursive: true });
    writeFileSync(join(root, "target/debug/exo"), "");
    writeFileSync(
      root + "/exosuit.toml",
      '[dev]\nbinary_dir = "target/debug"\n',
    );

    try {
      process.env.EXO_BIN = "/custom/exo";
      expect(resolveExoBinary("exo", root)).toBe(
        join(root, "target/debug/exo"),
      );
    } finally {
      if (previous === undefined) {
        delete process.env.EXO_BIN;
      } else {
        process.env.EXO_BIN = previous;
      }
    }
  });
});
