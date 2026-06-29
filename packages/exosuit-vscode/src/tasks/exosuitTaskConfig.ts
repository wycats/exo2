import * as fs from "node:fs";
import * as path from "node:path";

import { parse as parseToml } from "smol-toml";

export interface ExosuitTaskConfigEntry {
  id: string;
  desc?: string;
  cmd?: string;
  cwd?: string;
}

function findRepoRoot(startDir: string): string {
  let cur = startDir;
  for (let i = 0; i < 10; i++) {
    if (fs.existsSync(path.join(cur, "exosuit.toml"))) {
      return cur;
    }
    const parent = path.dirname(cur);
    if (parent === cur) {
      break;
    }
    cur = parent;
  }
  return startDir;
}

export function readExosuitTaskConfig(
  rootPath: string
): ExosuitTaskConfigEntry[] {
  const resolvedRoot = findRepoRoot(rootPath);
  const candidatePaths = [
    path.join(resolvedRoot, "exosuit.toml"),
    path.join(resolvedRoot, ".config", "exo", "exosuit.toml"),
  ];

  const configPath = candidatePaths.find((p) => fs.existsSync(p));
  if (!configPath) {
    return [];
  }

  try {
    const content = fs.readFileSync(configPath, "utf8");
    const data = parseToml(content) as any;
    const table = data?.tasks;

    if (!table || typeof table !== "object") {
      return [];
    }

    const entries: ExosuitTaskConfigEntry[] = [];
    for (const id of Object.keys(table)) {
      const entry = table[id] ?? {};
      entries.push({
        id,
        desc: typeof entry?.desc === "string" ? entry.desc : undefined,
        cmd: typeof entry?.cmd === "string" ? entry.cmd : undefined,
        cwd: typeof entry?.cwd === "string" ? entry.cwd : undefined,
      });
    }

    entries.sort((a, b) => a.id.localeCompare(b.id));
    return entries;
  } catch {
    return [];
  }
}
