import * as fs from "node:fs";
import * as path from "node:path";

import { exoMachineChannel } from "./machineChannel";
import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../../types/machineChannel";

export type LocateWhat = "artifacts" | "context" | "rfc" | "docs";
type ExoMachineChannel = (
  cwd: string,
  request: MachineChannelRequestEnvelope,
) => Promise<MachineChannelResponseEnvelope>;

const CORE_CONTEXT_IDS = new Set(["plan", "tasks", "ideas", "axioms"]);
const DEPRECATED_CONTEXT_IDS = new Map<string, string>([
  ["walkthrough", "Deprecated projection: walkthrough"],
  ["decisions", "Deprecated projection: decisions"],
]);

const CORE_ARTIFACT_ROOTS = [
  { id: "docs/agent-context", path: "docs/agent-context" },
  { id: "docs/rfcs", path: "docs/rfcs" },
];

const OPTIONAL_ARTIFACT_ROOTS = [{ id: "exosuit.toml", path: "exosuit.toml" }];

export async function locate(options: {
  rootPath: string;
  what: LocateWhat;
  id?: string | null;
  exoMachineChannel?: ExoMachineChannel;
}): Promise<
  | {
      items: Array<{
        id: string;
        path: string;
        label?: string;
        exists?: boolean;
      }>;
    }
  | { item: { id: string; path: string; label?: string; exists?: boolean } }
  | null
> {
  switch (options.what) {
    case "context": {
      let paths: Record<string, string>;
      let projectionKind = "repo_sql_projection";

      // Prefer Machine Channel v1 (CLI-shaped, structured), but keep a CLI-independent
      // fallback so the LM tool remains robust in environments without `exo` on PATH.
      try {
        const channel = options.exoMachineChannel ?? exoMachineChannel;
        const resp = await channel(options.rootPath, {
          protocol_version: 1,
          id: "vscode.locate.context.paths",
          op: {
            kind: "call",
            params: {
              address: { kind: "operation", path: ["context", "paths"] },
              input: {},
            },
          },
        });

        if (
          resp.status === "ok" &&
          resp.result &&
          typeof resp.result === "object"
        ) {
          const result = resp.result as Record<string, unknown>;
          const rawPaths = isStringRecord(result.paths) ? result.paths : result;
          paths = isStringRecord(rawPaths) ? rawPaths : {};
          projectionKind =
            typeof (result.projection as any)?.kind === "string"
              ? (result.projection as any).kind
              : projectionKind;
        } else {
          throw new Error(
            resp.error?.message ??
              `Machine channel returned status=${resp.status}`,
          );
        }
      } catch {
        paths = {
          plan: "docs/agent-context/epochs.sql",
          tasks: "docs/agent-context/tasks.sql",
          ideas: "docs/agent-context/ideas.sql",
        };
      }

      if (options.id) {
        const p = paths[options.id];
        if (!p) {
          return null;
        }

        const exists = contextPathExists(options.rootPath, p);
        const projectionIsAvailable = projectionKind !== "none";
        if (
          !exists &&
          (!CORE_CONTEXT_IDS.has(options.id) || !projectionIsAvailable)
        ) {
          return null;
        }

        return {
          item: {
            id: options.id,
            path: p,
            label: DEPRECATED_CONTEXT_IDS.get(options.id),
            exists,
          },
        };
      }

      const items = Object.entries(paths)
        .map(([id, p]) => ({
          id,
          path: p,
          label: DEPRECATED_CONTEXT_IDS.get(id),
          exists: contextPathExists(options.rootPath, p),
        }))
        .filter(
          (it) =>
            it.exists ||
            (projectionKind !== "none" && CORE_CONTEXT_IDS.has(it.id)),
        );

      return { items };
    }

    case "rfc": {
      if (!options.id) {
        return null;
      }
      const found = findRfcPath(options.rootPath, options.id);
      if (!found) {
        return null;
      }
      return {
        item: {
          id: options.id,
          path: found,
          exists: fs.existsSync(path.join(options.rootPath, found)),
        },
      };
    }

    case "docs": {
      const candidates = [
        "docs/vision.md",
        "docs/vision-edk.md",
        "docs/specs/architecture.md",
      ];

      const items = candidates
        .filter((p) => !options.id || p.includes(options.id))
        .map((p) => ({
          id: p,
          path: p,
          exists: fs.existsSync(path.join(options.rootPath, p)),
        }));

      return { items };
    }

    case "artifacts": {
      // The LM tool's `list:artifacts` is the primary discovery path.
      // Here we provide stable roots while keeping non-core items hidden unless present.
      const coreItems = CORE_ARTIFACT_ROOTS.map((it) => ({
        ...it,
        exists: fs.existsSync(path.join(options.rootPath, it.path)),
      }));

      const optionalItems = OPTIONAL_ARTIFACT_ROOTS.map((it) => ({
        ...it,
        exists: fs.existsSync(path.join(options.rootPath, it.path)),
      })).filter((it) => it.exists);

      return { items: [...coreItems, ...optionalItems] };
    }

    default:
      return null;
  }
}

function isStringRecord(value: unknown): value is Record<string, string> {
  if (!value || typeof value !== "object") {
    return false;
  }
  return Object.values(value).every((entry) => typeof entry === "string");
}

function contextPathExists(rootPath: string, candidatePath: string): boolean {
  const fullPath = path.isAbsolute(candidatePath)
    ? candidatePath
    : path.join(rootPath, candidatePath);
  return fs.existsSync(fullPath);
}

function findRfcPath(rootPath: string, id: string): string | null {
  const rfcsRoot = path.join(rootPath, "docs", "rfcs");
  if (!fs.existsSync(rfcsRoot)) {
    return null;
  }

  const stageDirs = [
    "stage-0",
    "stage-1",
    "stage-2",
    "stage-3",
    "stage-4",
    "withdrawn",
  ];

  for (const dir of stageDirs) {
    const fullDir = path.join(rfcsRoot, dir);
    if (!fs.existsSync(fullDir)) {
      continue;
    }
    const entries = fs.readdirSync(fullDir);
    const match = entries.find(
      (name) => name.startsWith(`${id}-`) && name.endsWith(".md"),
    );
    if (match) {
      return path.posix.join("docs", "rfcs", dir, match);
    }
  }

  return null;
}
