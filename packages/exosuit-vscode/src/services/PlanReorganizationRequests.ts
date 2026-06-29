import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../types/machineChannel";
import { exoMachineChannel } from "../agent/lmtool/machineChannel";

export type PlanReorganizationAction =
  | {
      type: "epoch.update";
      epoch_id: string;
      title: string;
    }
  | {
      type: "epoch.reorder";
      epoch_id: string;
      position: string;
    }
  | {
      type: "phase.update";
      phase_id: string;
      title: string;
    }
  | {
      type: "phase.move";
      phase_id: string;
      epoch_id: string;
      position?: string;
    }
  | {
      type: "goal.move";
      goal_id: string;
      phase_id: string;
      position?: string;
    };

export type PlanReorganizationRequest = {
  entityType: "epoch" | "phase" | "goal";
  entityId: string;
  subject: string;
  body: string;
  action: PlanReorganizationAction;
};

type MachineChannelSend = (
  request: MachineChannelRequestEnvelope,
) => Promise<MachineChannelResponseEnvelope>;

export function buildPlanReorganizationRequest(
  action: PlanReorganizationAction,
): PlanReorganizationRequest {
  switch (action.type) {
    case "epoch.update":
      return {
        entityType: "epoch",
        entityId: action.epoch_id,
        subject: `Recommend renaming epoch to "${action.title}"`,
        body: "The sidebar queued this plan reorganization request for the agent to apply.",
        action,
      };
    case "epoch.reorder":
      return {
        entityType: "epoch",
        entityId: action.epoch_id,
        subject: `Recommend moving epoch to ${action.position}`,
        body: "The sidebar queued this plan reorganization request for the agent to apply.",
        action,
      };
    case "phase.update":
      return {
        entityType: "phase",
        entityId: action.phase_id,
        subject: `Recommend renaming phase to "${action.title}"`,
        body: "The sidebar queued this plan reorganization request for the agent to apply.",
        action,
      };
    case "phase.move":
      return {
        entityType: "phase",
        entityId: action.phase_id,
        subject: `Recommend moving phase to epoch ${action.epoch_id}`,
        body: "The sidebar queued this plan reorganization request for the agent to apply.",
        action,
      };
    case "goal.move":
      return {
        entityType: "goal",
        entityId: action.goal_id,
        subject: `Recommend moving goal to phase ${action.phase_id}`,
        body: "The sidebar queued this plan reorganization request for the agent to apply.",
        action,
      };
  }
}

export async function queuePlanReorganizationRequest(
  workspaceRoot: string,
  action: PlanReorganizationAction,
  send: MachineChannelSend = (request) => exoMachineChannel(workspaceRoot, request),
): Promise<MachineChannelResponseEnvelope> {
  const request = buildPlanReorganizationRequest(action);
  const response = await send({
    protocol_version: 1,
    id: `vscode.plan.reorganization.${Date.now()}`,
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path: ["inbox", "add"] },
        input: {
          subject: request.subject,
          entity_type: request.entityType,
          entity_id: request.entityId,
          source: "user-feedback",
          intent: "fyi",
          priority: "immediate",
          body: request.body,
          action_json: JSON.stringify(request.action),
        },
      },
    },
  });

  if (response.status !== "ok") {
    throw new Error(
      response.error?.message ?? "Failed to queue reorganization request",
    );
  }

  return response;
}

export function resolvePlanEntityId(
  itemOrId: string | { id?: string } | undefined,
): string | null {
  if (!itemOrId) {
    return null;
  }

  const raw = typeof itemOrId === "string" ? itemOrId : itemOrId.id;
  if (!raw) {
    return null;
  }

  const withoutKnownPrefix = raw.replace(/^(epoch-phase|goal|task|exec):/, "");
  const segments = withoutKnownPrefix.split("/").filter(Boolean);
  return segments.at(-1) ?? null;
}
