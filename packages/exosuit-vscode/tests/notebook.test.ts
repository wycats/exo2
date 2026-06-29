import { describe, it, expect, vi } from "vitest";
import { ExosuitCommentController } from "../src/notebook/comments";
import { ExosuitNotebookSerializer } from "../src/notebook/serializer";
import { ExosuitNotebookController } from "../src/notebook/controller";
import { executeDirective } from "../src/notebook/directives";
import * as vscode from "vscode";

// Mock fs/promises and child_process
vi.mock("fs/promises", () => ({
  readFile: vi.fn().mockResolvedValue("file content"),
}));

vi.mock("child_process", () => {
  const exec = (_cmd: string, _opts: any, cb: any) =>
    cb(null, "command output", "");
  return {
    exec,
    default: { exec },
  };
});

// Mock vscode
vi.mock("vscode", () => {
  return {
    comments: {
      createCommentController: vi.fn().mockReturnValue({
        commentingRangeProvider: undefined,
        createCommentThread: vi.fn(),
        dispose: vi.fn(),
      }),
    },
    commands: {
      registerCommand: vi.fn(),
    },
    Disposable: class {
      dispose() {}
    },
    Range: class {
      constructor(
        public startLine: number,
        public startCharacter: number,
        public endLine: number,
        public endCharacter: number
      ) {}
    },
    workspace: {
      notebookDocuments: [],
      getWorkspaceFolder: vi.fn(),
      workspaceFolders: [{ uri: { fsPath: "/root" } }],
    },
    window: {
      showErrorMessage: vi.fn(),
    },
    CommentMode: {
      Preview: 1,
      Editing: 2,
    },
    NotebookCellData: class {
      constructor(
        public kind: number,
        public value: string,
        public languageId: string
      ) {}
    },
    NotebookCellKind: {
      Markup: 1,
      Code: 2,
    },
    NotebookData: class {
      constructor(public cells: any[]) {}
    },
    notebooks: {
      createNotebookController: vi.fn().mockReturnValue({
        dispose: vi.fn(),
        createNotebookCellExecution: vi.fn().mockReturnValue({
          start: vi.fn(),
          end: vi.fn(),
          clearOutput: vi.fn(),
          replaceOutput: vi.fn(),
        }),
      }),
    },
  };
});

describe("ExosuitCommentController", () => {
  it("should register comment controller", () => {
    const context = { subscriptions: [] } as any;
    new ExosuitCommentController(context);
    expect(vscode.comments.createCommentController).toHaveBeenCalledWith(
      "exo-plan",
      "Exosuit Plan"
    );
    expect(vscode.commands.registerCommand).toHaveBeenCalledWith(
      "exosuit.addComment",
      expect.any(Function)
    );
  });
});

describe("Notebook Registration", () => {
  it("should instantiate notebook serializer", () => {
    const serializer = new ExosuitNotebookSerializer();
    expect(serializer).toBeDefined();
    expect(serializer.deserializeNotebook).toBeDefined();
    expect(serializer.serializeNotebook).toBeDefined();
  });
});

describe("Notebook Serializer Logic", () => {
  it("should deserialize TOML into cells", async () => {
    const serializer = new ExosuitNotebookSerializer();
    const tomlContent = `
[phase]
id = "phase-1"
title = "Phase 1"

[plan]
[[plan.goals]]
name = "Goal 1"
`;
    const content = new TextEncoder().encode(tomlContent);
    const data = await serializer.deserializeNotebook(content, {} as any);

    expect(data.cells.length).toBe(2);
  });
});

describe("Notebook Controller", () => {
  it("should create controller with correct type", () => {
    new ExosuitNotebookController();
    expect(vscode.notebooks.createNotebookController).toHaveBeenCalledWith(
      "exosuit-notebook-controller",
      "exosuit-plan",
      "Exosuit Kernel"
    );
  });
});

describe("Directives", () => {
  it("should handle unquoted paths in @file", async () => {
    const result = await executeDirective("@file: src/main.ts");
    expect(result).toBe("file content");
  });
});
