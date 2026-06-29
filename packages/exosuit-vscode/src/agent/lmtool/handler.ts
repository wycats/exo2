import type * as vscode from "vscode";

import {
  invalidInput,
  invalidTicket,
  needsConfirmation,
  needsInput,
  notFound,
  ok,
  type ExosuitToolInput,
  type ExosuitToolOutputInternal,
} from "./protocol";
import { listItems } from "./list";
import { locate } from "./locate";
import { applyEdit } from "./edit";
import { mintTicket, verifyTicket } from "./tickets";
import { exoExec } from "./exo";
import { exoMachineChannel } from "./machineChannel";

import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../../types/machineChannel";

export interface ExosuitToolHandlerDeps {
  rootPath: string;
  workspaceRoots: string[];
  context: Pick<vscode.ExtensionContext, "secrets">;

  listItems?: typeof listItems;
  locate?: typeof locate;
  applyEdit?: typeof applyEdit;
  mintTicket?: typeof mintTicket;
  verifyTicket?: typeof verifyTicket;
  exoExec?: typeof exoExec;
  exoMachineChannel?: typeof exoMachineChannel;
}

type MachineChannelTicketPayload = {
  kind: "mc.v1";
  request: MachineChannelRequestEnvelope;
  ticket: string;
};

function isMachineChannelTicket(ticket: string): boolean {
  return ticket.startsWith("mc:");
}

function encodeMachineChannelTicket(
  payload: MachineChannelTicketPayload,
): string {
  const raw = JSON.stringify(payload);
  const encoded = Buffer.from(raw, "utf8").toString("base64url");
  return `mc:${encoded}`;
}

function decodeMachineChannelTicket(
  ticket: string,
): MachineChannelTicketPayload {
  if (!isMachineChannelTicket(ticket)) {
    throw new Error("Not a machine-channel ticket");
  }
  const encoded = ticket.slice("mc:".length);
  const raw = Buffer.from(encoded, "base64url").toString("utf8");
  const parsed = JSON.parse(raw) as MachineChannelTicketPayload;
  if (parsed.kind !== "mc.v1") {
    throw new Error("Unsupported machine-channel ticket kind");
  }
  return parsed;
}

async function callMachineChannel(
  deps: ExosuitToolHandlerDeps,
  request: MachineChannelRequestEnvelope,
): Promise<MachineChannelResponseEnvelope> {
  const doMachineChannel = deps.exoMachineChannel ?? exoMachineChannel;
  return doMachineChannel(deps.rootPath, request);
}

