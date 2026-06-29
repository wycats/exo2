import * as fs from "node:fs";
import * as path from "node:path";
import { parse as parseToml } from "smol-toml";

import {
  applyPrefixAndLimit,
  normalizeLimit,
  type ExosuitListKind,
  type ExosuitToolItem,
} from "./protocol";
import { exoMachineChannel } from "./machineChannel";

export interface ListOptions {
  rootPath: string;
  kind: ExosuitListKind;
  prefix?: string | null;
  limit?: number;
}

export async function listItems(
  options: ListOptions,
): Promise<ExosuitToolItem[]> {
  const normalizedLimit = normalizeLimit(options.limit) ?? 20;
  const prefix = options.prefix;

  switch (options.kind) {
    case "ports": {
      return applyPrefixAndLimit(
        [
          { kind: "port", id: "run", label: "run" },
          { kind: "port", id: "locate", label: "locate" },
          { kind: "port", id: "edit", label: "edit" },
        ],
        prefix,
        normalizedLimit,
      );
    }

    case "tasks": {
      const tasks: ExosuitToolItem[] = [];

      // Prefer Machine Channel v1 so the VS Code LM tool and `exo` stay aligned.
      // Keep a fallback to parsing exosuit.toml directly so the LM tool remains
      // usable even when the channel is unavailable.
      try {
        const resp = await exoMachineChannel(options.rootPath, {
          protocol_version: 1,
          id: "vscode.lmtool.list.run.tasks",
          op: {
            kind: "list",
            params: {
              address: { kind: "namespace", path: ["run"] },
              kind: "tasks",
              page: { cursor: null, limit: Math.max(normalizedLimit, 20) },
            },
          },
        });

        if (
          resp.status === "ok" &&
          resp.result &&
          typeof resp.result === "object"
        ) {
          const items = (resp.result as any).items as Array<any>;
          if (Array.isArray(items)) {
            for (const it of items) {
              const id = typeof it?.id === "string" ? it.id : null;
              if (!id) {
                continue;
              }
              const desc =
                typeof it?.description === "string"
                  ? it.description
                  : typeof it?.title === "string"
                    ? it.title
                    : undefined;
              tasks.push({
                kind: "task",
                id,
                label: desc ?? id,
                description: desc,
              });
            }
          }

          tasks.sort((a, b) => a.id.localeCompare(b.id));
          return applyPrefixAndLimit(
            dedupeById(tasks),
            prefix,
            normalizedLimit,
          );
        }
      } catch {
        // Fall back to parsing exosuit.toml.
      }

      // We intentionally parse exosuit.toml directly here.
      // `exo toml read --format json` is not currently JSON, so using the CLI output
      // would be brittle and would silently collapse to an empty list.
      const candidatePaths = [
        path.join(options.rootPath, "exosuit.toml"),
        path.join(options.rootPath, ".config", "exo", "exosuit.toml"),
      ];

      const exosuitTomlPath = candidatePaths.find((p) => fs.existsSync(p));
      if (!exosuitTomlPath) {
        tasks.sort((a, b) => a.id.localeCompare(b.id));
        return applyPrefixAndLimit(dedupeById(tasks), prefix, normalizedLimit);
      }

      try {
        const content = fs.readFileSync(exosuitTomlPath, "utf8");
        const data = parseToml(content) as any;
        const table = data?.tasks;

        if (table && typeof table === "object") {
          for (const id of Object.keys(table)) {
            const entry = table[id] ?? {};
            const desc =
              typeof entry?.desc === "string" ? entry.desc : undefined;
            tasks.push({
              kind: "task",
              id,
              label: desc ?? id,
              description: desc,
            });
          }
        }
      } catch {
        // Treat parse errors as "no tasks"; higher-level code will steer to artifacts.
      }

      tasks.sort((a, b) => a.id.localeCompare(b.id));
      return applyPrefixAndLimit(dedupeById(tasks), prefix, normalizedLimit);
    }

    case "artifacts": {
      const candidates: ExosuitToolItem[] = [
        {
          kind: "artifact",
          id: "exosuit.toml",
          label: "exosuit.toml (task config)",
          path: "exosuit.toml",
        },
        {
          kind: "artifact",
          id: ".config/exo/exosuit.toml",
          label: ".config/exo/exosuit.toml (legacy task config)",
          path: ".config/exo/exosuit.toml",
        },
        {
          kind: "artifact",
          id: "docs/rfcs/README.md",
          label: "RFC index",
          path: "docs/rfcs/README.md",
        },
        {
          kind: "artifact",
          id: "docs/rfcs/stage-0",
          label: "RFCs: stage-0",
          path: "docs/rfcs/stage-0",
        },
        {
          kind: "artifact",
          id: "docs/agent-context/axioms.system.toml",
          label: "Axioms: System",
          path: "docs/agent-context/axioms.system.toml",
        },
        {
          kind: "artifact",
          id: "docs/agent-context/axioms.workflow.toml",
          label: "Axioms: Workflow",
          path: "docs/agent-context/axioms.workflow.toml",
        },
        {
          kind: "artifact",
          id: "docs/agent-context/axioms.design.toml",
          label: "Axioms: Design",
          path: "docs/agent-context/axioms.design.toml",
        },
        {
          kind: "artifact",
          id: ".config/exo/tool-presentation.toml",
          label: "Tool presentation (preferred)",
          path: ".config/exo/tool-presentation.toml",
        },
        {
          kind: "artifact",
          id: "docs/agent-context/tool-presentation.toml",
          label: "Deprecated: tool presentation (legacy)",
          path: "docs/agent-context/tool-presentation.toml",
        },
        {
          kind: "artifact",
          id: ".config/exo/hooks.toml",
          label: "Exo hooks config",
          path: ".config/exo/hooks.toml",
        },
      ];

      for (const item of candidates) {
        if (item.path) {
          item.exists = fs.existsSync(path.join(options.rootPath, item.path));
        }
      }

      const coreArtifactIds = new Set<string>([
        "docs/rfcs/README.md",
        "docs/rfcs/stage-0",
      ]);

      const visible = candidates.filter((item) => {
        if (coreArtifactIds.has(item.id)) {
          return true;
        }
        return item.exists === true;
      });

      return applyPrefixAndLimit(visible, prefix, normalizedLimit);
    }

    case "recipes": {
      // Phase 1: recipes are a curated alias list; leave empty until defined.
      return [];
    }

    default: {
      return [];
    }
  }
}

function dedupeById(items: ExosuitToolItem[]): ExosuitToolItem[] {
  const seen = new Set<string>();
  const out: ExosuitToolItem[] = [];
  for (const it of items) {
    if (seen.has(it.id)) {
      continue;
    }
    seen.add(it.id);
    out.push(it);
  }
  return out;
}
