import * as vscode from "vscode";

import { existsSync } from "node:fs";
import * as path from "node:path";

export interface WorkspaceRootCandidate {
  fsPath: string;
}

export interface WorkspaceRootSelection {
  rootPath: string | undefined;
  reason: string;
  candidates: string[];
}

export interface WorkspaceRootSelectionOptions {
  hasExosuitToml?: (rootPath: string) => boolean;
}

function normalizeRootPath(rootPath: string): string {
  return path.resolve(rootPath);
}

export function isFilesystemRoot(rootPath: string): boolean {
  const normalized = normalizeRootPath(rootPath);
  return normalized === path.parse(normalized).root;
}

function defaultHasExosuitToml(rootPath: string): boolean {
  return existsSync(path.join(rootPath, "exosuit.toml"));
}

/**
 * Select the workspace root Exosuit should use for daemon and machine-channel work.
 *
 * VS Code dev hosts can expose `/` as the first workspace folder. That is never
 * a useful daemon root unless it is the only explicitly selected project root,
 * so project folders containing `exosuit.toml` win first, and filesystem roots
 * are only considered after all real folders have been exhausted.
 */
export function selectWorkspaceRoot(
  folders: readonly WorkspaceRootCandidate[] | undefined,
  options: WorkspaceRootSelectionOptions = {},
): WorkspaceRootSelection {
  const hasExosuitToml = options.hasExosuitToml ?? defaultHasExosuitToml;
  const seen = new Set<string>();
  const candidates: string[] = [];

  for (const folder of folders ?? []) {
    const normalized = normalizeRootPath(folder.fsPath);
    if (seen.has(normalized)) {
      continue;
    }
    seen.add(normalized);
    candidates.push(normalized);
  }

  const projectRoot = candidates.find((candidate) => hasExosuitToml(candidate));
  if (projectRoot) {
    return {
      rootPath: projectRoot,
      reason: "contains exosuit.toml",
      candidates,
    };
  }

  const nonFilesystemRoot = candidates.find(
    (candidate) => !isFilesystemRoot(candidate),
  );
  if (nonFilesystemRoot) {
    return {
      rootPath: nonFilesystemRoot,
      reason: "first non-filesystem-root workspace folder",
      candidates,
    };
  }

  return {
    rootPath: undefined,
    reason:
      candidates.length === 0
        ? "no workspace folders"
        : "only filesystem root workspace folders are available",
    candidates,
  };
}

export function selectCurrentWorkspaceRoot(): WorkspaceRootSelection {
  return selectWorkspaceRoot(
    vscode.workspace.workspaceFolders?.map((folder) => ({
      fsPath: folder.uri.fsPath,
    })),
  );
}

export function currentWorkspaceRoot(): string | undefined {
  return selectCurrentWorkspaceRoot().rootPath;
}
