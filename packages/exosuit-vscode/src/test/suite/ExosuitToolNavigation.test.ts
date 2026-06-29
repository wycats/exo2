import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";

import { handleExosuitToolInput } from "../../agent/lmtool/handler";
import { listItems } from "../../agent/lmtool/list";
import { locate } from "../../agent/lmtool/locate";

class MemorySecrets {
  private storeMap = new Map<string, string>();

  async get(key: string): Promise<string | undefined> {
    return this.storeMap.get(key);
  }

  async store(key: string, value: string): Promise<void> {
    this.storeMap.set(key, value);
  }
}

describe("Exosuit LM Tool Navigation (torture suite)", () => {
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

  function getWorkspaceRoot(): string {
    const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    assert.ok(root, "Expected a workspace folder for extension-host tests");
    return findRepoRoot(root!);
  }

  function makeDeps(
    overrides?: Partial<Parameters<typeof handleExosuitToolInput>[0]>,
  ) {
    const rootPath = getWorkspaceRoot();
    const workspaceRoots = (vscode.workspace.workspaceFolders ?? []).map((f) =>
      f.uri.toString(),
    );

    return {
      rootPath,
      workspaceRoots,
      context: { secrets: new MemorySecrets() } as any,
      listItems,
      locate: async () => ({
        items: [
          { id: "plan", path: "docs/agent-context/plan.toml", exists: true },
        ],
      }),
      exoMachineChannel: async (_cwd: string, request: any) => {
        // Default stub: confirm-required for exec calls, ok for confirmed exec.
        if (request?.op?.kind === "call") {
          const addr = request.op.params?.address;
          const path = addr?.path;
          if (addr?.kind === "operation" && Array.isArray(path)) {
            if (path[0] === "run" && path[1] === "task") {
              if (request.auth?.confirm === true) {
                return {
                  protocol_version: 1,
                  id: request.id,
                  status: "ok",
                  result: {
                    task_id: request.op.params?.input?.id,
                    stdout: "ok",
                  },
                };
              }
              return {
                protocol_version: 1,
                id: request.id,
                status: "confirm_required",
                ticket: "blake3:test-ticket",
              };
            }
          }
        }

        return {
          protocol_version: 1,
          id: request?.id ?? "unknown",
          status: "error",
          error: { code: "unknown_address", message: "stub: unhandled" },
        };
      },
      exoExec: async () => "ok",
      applyEdit: async (opts: any) => {
        if (
          opts.resource === "tasks" &&
          opts.action === "update" &&
          opts.payload?.status !== "completed"
        ) {
          return {
            error:
              "tasks.update currently supports only status='completed' (Phase 1).",
          };
        }
        return { stdout: "ok" };
      },
      ...(overrides ?? {}),
    } as any;
  }

  it("list ports returns stable items", async () => {
    const out = await handleExosuitToolInput(makeDeps(), {
      list: { kind: "ports", prefix: null, limit: 20 },
    });

    assert.strictEqual(out.status, "ok");
    assert.ok(out.result);
    assert.strictEqual(out.steering.nextCall, null);

    const items = (out.result as any).data.items as Array<any>;
    const ids = items.map((i) => i.id).sort();
    assert.deepStrictEqual(ids, ["edit", "locate", "run"].sort());
  });

  // Skipped: listItems imports exoMachineChannel directly; the singleton server responds before the exosuit.toml fallback is reached. Needs listItems to accept injectable machine channel.
  it.skip("list tasks reads exosuit.toml", async () => {
    const tempRoot = fs.mkdtempSync(
      path.join(os.tmpdir(), "exosuit-lmtool-exosuit-toml-"),
    );

    try {
      fs.writeFileSync(
        path.join(tempRoot, "exosuit.toml"),
        `
[tasks]
build-ext = { cmd = "echo build", desc = "Compile extension" }
test-core = { cmd = "echo test", desc = "Run tests" }
        `.trim(),
        "utf8",
      );

      const out = await handleExosuitToolInput(
        makeDeps({
          rootPath: tempRoot,
          exoMachineChannel: async () => {
            throw new Error("machine channel unavailable");
          },
        }),
        {
          list: { kind: "tasks", prefix: null, limit: 50 },
        },
      );

      assert.strictEqual(out.status, "ok");
      const items = (out.result as any).data.items as Array<any>;
      assert.ok(items.length >= 1);
      assert.ok(items.some((i) => i.id === "build-ext"));
    } finally {
      fs.rmSync(tempRoot, { recursive: true, force: true });
    }
  });

  it("list artifacts hides non-core when missing", async () => {
    const tempRoot = fs.mkdtempSync(
      path.join(os.tmpdir(), "exosuit-lmtool-artifacts-"),
    );

    try {
      const out = await handleExosuitToolInput(
        makeDeps({
          rootPath: tempRoot,
        }),
        {
          list: { kind: "artifacts", prefix: null, limit: 50 },
        },
      );

      assert.strictEqual(out.status, "ok");
      const items = (out.result as any).data.items as Array<any>;
      const ids = items.map((i) => i.id);

      // Core artifacts are listable even if they don't exist.
      for (const id of ["docs/rfcs/README.md", "docs/rfcs/stage-0"]) {
        assert.ok(ids.includes(id), `expected core artifact: ${id}`);
      }

      // Deleted TOML surfaces should not appear.
      assert.ok(!ids.includes("docs/agent-context/plan.toml"));
      assert.ok(
        !ids.includes("docs/agent-context/current/implementation-plan.toml"),
      );
      assert.ok(!ids.includes("docs/agent-context/decisions.toml"));
      assert.ok(!ids.includes("docs/agent-context/current/task-list.toml"));
      assert.ok(!ids.includes("docs/agent-context/ideas.toml"));
      assert.ok(!ids.includes("docs/agent-context/axioms.system.toml"));
      assert.ok(!ids.includes("docs/agent-context/axioms.workflow.toml"));
    } finally {
      fs.rmSync(tempRoot, { recursive: true, force: true });
    }
  });

  it("request run with missing targetId steers to list tasks", async () => {
    const out = await handleExosuitToolInput(makeDeps(), {
      run: { targetKind: "task", targetId: null },
    });

    assert.strictEqual(out.status, "needs_input");
    assert.ok(out.steering.nextCall);
    assert.ok("list" in (out.steering.nextCall as any));
    assert.strictEqual((out.steering.nextCall as any).list.kind, "tasks");
  });

  it("request run unknown task returns NOT_FOUND + next_call", async () => {
    const out = await handleExosuitToolInput(
      makeDeps({
        exoMachineChannel: async (_cwd: string, request: any) => {
          if (request?.op?.kind === "call") {
            return {
              protocol_version: 1,
              id: request.id,
              status: "error",
              error: { code: "not_found", message: "no such task" },
            };
          }
          return {
            protocol_version: 1,
            id: request?.id ?? "unknown",
            status: "error",
            error: { code: "unknown_address", message: "stub" },
          };
        },
        exoExec: async () => {
          throw new Error("no such task");
        },
      }),
      {
        run: { targetKind: "task", targetId: "nope" },
      },
    );

    assert.strictEqual(out.status, "error");
    assert.strictEqual(out.code, "NOT_FOUND");
    assert.ok(out.steering.nextCall);
    assert.ok("list" in (out.steering.nextCall as any));
  });

  it("request run uses confirm ticket + use confirm runs", async () => {
    const deps = makeDeps();
    const req = await handleExosuitToolInput(deps, {
      run: { targetKind: "task", targetId: "build-ext" },
    });

    assert.strictEqual(req.status, "needs_confirmation");
    assert.ok(req.ticket);
    assert.deepStrictEqual(req.steering.nextCall, {
      use: {
        ticket: req.ticket,
        confirm: true,
      },
    });

    const out = await handleExosuitToolInput(deps, {
      use: {
        ticket: req.ticket!,
        confirm: true,
      },
    });

    assert.strictEqual(out.status, "ok");
    assert.strictEqual((out.result as any).type, "run");
  });

  it("request locate returns ok", async () => {
    const out = await handleExosuitToolInput(makeDeps(), {
      locate: { what: "context", id: null },
    });

    assert.strictEqual(out.status, "ok");
    assert.ok(out.result);
  });

  it("locate context accepts sidecar absolute projection paths", async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "exosuit-locate-"));
    const sidecarRoot = fs.mkdtempSync(
      path.join(os.tmpdir(), "exosuit-sidecar-"),
    );
    fs.mkdirSync(path.join(sidecarRoot, "agent-context"), { recursive: true });
    fs.writeFileSync(
      path.join(sidecarRoot, "agent-context", "tasks.sql"),
      "-- test\n",
    );

    try {
      const out = await locate({
        rootPath: tmp,
        what: "context",
        exoMachineChannel: async () => ({
          protocol_version: 1,
          id: "test",
          status: "ok",
          result: {
            projection: { kind: "sidecar_sql_projection", root: sidecarRoot },
            paths: {
              tasks: path.join(sidecarRoot, "agent-context", "tasks.sql"),
            },
          },
        }),
      });

      assert.ok(out);
      assert.ok("items" in out);
      const items = (out as any).items as Array<any>;
      assert.ok(items.some((it) => it.id === "tasks"));
      const tasks = items.find((it) => it.id === "tasks");
      assert.ok(path.isAbsolute(tasks.path));
      assert.ok(tasks.path.endsWith("tasks.sql"));
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
      fs.rmSync(sidecarRoot, { recursive: true, force: true });
    }
  });

  it("locate artifacts hides missing non-core roots", async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "exosuit-locate-"));
    const out = await locate({ rootPath: tmp, what: "artifacts" });

    assert.ok(out);
    assert.ok("items" in out);
    const ids = (out as any).items.map((it: any) => it.id);
    assert.deepStrictEqual(
      ids.sort(),
      ["docs/agent-context", "docs/rfcs"].sort(),
    );
  });

  it("request edit without payload steers to needs_input", async () => {
    const out = await handleExosuitToolInput(makeDeps(), {
      edit: { resource: "walkthrough", action: "add" },
    } as any);

    assert.strictEqual(out.status, "needs_input");
    assert.ok(out.steering.nextCall);
  });

  it("edit request mints confirm-required ticket", async () => {
    const out = await handleExosuitToolInput(makeDeps(), {
      edit: {
        resource: "tasks",
        action: "add",
        input: { id: "demo", label: "Demo" },
      },
    } as any);

    assert.strictEqual(out.status, "needs_confirmation");
    assert.ok(out.ticket);
    assert.deepStrictEqual(out.steering.nextCall, {
      use: {
        ticket: out.ticket,
        confirm: true,
      },
    });
  });

  it("use without confirm re-prompts confirmation", async () => {
    const deps = makeDeps();
    const req = await handleExosuitToolInput(deps, {
      edit: {
        resource: "tasks",
        action: "add",
        input: { id: "demo", label: "Demo" },
      },
    } as any);

    const out = await handleExosuitToolInput(deps, {
      use: {
        ticket: req.ticket!,
      },
    });

    assert.strictEqual(out.status, "needs_confirmation");
    assert.ok(out.steering.nextCall);
  });

  it("use confirm applies edit", async () => {
    const deps = makeDeps();
    const req = await handleExosuitToolInput(deps, {
      edit: {
        resource: "tasks",
        action: "add",
        input: { id: "demo", label: "Demo" },
      },
    } as any);

    const out = await handleExosuitToolInput(deps, {
      use: {
        ticket: req.ticket!,
        confirm: true,
      },
    });

    assert.strictEqual(out.status, "ok");
    assert.strictEqual((out.result as any).type, "edit");
  });

  it("invalid ticket returns INVALID_TICKET with steering", async () => {
    const out = await handleExosuitToolInput(makeDeps(), {
      use: {
        ticket: "not-a-ticket",
        confirm: true,
      },
    });

    assert.strictEqual(out.status, "error");
    assert.strictEqual(out.code, "INVALID_TICKET");
    assert.ok(out.steering.nextCall);
  });

  it("edit validation errors steer back to request", async () => {
    const deps = makeDeps();

    const req = await handleExosuitToolInput(deps, {
      edit: {
        resource: "tasks",
        action: "update",
        input: { id: "demo", status: "pending" },
      },
    } as any);

    const out = await handleExosuitToolInput(deps, {
      use: {
        ticket: req.ticket!,
        confirm: true,
      },
    });

    assert.strictEqual(out.status, "error");
    assert.strictEqual(out.code, "INVALID_INPUT");
    assert.ok(out.steering.nextCall);
    assert.ok("edit" in (out.steering.nextCall as any));
  });
});
