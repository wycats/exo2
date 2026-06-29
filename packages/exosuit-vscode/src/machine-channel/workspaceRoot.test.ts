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
});

describe("isFilesystemRoot", () => {
  it("identifies exact filesystem roots", () => {
    expect(isFilesystemRoot(root)).toBe(true);
    expect(isFilesystemRoot(projectRoot)).toBe(false);
  });
});
