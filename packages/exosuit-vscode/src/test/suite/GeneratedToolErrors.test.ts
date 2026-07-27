import { describe, it } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";

import { buildToolFromSpec } from "../../lmtool/tool-factory";
import type { OperationSpec } from "../../lmtool/command-spec.types";
import { DaemonChannelServer } from "../../machine-channel/DaemonChannelServer";

function operationSpec(): OperationSpec {
  return {
    name: "promote",
    description: "Promote RFC to the specified next stage",
    effect: "write",
    recovery_class: "external_at_most_once",
    needs_upgrade_gate: false,
    args: [
      {
        id: "id",
        name: "id",
        description: "The RFC ID to promote",
        kind: "positional",
        value_type: "string",
        optional: false,
      },
      {
        id: "stage",
        name: "stage",
        description: "Required target stage",
        kind: "option",
        value_type: "int",
        optional: false,
      },
    ],
  };
}

function toolText(result: unknown): string {
  const content = (result as { content?: Array<{ value?: string }> }).content;
  assert.ok(content, "tool result has content");
  assert.strictEqual(content.length, 1);
  const value = content[0]?.value;
  assert.strictEqual(typeof value, "string");
  return value as string;
}

describe("Generated LM tool error handling", () => {
  it("preserves machine-channel error codes, details, and steering", async () => {
    const originalGetInstance = DaemonChannelServer.getInstance;
    assert.ok(
      vscode.workspace.workspaceFolders?.[0]?.uri.fsPath,
      "expected an extension-host workspace",
    );
    (DaemonChannelServer as any).getInstance = () => ({
      getServerModeAvailability: () => ({ available: true }),
      request: async () => ({
        protocol_version: 1,
        id: "test",
        status: "error",
        error: {
          code: "invalid_input",
          message:
            "Invalid RFC ID '__invalid_probe__'. Expected a numeric RFC number.",
          details: {
            operation: "rfc.promote",
            rfc_id: "__invalid_probe__",
            mutation_performed: false,
            safe_next: "exo rfc list",
          },
        },
        steering: {
          next_call: { kind: "call", params: { address: "help" } },
        },
      }),
    });

    try {
      const tool = buildToolFromSpec("rfc", "promote", operationSpec());
      const result = await tool.invoke(
        {
          input: { id: "__invalid_probe__", stage: 999 },
        } as any,
        {} as any,
      );
      const payload = JSON.parse(toolText(result));

      assert.strictEqual(payload.status, "error");
      assert.strictEqual(payload.code, "invalid_input");
      assert.strictEqual(
        payload.message,
        "Invalid RFC ID '__invalid_probe__'. Expected a numeric RFC number.",
      );
      assert.deepStrictEqual(payload.details, {
        operation: "rfc.promote",
        rfc_id: "__invalid_probe__",
        mutation_performed: false,
        safe_next: "exo rfc list",
      });
      assert.deepStrictEqual(payload.steering, {
        next_call: { kind: "call", params: { address: "help" } },
      });
      assert.ok(payload.output.includes("invalid_input"));
    } finally {
      (DaemonChannelServer as any).getInstance = originalGetInstance;
    }
  });

  it("replays approval with the request id returned by the daemon", async () => {
    const originalGetInstance = DaemonChannelServer.getInstance;
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    assert.ok(workspaceRoot, "expected an extension-host workspace");
    const requests: any[] = [];
    (DaemonChannelServer as any).getInstance = () => ({
      getServerModeAvailability: () => ({ available: true }),
      request: async (request: any) => {
        requests.push(request);
        if (request.auth?.confirm === true) {
          return {
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: { promoted: true },
          };
        }

        return {
          protocol_version: 1,
          id: `${request.id}.daemon`,
          status: "confirm_required",
          ticket: "blake3:test-ticket",
        };
      },
    });

    try {
      const tool = buildToolFromSpec("rfc", "promote", operationSpec());
      await tool.invoke(
        {
          input: { id: "10196", stage: 3 },
        } as any,
        {} as any,
      );

      assert.strictEqual(requests.length, 2);
      assert.strictEqual(requests[1].id, `${requests[0].id}.daemon`);
      assert.strictEqual(requests[1].auth.requestId, requests[1].id);
      assert.strictEqual(requests[1].auth.workspaceRoot, workspaceRoot);
    } finally {
      (DaemonChannelServer as any).getInstance = originalGetInstance;
    }
  });
});
