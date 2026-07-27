import { describe, expect, it } from "vitest";
import * as path from "node:path";

import { isFilesystemRoot, selectWorkspaceRoot } from "../workspaceRoot";

const hasExosuitToml = (roots: Set<string>) => (rootPath: string) =>
  roots.has(rootPath);
const root = path.parse(process.cwd()).root;
const projectRoot = path.join(root, "Users", "example", "project");
const otherRoot = path.join(root, "Users", "example", "other");

describe("selectWorkspaceRoot", () => {
  it("prefers a workspace folder containing exosuit.toml", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: root }, { fsPath: projectRoot }],
      {
        hasExosuitToml: hasExosuitToml(new Set([projectRoot])),
      },
    );

    expect(selection.rootPath).toBe(projectRoot);
    expect(selection.reason).toBe("contains exosuit.toml");
  });

  it("rejects exact filesystem root when another candidate exists", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: root }, { fsPath: otherRoot }],
      {
        hasExosuitToml: hasExosuitToml(new Set()),
      },
    );

    expect(selection.rootPath).toBe(otherRoot);
    expect(selection.reason).toBe("first non-filesystem-root workspace folder");
  });

  it("does not select filesystem root as a daemon workspace", () => {
    const selection = selectWorkspaceRoot([{ fsPath: root }], {
      hasExosuitToml: hasExosuitToml(new Set()),
    });

    expect(selection.rootPath).toBeUndefined();
    expect(selection.reason).toBe(
      "only filesystem root workspace folders are available",
    );
  });

  it("normalizes duplicate candidates", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: path.join(projectRoot, ".") }],
      { hasExosuitToml: hasExosuitToml(new Set()) },
    );

    expect(selection.candidates).toEqual([projectRoot]);
  });

  it("requires an explicit selection when multiple Exosuit projects are open", () => {
    const selection = selectWorkspaceRoot(
      [{ fsPath: projectRoot }, { fsPath: otherRoot }],
      {
        hasExosuitToml: hasExosuitToml(
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
        hasExosuitToml: hasExosuitToml(
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
      hasExosuitToml: hasExosuitToml(new Set([projectRoot])),
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
        hasExosuitToml: hasExosuitToml(new Set()),
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
        hasExosuitToml: hasExosuitToml(
          new Set([projectRoot, otherRoot]),
        ),
      },
    );

    expect(selection.rootPath).toBe(projectRoot);
    expect(selection.reason).toBe(
      "first workspace folder containing exosuit.toml",
    );
  });
});

describe("isFilesystemRoot", () => {
  it("identifies exact filesystem roots", () => {
    expect(isFilesystemRoot(root)).toBe(true);
    expect(isFilesystemRoot(projectRoot)).toBe(false);
  });
});
