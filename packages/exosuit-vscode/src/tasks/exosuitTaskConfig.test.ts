import { describe, expect, test } from "vitest";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { readExosuitTaskConfig } from "./exosuitTaskConfig";

describe("readExosuitTaskConfig", () => {
  test("returns empty when config missing", () => {
    const root = fs.mkdtempSync(
      path.join(os.tmpdir(), "exosuit-taskcfg-missing-")
    );
    try {
      expect(readExosuitTaskConfig(root)).toEqual([]);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  test("prefers root exosuit.toml over .config/exo/exosuit.toml", () => {
    const root = fs.mkdtempSync(
      path.join(os.tmpdir(), "exosuit-taskcfg-pref-")
    );

    try {
      fs.mkdirSync(path.join(root, ".config", "exo"), { recursive: true });

      fs.writeFileSync(
        path.join(root, ".config", "exo", "exosuit.toml"),
        `[tasks]\nlegacy = { cmd = \"echo legacy\" }\n`,
        "utf8"
      );

      fs.writeFileSync(
        path.join(root, "exosuit.toml"),
        `[tasks]\nnew = { cmd = \"echo new\", desc = \"New\" }\n`,
        "utf8"
      );

      const tasks = readExosuitTaskConfig(root);
      expect(tasks.map((t) => t.id)).toEqual(["new"]);
      expect(tasks[0]?.desc).toBe("New");
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
});
