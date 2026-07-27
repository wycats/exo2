import * as vscode from "vscode";

import { existsSync, readdirSync } from "node:fs";
import * as path from "node:path";
import { resolveDaemonRuntimePaths } from "./machine-channel/socket-client";

export interface WorkspaceRootCandidate {
  fsPath: string;
}

export interface WorkspaceRootSelection {
  rootPath: string | undefined;
  reason: string;
  candidates: string[];
}

export interface WorkspaceRootSelectionOptions {
  hasExosuitProjectState?: (rootPath: string) => boolean;
  requestedRoot?: string;
  requireExplicitSelection?: boolean;
}

export interface LmToolWorkspaceRootSelectionOptions {
  requestedRoot?: string;
  hasResolvedExosuitProjectState?: (rootPath: string) => Promise<boolean>;
}

function normalizeRootPath(rootPath: string): string {
  return path.resolve(rootPath);
}

export function isFilesystemRoot(rootPath: string): boolean {
  const normalized = normalizeRootPath(rootPath);
  return normalized === path.parse(normalized).root;
}

function defaultHasExosuitProjectState(rootPath: string): boolean {
  if (
    existsSync(path.join(rootPath, "exosuit.toml")) ||
    existsSync(path.join(rootPath, ".exo", "cache", "exo.db")) ||
    existsSync(path.join(rootPath, ".cache", "exo.db"))
  ) {
    return true;
  }

  const projectionRoot = path.join(rootPath, "docs", "agent-context");
  try {
    return readdirSync(projectionRoot).some((entry) => entry.endsWith(".sql"));
  } catch {
    return false;
  }
}

async function defaultHasResolvedExosuitProjectState(
  rootPath: string,
): Promise<boolean> {
  try {
    const paths = await resolveDaemonRuntimePaths(rootPath);
    const stateRoot = path.dirname(paths.runtimeDir);
    return existsSync(path.join(stateRoot, "cache", "exo.db"));
  } catch {
    return false;
  }
}

/**
 * Select the workspace root Exosuit should use for daemon and machine-channel work.
 *
 * VS Code dev hosts can expose `/` as the first workspace folder. That is never
 * a useful daemon root unless it is the only explicitly selected project root,
 * so folders containing established Exo state win first, and filesystem roots
 * are only considered after all real folders have been exhausted.
 */
export function selectWorkspaceRoot(
  folders: readonly WorkspaceRootCandidate[] | undefined,
  options: WorkspaceRootSelectionOptions = {},
): WorkspaceRootSelection {
  const hasExosuitProjectState =
    options.hasExosuitProjectState ?? defaultHasExosuitProjectState;
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

  if (options.requestedRoot !== undefined) {
    const requestedRoot = normalizeRootPath(options.requestedRoot);
    if (!seen.has(requestedRoot)) {
      return {
        rootPath: undefined,
        reason: `requested workspaceRoot is not an open workspace folder: ${requestedRoot}`,
        candidates,
      };
    }
    if (isFilesystemRoot(requestedRoot)) {
      return {
        rootPath: undefined,
        reason: "requested workspaceRoot is a filesystem root",
        candidates,
      };
    }

    return {
      rootPath: requestedRoot,
      reason: "requested open workspace folder",
      candidates,
    };
  }

  const projectRoots = candidates.filter((candidate) =>
    hasExosuitProjectState(candidate),
  );
  if (projectRoots.length === 1) {
    return {
      rootPath: projectRoots[0],
      reason: "contains Exo project state",
      candidates,
    };
  }
  if (projectRoots.length > 1) {
    if (options.requireExplicitSelection) {
      return {
        rootPath: undefined,
        reason: `multiple Exosuit project workspace folders are open; provide workspaceRoot from: ${projectRoots.join(", ")}`,
        candidates,
      };
    }
    return {
      rootPath: projectRoots[0],
      reason: "first workspace folder containing Exo project state",
      candidates,
    };
  }

  const nonFilesystemRoots = candidates.filter(
    (candidate) => !isFilesystemRoot(candidate),
  );
  if (nonFilesystemRoots.length === 1) {
    return {
      rootPath: nonFilesystemRoots[0],
      reason: "first non-filesystem-root workspace folder",
      candidates,
    };
  }
  if (nonFilesystemRoots.length > 1) {
    if (options.requireExplicitSelection) {
      return {
        rootPath: undefined,
        reason: `multiple workspace folders are open; provide workspaceRoot from: ${nonFilesystemRoots.join(", ")}`,
        candidates,
      };
    }
    return {
      rootPath: nonFilesystemRoots[0],
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

export async function selectLmToolWorkspaceRoot(
  folders: readonly WorkspaceRootCandidate[] | undefined,
  options: LmToolWorkspaceRootSelectionOptions = {},
): Promise<WorkspaceRootSelection> {
  const initial = selectWorkspaceRoot(folders, {
    requestedRoot: options.requestedRoot,
    requireExplicitSelection: true,
  });
  if (options.requestedRoot !== undefined) {
    return initial;
  }

  const candidates = initial.candidates.filter(
    (candidate) => !isFilesystemRoot(candidate),
  );
  if (candidates.length < 2) {
    return initial;
  }

  const hasResolvedExosuitProjectState =
    options.hasResolvedExosuitProjectState ??
    defaultHasResolvedExosuitProjectState;
  const resolvedProjectRoots = new Set<string>();
  await Promise.all(
    candidates
      .filter((candidate) => !defaultHasExosuitProjectState(candidate))
      .map(async (candidate) => {
        if (await hasResolvedExosuitProjectState(candidate)) {
          resolvedProjectRoots.add(candidate);
        }
      }),
  );

  return selectWorkspaceRoot(folders, {
    requireExplicitSelection: true,
    hasExosuitProjectState: (rootPath) =>
      defaultHasExosuitProjectState(rootPath) ||
      resolvedProjectRoots.has(rootPath),
  });
}

export function selectCurrentLmToolWorkspaceRoot(
  requestedRoot?: string,
): Promise<WorkspaceRootSelection> {
  return selectLmToolWorkspaceRoot(
    vscode.workspace.workspaceFolders?.map((folder) => ({
      fsPath: folder.uri.fsPath,
    })),
    { requestedRoot },
  );
}

export function currentWorkspaceRoot(): string | undefined {
  return selectCurrentWorkspaceRoot().rootPath;
}
