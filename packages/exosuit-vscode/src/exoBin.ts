import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

import { parse as parseToml } from "smol-toml";

/**
 * Known binary names in the exo toolchain.
 */
export type ExoBinaryName = "exo" | "exohistory" | "exohook";

/**
 * Resolve a named binary.
 *
 * Returns the workspace-local binary when `[dev].binary_dir` is configured,
 * then falls back to explicit `EXO_BIN`, then PATH lookup by binary name.
 *
 * @param name - The binary name (e.g. `"exo"`, `"exohistory"`, `"exohook"`)
 * @param _workspaceRoot - Unused (kept for API compatibility during transition)
 */
export function resolveExoBinary(
  name: ExoBinaryName,
  workspaceRoot?: string,
): string {
  const workspaceBinary = workspaceRoot
    ? resolveWatchableBinaryPath(name, workspaceRoot)
    : null;
  if (workspaceBinary) {
    return workspaceBinary;
  }

  if (name === "exo") {
    const envBin = process.env.EXO_BIN;
    if (envBin && envBin.trim().length > 0) {
      return envBin.trim();
    }
  }

  return name;
}

/**
 * @deprecated Use `resolveExoBinary("exo", workspaceRoot)` instead.
 */
export function resolveExoBin(_workspaceRoot: string): string {
  return "exo";
}

/**
 * Returns false — binary resolution is now handled by the re-exec protocol,
 * not by the extension pointing at a specific file path.
 */
export function isResolvableBinPath(_resolvedBin: string): boolean {
  return false;
}

/**
 * Resolve the workspace-local binary path for file watching.
 *
 * Reads `[dev] binary_dir` from `exosuit.toml` (mirroring the re-exec
 * protocol in Rust) to find the path the daemon actually runs from.
 * Returns null if no workspace-local binary exists.
 */
export function resolveWatchableBinaryPath(
  name: ExoBinaryName,
  workspaceRoot: string,
): string | null {
  try {
    const tomlPath = join(workspaceRoot, "exosuit.toml");
    const content = readFileSync(tomlPath, "utf-8");
    const data = parseToml(content) as Record<string, unknown>;
    const dev = data["dev"];
    if (!dev || typeof dev !== "object") {
      return null;
    }

    const binaryDir = (dev as Record<string, unknown>)["binary_dir"];
    if (typeof binaryDir !== "string") {
      return null;
    }

    const candidate = join(workspaceRoot, binaryDir, name);
    if (existsSync(candidate)) {
      return candidate;
    }
  } catch {
    // No exosuit.toml or parse error
  }
  return null;
}

/**
 * Build a shell command string using the exo binary.
 */
export function exoCommand(args: string, _workspaceRoot?: string): string {
  const exoBin = resolveExoBinary("exo", _workspaceRoot);
  return `${JSON.stringify(exoBin)} ${args}`;
}
