import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../types/machineChannel";
import { exoMachineChannel } from "../agent/lmtool/machineChannel";

export interface WorkbenchLaneDiagnostic {
  code: string;
  message: string;
  lane_id: string;
  lane_phase_id: string;
  focused_phase_id?: string;
}

export interface WorkbenchLaneSummary {
  id: string;
  title: string;
  intent: string;
  state: "prepared" | "executing";
  created_at: string;
  updated_at: string;
  phase_id: string;
  phase_title: string;
  phase_status: string;
  focused_here: boolean;
}

export interface WorkbenchLaneList {
  kind: "lane.list";
  ok: true;
  lanes: WorkbenchLaneSummary[];
  diagnostics: WorkbenchLaneDiagnostic[];
}

export interface WorkbenchLaneQuickPickItem {
  label: string;
  description: string;
  detail: string;
  laneId: string;
}

export type WorkbenchLaneMachineChannel = (
  cwd: string,
  request: MachineChannelRequestEnvelope,
) => Promise<MachineChannelResponseEnvelope>;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isLane(value: unknown): value is WorkbenchLaneSummary {
  if (!isRecord(value)) {
    return false;
  }

  return (
    typeof value.id === "string" &&
    typeof value.title === "string" &&
    typeof value.intent === "string" &&
    (value.state === "prepared" || value.state === "executing") &&
    typeof value.phase_id === "string" &&
    typeof value.phase_title === "string" &&
    typeof value.phase_status === "string" &&
    typeof value.focused_here === "boolean"
  );
}

function isDiagnostic(value: unknown): value is WorkbenchLaneDiagnostic {
  return (
    isRecord(value) &&
    typeof value.code === "string" &&
    typeof value.message === "string" &&
    typeof value.lane_id === "string" &&
    typeof value.lane_phase_id === "string"
  );
}

export function workbenchLaneListFrom(
  value: unknown,
): WorkbenchLaneList | null {
  if (
    !isRecord(value) ||
    value.kind !== "lane.list" ||
    value.ok !== true ||
    !Array.isArray(value.lanes) ||
    !Array.isArray(value.diagnostics) ||
    !value.lanes.every(isLane) ||
    !value.diagnostics.every(isDiagnostic)
  ) {
    return null;
  }

  return value as unknown as WorkbenchLaneList;
}

export function focusableWorkbenchLanes(
  data: WorkbenchLaneList,
): WorkbenchLaneSummary[] {
  const mismatchedLaneIds = new Set(
    data.diagnostics
      .filter((diagnostic) => diagnostic.code === "lane.phase_focus_mismatch")
      .map((diagnostic) => diagnostic.lane_id),
  );
  return data.lanes.filter(
    (lane) =>
      lane.phase_status === "in-progress" &&
      (!lane.focused_here || mismatchedLaneIds.has(lane.id)),
  );
}

export function workbenchLaneQuickPickItem(
  lane: WorkbenchLaneSummary,
): WorkbenchLaneQuickPickItem {
  return {
    label: lane.title,
    description: `${lane.state} • ${lane.phase_title}`,
    detail: `${lane.intent}\nLane ID: ${lane.id}`,
    laneId: lane.id,
  };
}

export function workbenchLaneFocusTargetId(target: unknown): string | null {
  if (typeof target === "string") {
    return target;
  }
  if (!isRecord(target)) {
    return null;
  }
  if (typeof target.laneId === "string") {
    return target.laneId;
  }
  return isLane(target.lane) ? target.lane.id : null;
}

function laneRequest(
  operation: "list" | "focus",
  requestId: string,
  input: Record<string, unknown>,
): MachineChannelRequestEnvelope {
  return {
    protocol_version: 1,
    id: requestId,
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path: ["lane", operation] },
        input,
      },
    },
  };
}

export async function listWorkbenchLanes(
  workspaceRoot: string,
  requestId: string,
  send: WorkbenchLaneMachineChannel = exoMachineChannel,
): Promise<WorkbenchLaneList> {
  const response = await send(
    workspaceRoot,
    laneRequest("list", requestId, {}),
  );
  const data =
    response.status === "ok"
      ? workbenchLaneListFrom(response.result)
      : null;
  if (!data) {
    throw new Error(
      response.error?.message ?? "Exo returned an invalid work lane list",
    );
  }
  return data;
}

export async function focusWorkbenchLane(
  workspaceRoot: string,
  laneId: string,
  requestId: string,
  send: WorkbenchLaneMachineChannel = exoMachineChannel,
): Promise<WorkbenchLaneSummary> {
  const response = await send(
    workspaceRoot,
    laneRequest("focus", requestId, { id: laneId }),
  );
  if (
    response.status !== "ok" ||
    !isRecord(response.result) ||
    response.result.kind !== "lane.focus" ||
    response.result.ok !== true ||
    !isLane(response.result.lane)
  ) {
    throw new Error(
      response.error?.message ?? `Exo could not focus work lane ${laneId}`,
    );
  }

  return response.result.lane;
}
