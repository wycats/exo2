import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";
import * as fs from "node:fs";
import * as path from "node:path";
import { spawn } from "node:child_process";

describe("Machine Channel contract (exo json server)", () => {
  function findRepoRoot(startDir: string): string {
    let cur = startDir;
    for (let i = 0; i < 15; i++) {
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

  function repoRoot(): string {
    const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    assert.ok(root, "Expected a workspace folder for extension-host tests");
    return findRepoRoot(root!);
  }

  async function tryRun(
    exoBin: string,
    cwd: string,
    request: unknown,
  ): Promise<{
    ok: boolean;
    response?: any;
    exitCode?: number | null;
    stdout?: string;
    stderr?: string;
    error?: unknown;
  }> {
    return new Promise((resolve) => {
      const child = spawn(exoBin, ["json", "server"], {
        cwd,
        shell: false,
        stdio: ["pipe", "pipe", "pipe"],
      });

      let stdout = "";
      let stderr = "";

      child.stdout.setEncoding("utf8");
      child.stderr.setEncoding("utf8");

      child.stdout.on("data", (chunk) => {
        stdout += chunk;
      });

      child.stderr.on("data", (chunk) => {
        stderr += chunk;
      });

      child.on("error", (error) => {
        resolve({ ok: false, error, stdout, stderr });
      });

      child.on("close", (exitCode) => {
        const lines = stdout
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter((line) => line.length > 0);
        const payload = lines[0];

        if (!payload) {
          resolve({
            ok: false,
            error: new Error("No response line received from json server"),
            exitCode,
            stdout,
            stderr,
          });
          return;
        }

        try {
          const parsed = JSON.parse(payload);
          resolve({ ok: true, response: parsed, exitCode, stdout, stderr });
        } catch (error) {
          resolve({ ok: false, error, exitCode, stdout, stderr });
        }
      });

      child.stdin.setDefaultEncoding("utf8");
      child.stdin.write(`${JSON.stringify(request)}\n`);
      child.stdin.end();
    });
  }

  async function resolveExoBin(): Promise<string> {
    const root = repoRoot();

    const candidates: string[] = [];

    const envBin = process.env.EXO_BIN;
    if (envBin && envBin.trim().length > 0) {
      candidates.push(envBin.trim());
    }

    // Prefer repo-local binaries to enforce “up to date” behavior.
    candidates.push(path.join(root, "target", "debug", "exo"));
    candidates.push(path.join(root, "target", "release", "exo"));

    // Final fallback: PATH (still strict because we validate capability below).
    candidates.push("exo");

    const probe = {
      protocol_version: 1,
      id: "probe.help.root",
      op: { kind: "help", params: { address: { kind: "root" } } },
    };

    for (const candidate of candidates) {
      // For file-path candidates, ensure they exist.
      if (candidate.includes(path.sep) && !fs.existsSync(candidate)) {
        continue;
      }

      const out = await tryRun(candidate, root, probe);
      if (!out.ok || !out.response) {
        continue;
      }

      const resp = out.response as any;
      if (resp?.protocol_version !== 1) {
        continue;
      }

      if (resp?.status !== "ok") {
        continue;
      }

      if (!resp?.result || typeof resp.result !== "object") {
        continue;
      }

      return candidate;
    }

    assert.fail(
      "No compatible 'exo' binary found for Machine Channel v1. " +
        "Build it (e.g. `cargo build -p exo`) or set EXO_BIN to a compatible binary.",
    );
  }

  async function exoRequest(request: unknown): Promise<any> {
    const root = repoRoot();
    const exoBin = await resolveExoBin();

    const out = await tryRun(exoBin, root, request);
    if (!out.ok) {
      throw new Error(
        `Failed to run exo machine channel (${exoBin}).\n\nstdout:\n${
          out.stdout ?? ""
        }\n\nstderr:\n${out.stderr ?? ""}\n\nerror: ${String(out.error)}`,
      );
    }

    return out.response;
  }

  it("help(root) returns ok + namespaces", async () => {
    const resp = await exoRequest({
      protocol_version: 1,
      id: "t.help.root",
      op: { kind: "help", params: { address: { kind: "root" } } },
    });

    assert.strictEqual(resp.status, "ok");
    assert.strictEqual(resp.protocol_version, 1);

    const namespaces = resp.result?.namespaces;
    assert.ok(Array.isArray(namespaces), "Expected result.namespaces array");
    assert.ok(
      namespaces.some(
        (ns: any) =>
          Array.isArray(ns?.path) && ns.path.some((p: any) => p === "phase"),
      ),
      "Expected root help to include a phase namespace",
    );
  });

  it("call(context.paths) returns canonical paths", async () => {
    const resp = await exoRequest({
      protocol_version: 1,
      id: "t.call.context.paths",
      op: {
        kind: "call",
        params: {
          address: { kind: "operation", path: ["context", "paths"] },
          input: {},
        },
      },
    });

    assert.strictEqual(resp.status, "ok");
    const paths = resp.result?.paths ?? resp.result;
    assert.ok(
      ["repo_sql_projection", "sidecar_sql_projection", "none"].includes(
        resp.result?.projection?.kind,
      ),
    );
    if (resp.result?.projection?.kind !== "none") {
      assert.ok(paths?.plan?.endsWith("epochs.sql"));
      assert.ok(paths?.tasks?.endsWith("tasks.sql"));
    }
  });

  it("list(phase.execution, tasks) returns items + paging", async () => {
    const resp = await exoRequest({
      protocol_version: 1,
      id: "t.list.phase.execution.tasks",
      op: {
        kind: "list",
        params: {
          address: { kind: "namespace", path: ["phase", "execution"] },
          kind: "tasks",
          page: { cursor: null, limit: 2 },
        },
      },
    });

    assert.strictEqual(resp.status, "ok");
    assert.ok(Array.isArray(resp.result?.items), "Expected result.items array");
    assert.ok(resp.result?.page, "Expected result.page");

    const nextCursor = resp.result.page?.next_cursor;
    assert.ok(
      nextCursor === null || typeof nextCursor === "string",
      "Expected page.next_cursor to be string|null",
    );
  });

  it("protocol version mismatch returns error + steering", async () => {
    const resp = await exoRequest({
      protocol_version: 999,
      id: "t.bad.version",
      op: { kind: "help", params: { address: { kind: "root" } } },
    });

    assert.strictEqual(resp.status, "error");
    assert.strictEqual(resp.error?.code, "version_mismatch");
    // The machine channel returns the *internal* protocol shape (op: { kind: "help" }),
    // not the LM tool shape. The test was asserting on the raw wire format.
    // We should check that the steering payload is valid for the *machine channel*,
    // which uses `op: { kind: ... }`.
    assert.strictEqual(resp.steering?.next_call?.kind, "help");
  });
});
