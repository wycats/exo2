import { describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import * as path from "node:path";

import {
  isFilesystemRoot,
  selectLmToolWorkspaceRoot,
  selectWorkspaceRoot,
} from "../workspaceRoot";

const hasExosuitProjectState = (roots: Set<string>) => (rootPath: string) =>
  roots.has(rootPath);
const root = path.parse(process.cwd()).root;
const projectRoot = path.join(root, "Users", "example", "project");
const otherRoot = path.join(root, "Users", "example", "other");

describe("selectWorkspaceRoot", () => {
  it("prefers a workspace folder containing Exo project state", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: root }, { fsPath: projectRoot }],
      {
        hasExosuitProjectState: hasExosuitProjectState(new Set([projectRoot])),
      },
    );

    expect(selection.rootPath).toBe(projectRoot);
    expect(selection.reason).toBe("contains Exo project state");
  });

  it("recognizes database and SQL projection project state without a manifest", () => {
    for (const marker of ["database", "legacy-database", "projection"] as const) {
      const tempRoot = mkdtempSync(
        path.join(tmpdir(), `exo-workspace-root-${marker}-`),
      );
      try {
        const project = path.join(tempRoot, "project");
        const unrelated = path.join(tempRoot, "unrelated");
        mkdirSync(project);
        mkdirSync(unrelated);
        if (marker === "database") {
          mkdirSync(path.join(project, ".exo", "cache"), { recursive: true });
          writeFileSync(path.join(project, ".exo", "cache", "exo.db"), "");
        } else if (marker === "legacy-database") {
          mkdirSync(path.join(project, ".cache"));
          writeFileSync(path.join(project, ".cache", "exo.db"), "");
        } else {
          mkdirSync(path.join(project, "docs", "agent-context"), {
            recursive: true,
          });
          writeFileSync(
            path.join(project, "docs", "agent-context", "goals.sql"),
            "",
          );
        }

        const selection = selectWorkspaceRoot(
          [{ fsPath: project }, { fsPath: unrelated }],
          { requireExplicitSelection: true },
        );

        expect(selection.rootPath).toBe(project);
        expect(selection.reason).toBe("contains Exo project state");
      } finally {
        rmSync(tempRoot, { recursive: true, force: true });
      }
    }
  });

  it("uses Exo policy resolution to identify external project state", async () => {
    const selection = await selectLmToolWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: otherRoot }],
      {
        hasResolvedExosuitProjectState: async (rootPath) =>
          rootPath === projectRoot,
      },
    );

    expect(selection.rootPath).toBe(projectRoot);
    expect(selection.reason).toBe("contains Exo project state");
  });

  it("does not prefer a local marker over another policy-resolved project", async () => {
    const tempRoot = mkdtempSync(
      path.join(tmpdir(), "exo-workspace-root-mixed-state-"),
    );
    try {
      const localProject = path.join(tempRoot, "local-project");
      const externalProject = path.join(tempRoot, "external-project");
      mkdirSync(path.join(localProject, ".exo", "cache"), { recursive: true });
      mkdirSync(externalProject);
      writeFileSync(path.join(localProject, ".exo", "cache", "exo.db"), "");

      const selection = await selectLmToolWorkspaceRoot(
        [{ fsPath: localProject }, { fsPath: externalProject }],
        {
          hasResolvedExosuitProjectState: async (rootPath) =>
            rootPath === externalProject,
        },
      );

      expect(selection.rootPath).toBeUndefined();
      expect(selection.reason).toContain(
        "multiple Exosuit project workspace folders are open",
      );
      expect(selection.reason).toContain(localProject);
      expect(selection.reason).toContain(externalProject);
    } finally {
      rmSync(tempRoot, { recursive: true, force: true });
    }
  });

  it("rejects exact filesystem root when another candidate exists", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: root }, { fsPath: otherRoot }],
      {
        hasExosuitProjectState: hasExosuitProjectState(new Set()),
      },
    );

    expect(selection.rootPath).toBe(otherRoot);
    expect(selection.reason).toBe("first non-filesystem-root workspace folder");
  });

  it("does not select filesystem root as a daemon workspace", () => {
    const selection = selectWorkspaceRoot([{ fsPath: root }], {
      hasExosuitProjectState: hasExosuitProjectState(new Set()),
    });

    expect(selection.rootPath).toBeUndefined();
    expect(selection.reason).toBe(
      "only filesystem root workspace folders are available",
    );
  });

  it("normalizes duplicate candidates", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: path.join(projectRoot, ".") }],
      { hasExosuitProjectState: hasExosuitProjectState(new Set()) },
    );

    expect(selection.candidates).toEqual([projectRoot]);
  });

  it("requires an explicit selection when multiple Exosuit projects are open", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: otherRoot }],
      {
        hasExosuitProjectState: hasExosuitProjectState(
          new Set([projectRoot, otherRoot]),
        ),
        requireExplicitSelection: true,
      },
    );

    expect(selection.rootPath).toBeUndefined();
    expect(selection.reason).toContain(
      "multiple Exosuit project workspace folders are open",
    );
    expect(selection.reason).toContain(projectRoot);
    expect(selection.reason).toContain(otherRoot);
  });

  it("accepts an explicitly requested open workspace folder", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: otherRoot }],
      {
        hasExosuitProjectState: hasExosuitProjectState(
          new Set([projectRoot, otherRoot]),
        ),
        requestedRoot: path.join(otherRoot, "."),
        requireExplicitSelection: true,
      },
    );

    expect(selection.rootPath).toBe(otherRoot);
    expect(selection.reason).toBe("requested open workspace folder");
  });

  it("rejects a requested path that is not an open workspace folder", () => {
    const missingRoot = path.join(root, "Users", "example", "missing");
    const selection = selectWorkspaceRoot([{ fsPath: projectRoot }], {
      hasExosuitProjectState: hasExosuitProjectState(new Set([projectRoot])),
      requestedRoot: missingRoot,
    });

    expect(selection.rootPath).toBeUndefined();
    expect(selection.reason).toBe(
      `requested workspaceRoot is not an open workspace folder: ${missingRoot}`,
    );
  });

  it("does not silently choose between fallback workspace folders", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: otherRoot }],
      {
        hasExosuitProjectState: hasExosuitProjectState(new Set()),
        requireExplicitSelection: true,
      },
    );

    expect(selection.rootPath).toBeUndefined();
    expect(selection.reason).toContain("multiple workspace folders are open");
  });

  it("preserves first-project selection for non-LM-tool callers", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: otherRoot }],
      {
        hasExosuitProjectState: hasExosuitProjectState(
          new Set([projectRoot, otherRoot]),
        ),
      },
    );

    expect(selection.rootPath).toBe(projectRoot);
    expect(selection.reason).toBe(
      "first workspace folder containing Exo project state",
    );
  });
});

describe("isFilesystemRoot", () => {
  it("identifies exact filesystem roots", () => {
    expect(isFilesystemRoot(root)).toBe(true);
    expect(isFilesystemRoot(projectRoot)).toBe(false);
  });
});