export async function handleExosuitToolInput(
  deps: ExosuitToolHandlerDeps,
  input: ExosuitToolInput,
): Promise<ExosuitToolOutputInternal> {
  const doListItems = deps.listItems ?? listItems;
  const doLocate = deps.locate ?? locate;
  const doApplyEdit = deps.applyEdit ?? applyEdit;
  const doMintTicket = deps.mintTicket ?? mintTicket;
  const doVerifyTicket = deps.verifyTicket ?? verifyTicket;
  const doExoExec = deps.exoExec ?? exoExec;

  try {
    if (!input || typeof input !== "object") {
      return invalidInput("Input must be an object.", {
        list: { kind: "ports", prefix: null, limit: 20 },
      });
    }

    if ("list" in input) {
      const listKind = input.list.kind;
      if (!listKind) {
        return needsInput("Missing list.kind.", {
          list: { kind: "ports", prefix: null, limit: 20 },
        });
      }

      const items = await doListItems({
        rootPath: deps.rootPath,
        kind: listKind,
        prefix: input.list.prefix ?? null,
        limit: input.list.limit,
      });

      const message =
        listKind === "tasks" && items.length === 0
          ? "No tasks found. Use 'exo task add' to create tasks, or add runnable task config in exosuit.toml or .config/exo/exosuit.toml with a [tasks] table. Use list:artifacts to locate relevant files."
          : `Listed ${items.length} item(s) for kind=${listKind}.`;

      return ok(
        {
          type: "list",
          data: {
            kind: listKind,
            items,
          },
        },
        message,
      );
    }

    if ("run" in input) {
      const run = input.run;
      const targetKind = run.targetKind ?? "task";
      const targetId = run.targetId ?? null;

      if (targetKind !== "task") {
        return invalidInput(
          "Only task execution is implemented in Phase 1 (recipes are not yet supported).",
          { list: { kind: "tasks", prefix: null, limit: 20 } },
        );
      }

      if (!targetId || targetId.trim().length === 0) {
        return needsInput("Missing run.targetId.", {
          list: { kind: "tasks", prefix: null, limit: 20 },
        });
      }

      // Default: route task execution through Machine Channel v1.
      // (Subprocess transport today; WASM transport later.)
      const req: MachineChannelRequestEnvelope = {
        protocol_version: 1,
        id: `vscode.lmtool.run.task.${targetId}`,
        op: {
          kind: "call",
          params: {
            address: { kind: "operation", path: ["run", "task"] },
            input: { id: targetId },
          },
        },
      };

      const resp = await callMachineChannel(deps, req);

      if (resp.status === "ok") {
        return ok(
          {
            type: "run",
            data: {
              targetKind: "task",
              targetId,
              result: resp.result,
            },
          },
          `Ran task ${targetId}.`,
        );
      }

      if (
        resp.status === "confirm_required" &&
        typeof resp.ticket === "string"
      ) {
        const mcTicket = encodeMachineChannelTicket({
          kind: "mc.v1",
          request: req,
          ticket: resp.ticket,
        });
        return needsConfirmation(
          `Task execution requires confirmation: ${targetId}.`,
          mcTicket,
          { use: { ticket: mcTicket, confirm: true } },
        );
      }

      if (
        resp.status === "error" &&
        resp.error?.code === "not_found" &&
        typeof resp.error?.message === "string" &&
        resp.error.message.includes("exosuit.toml")
      ) {
        return notFound(
          "Task execution requires task config. This project may be mid-migration; create exosuit.toml (or .config/exo/exosuit.toml) with [tasks], then try again.",
          {
            list: { kind: "artifacts", prefix: "exosuit.toml", limit: 20 },
          },
        );
      }

      // Fall back to the legacy CLI invocation for environments where the
      // machine channel is unavailable.
      if (resp.status === "error") {
        try {
          const stdout = await doExoExec({
            cwd: deps.rootPath,
            args: ["run", targetId],
          });
          return ok(
            {
              type: "run",
              data: {
                targetKind: "task",
                targetId,
                stdout,
                note: "legacy-cli",
              },
            },
            `Ran task ${targetId}.`,
          );
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          return notFound(`Failed to run task ${targetId}: ${message}`, {
            list: { kind: "tasks", prefix: null, limit: 20 },
          });
        }
      }

      return notFound(
        resp.error?.message ?? `Failed to run task ${targetId}.`,
        { list: { kind: "tasks", prefix: null, limit: 20 } },
      );
    }

    if ("locate" in input) {
      const what = input.locate.what ?? "artifacts";
      const id = input.locate.id ?? null;

      const located = await doLocate({
        rootPath: deps.rootPath,
        what: what as any,
        id,
      });
      if (!located) {
        const next: ExosuitToolInput =
          what === "context"
            ? {
                locate: { what: "context", id: null },
              }
            : {
                list: { kind: "artifacts", prefix: null, limit: 20 },
              };

        return notFound(`Could not locate ${what}${id ? `:${id}` : ""}.`, next);
      }

      return ok(
        {
          type: "locate",
          data: located,
        },
        `Located ${what}${id ? `:${id}` : ""}.`,
      );
    }

    if ("edit" in input) {
      const resource = input.edit.resource;
      const action = input.edit.action;
      if (!resource || !action) {
        return needsInput("Missing edit.resource or edit.action.", {
          edit: {
            resource: "walkthrough",
            action: "add",
            input: {
              type: "feat",
              description: "<describe the change>",
              details: "<optional details>",
            },
          },
        });
      }

      if ((resource as string) === "decisions") {
        return invalidInput(
          "decisions is deprecated and is projected from RFCs; edits are not supported. Use RFC tooling instead.",
          { list: { kind: "artifacts", prefix: null, limit: 20 } },
        );
      }

      const payload = input.edit.input;
      if (!payload || typeof payload !== "object") {
        return needsInput(
          "Missing input payload for edit. Provide edit.input: { ... }.",
          {
            edit: {
              resource,
              action,
              input:
                resource === "walkthrough" && action === "add"
                  ? {
                      type: "feat",
                      description: "<describe the change>",
                      details: "<optional details>",
                    }
                  : { note: "<payload>" },
            },
          },
        );
      }

      const steerOnInvalid: ExosuitToolInput = {
        edit: { resource, action, input: payload },
      };

      const ticket = await doMintTicket({
        context: deps.context as any,
        workspaceRoots: deps.workspaceRoots,
        capKind: `edit.${resource}.${action}`,
        capData: { resource, action, payload },
        confirmRequired: true,
        expiresInMs: 5 * 60 * 1000,
        steerOnInvalid,
      });

      return needsConfirmation(
        `Edit requires confirmation: ${resource}.${action}.`,
        ticket,
        { use: { ticket, confirm: true } },
      );
    }

    if ("use" in input) {
      const ticket = input.use.ticket;
      if (!ticket) {
        return needsInput("Missing ticket.", {
          list: { kind: "ports", prefix: null, limit: 20 },
        });
      }

      if (isMachineChannelTicket(ticket)) {
        if (input.use.confirm !== true) {
          return needsConfirmation("Confirmation required.", ticket, {
            use: {
              ticket,
              confirm: true,
            },
          });
        }

        let payload: MachineChannelTicketPayload;
        try {
          payload = decodeMachineChannelTicket(ticket);
        } catch (e) {
          return invalidTicket(
            `INVALID_TICKET: ${e instanceof Error ? e.message : String(e)}`,
            { list: { kind: "ports", prefix: null, limit: 20 } },
          );
        }

        const confirmedRequest: MachineChannelRequestEnvelope = {
          ...payload.request,
          id: `${payload.request.id}.confirm`,
          auth: { ticket: payload.ticket, confirm: true },
        };

        const resp = await callMachineChannel(deps, confirmedRequest);
        if (resp.status === "ok") {
          return ok(
            {
              type: "run",
              data: {
                result: resp.result,
              },
            },
            "Ran task (machine channel).",
          );
        }

        return notFound(
          resp.error?.message ??
            `Machine channel returned status=${resp.status}`,
          { list: { kind: "tasks", prefix: null, limit: 20 } },
        );
      }

      const verified = await doVerifyTicket({
        context: deps.context as any,
        workspaceRoots: deps.workspaceRoots,
        ticket,
      });

      if (!verified.ok) {
        const steer = (verified as any).steerOnInvalid;
        return invalidTicket(
          `INVALID_TICKET: ${verified.reason}`,
          steer && typeof steer === "object"
            ? (steer as any)
            : { list: { kind: "ports", prefix: null, limit: 20 } },
        );
      }

      const payload = verified.payload;
      if (
        payload.cap?.constraints?.confirmRequired &&
        input.use.confirm !== true
      ) {
        return needsConfirmation("Confirmation required.", ticket, {
          use: {
            ticket,
            confirm: true,
          },
        });
      }

      const capKind = payload.cap.kind;
      if (capKind.startsWith("edit.")) {
        const data = payload.cap.data as any;
        const result = await doApplyEdit({
          rootPath: deps.rootPath,
          resource: data.resource,
          action: data.action,
          payload: data.payload,
        });

        if ((result as any).error) {
          return invalidInput((result as any).error, {
            edit: {
              resource: data.resource,
              action: data.action,
              input: data.payload,
            },
          });
        }

        return ok(
          {
            type: "edit",
            data: {
              resource: data.resource,
              action: data.action,
              stdout: (result as any).stdout,
            },
          },
          `Applied edit ${data.resource}.${data.action}.`,
        );
      }

      return invalidInput(
        `Ticket capability not implemented: ${capKind}`,
        (payload.cap?.steerOnInvalid as any) ?? {
          list: { kind: "ports", prefix: null, limit: 20 },
        },
      );
    }

    return invalidInput(
      "Unknown operation. Provide list, run, locate, edit, or use.",
      {
        list: { kind: "ports", prefix: null, limit: 20 },
      },
    );
  } catch (e) {
    return {
      status: "error",
      code: "INTERNAL",
      message: `Internal error: ${e instanceof Error ? e.message : String(e)}`,
      result: null,
      ticket: null,
      steering: {
        nextCall: {
          list: { kind: "ports", prefix: null, limit: 20 },
        },
      },
    };
  }
}
