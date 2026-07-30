export interface WorkbenchLaunchResult {
  kind: "workbench.launch";
  ok: true;
  schema_version: 1;
  url: string;
  expires_at: string;
  expires_in_seconds: 300;
  reused_host: boolean;
  project: {
    id: string;
  };
  workspace: WorkbenchWorkspaceIdentity;
  daemon: {
    instance_id: string;
  };
}

export interface WorkbenchWorkspaceIdentity {
  key: string;
  label: string;
  branch: string | null;
  head: string | null;
}

export interface WorkbenchSnapshot {
  kind: "workbench.snapshot";
  ok: true;
  schema_version: 1;
  observed_at: string;
  revision: number;
  project: {
    id: string;
  };
  workspace: WorkbenchSnapshotWorkspace;
  lanes: WorkbenchLaneSummary[];
  focused_lane: WorkbenchLaneDetails | null;
  phase: WorkbenchPhase | null;
  steering: WorkbenchSteering;
  diagnostics: WorkbenchDiagnostic[];
}

export interface WorkbenchSnapshotWorkspace extends WorkbenchWorkspaceIdentity {
  detached: boolean;
  dirty: boolean;
}

export interface WorkbenchLaneSummary {
  id: string;
  title: string;
  state: "prepared" | "executing";
  phase_id: string;
  phase_title: string;
  phase_status: string;
  focused_here: boolean;
}

export interface WorkbenchLaneDetails extends WorkbenchLaneSummary {
  intent: string;
  created_at: string;
  updated_at: string;
}

export interface WorkbenchPhase {
  id: string;
  title: string;
  status: string;
  goals: WorkbenchGoal[];
}

export interface WorkbenchGoal {
  id: string;
  title: string;
  status: string;
  tasks: WorkbenchTask[];
}

export interface WorkbenchTask {
  id: string;
  title: string;
  status: string;
}

export interface WorkbenchSteering {
  situation: string;
  next_actions: WorkbenchSuggestedAction[];
}

export interface WorkbenchSuggestedAction {
  label: string;
  command: string;
  rationale: string;
  intent: string;
  confidence: number | null;
}

export interface WorkbenchDiagnostic {
  code: string;
  severity: "info" | "warning" | "error";
  message: string;
}

export interface WorkbenchSessionResult {
  kind: "workbench.session";
  ok: true;
  schema_version: 1;
  session_key: string;
  project_id: string;
  workspace_key: string;
  expires_at: string;
}

export interface WorkbenchCommandRequest {
  protocol_version: 1;
  id: string;
  session_key: string;
  operation:
    | { kind: "snapshot" }
    | { kind: "lane_focus"; lane_id: string };
}

export type WorkbenchPlanningOperation =
  | { kind: "task_add"; goal_id: string; title: string }
  | { kind: "task_update"; task_id: string; title: string }
  | { kind: "task_reorder"; task_id: string; position: number }
  | { kind: "task_start"; task_id: string }
  | { kind: "task_log"; task_id: string; message: string }
  | { kind: "task_complete_review"; task_id: string; outcome: string }
  | { kind: "task_complete_approve"; review_id: string };

export interface WorkbenchPlanningRequest {
  protocol_version: 2;
  id: string;
  session_key: string;
  expected_revision: number;
  expected_phase_id: string;
  operation: WorkbenchPlanningOperation;
}

export interface WorkbenchTaskMutationResult {
  kind: "workbench.task_mutation";
  ok: true;
  schema_version: 1;
  operation:
    | "task_add"
    | "task_update"
    | "task_reorder"
    | "task_start"
    | "task_log"
    | "task_complete_approve";
  task_id: string;
}

export interface WorkbenchTaskCompletionReview {
  kind: "workbench.task_completion_review";
  ok: true;
  schema_version: 1;
  review_id: string;
  task_id: string;
  readiness_rationale: string;
  proposed_outcome: string;
  approval_evidence_present: boolean;
}

export type WorkbenchPlanningResult =
  | WorkbenchTaskMutationResult
  | WorkbenchTaskCompletionReview;

export function decodeWorkbenchSnapshot(value: unknown): WorkbenchSnapshot {
  const snapshot = record(value, "workbench snapshot");
  literal(snapshot.kind, "workbench.snapshot", "kind");
  literal(snapshot.ok, true, "ok");
  literal(snapshot.schema_version, 1, "schema_version");
  string(snapshot.observed_at, "observed_at");
  finiteNumber(snapshot.revision, "revision");
  project(snapshot.project);
  workspace(snapshot.workspace);
  array(snapshot.lanes, "lanes").forEach((lane) => laneSummary(lane));
  if (snapshot.focused_lane !== null) {
    laneDetails(snapshot.focused_lane);
  }
  if (snapshot.phase !== null) {
    phase(snapshot.phase);
  }
  steering(snapshot.steering);
  array(snapshot.diagnostics, "diagnostics").forEach((diagnostic) =>
    workbenchDiagnostic(diagnostic),
  );
  return value as WorkbenchSnapshot;
}

export function decodeWorkbenchPlanningResult(
  value: unknown,
): WorkbenchPlanningResult {
  const result = record(value, "workbench planning result");
  literal(result.ok, true, "planning.ok");
  literal(result.schema_version, 1, "planning.schema_version");

  if (result.kind === "workbench.task_completion_review") {
    string(result.review_id, "planning.review_id");
    string(result.task_id, "planning.task_id");
    string(result.readiness_rationale, "planning.readiness_rationale");
    string(result.proposed_outcome, "planning.proposed_outcome");
    boolean(
      result.approval_evidence_present,
      "planning.approval_evidence_present",
    );
    return value as WorkbenchTaskCompletionReview;
  }

  literal(result.kind, "workbench.task_mutation", "planning.kind");
  if (
    result.operation !== "task_add" &&
    result.operation !== "task_update" &&
    result.operation !== "task_reorder" &&
    result.operation !== "task_start" &&
    result.operation !== "task_log" &&
    result.operation !== "task_complete_approve"
  ) {
    invalid("planning.operation");
  }
  string(result.task_id, "planning.task_id");
  return value as WorkbenchTaskMutationResult;
}

function project(value: unknown): void {
  string(record(value, "project").id, "project.id");
}

function workspace(value: unknown): void {
  const item = record(value, "workspace");
  string(item.key, "workspace.key");
  string(item.label, "workspace.label");
  nullableString(item.branch, "workspace.branch");
  nullableString(item.head, "workspace.head");
  boolean(item.detached, "workspace.detached");
  boolean(item.dirty, "workspace.dirty");
}

function laneSummary(value: unknown): void {
  const lane = record(value, "lane");
  string(lane.id, "lane.id");
  string(lane.title, "lane.title");
  if (lane.state !== "prepared" && lane.state !== "executing") {
    invalid("lane.state");
  }
  string(lane.phase_id, "lane.phase_id");
  string(lane.phase_title, "lane.phase_title");
  string(lane.phase_status, "lane.phase_status");
  boolean(lane.focused_here, "lane.focused_here");
}

function laneDetails(value: unknown): void {
  laneSummary(value);
  const lane = record(value, "focused_lane");
  string(lane.intent, "focused_lane.intent");
  string(lane.created_at, "focused_lane.created_at");
  string(lane.updated_at, "focused_lane.updated_at");
}

function phase(value: unknown): void {
  const item = record(value, "phase");
  string(item.id, "phase.id");
  string(item.title, "phase.title");
  string(item.status, "phase.status");
  array(item.goals, "phase.goals").forEach((goal) => {
    const goalItem = record(goal, "goal");
    string(goalItem.id, "goal.id");
    string(goalItem.title, "goal.title");
    string(goalItem.status, "goal.status");
    array(goalItem.tasks, "goal.tasks").forEach((task) => {
      const taskItem = record(task, "task");
      string(taskItem.id, "task.id");
      string(taskItem.title, "task.title");
      string(taskItem.status, "task.status");
    });
  });
}

function steering(value: unknown): void {
  const item = record(value, "steering");
  string(item.situation, "steering.situation");
  array(item.next_actions, "steering.next_actions").forEach((action) => {
    const actionItem = record(action, "suggested action");
    string(actionItem.label, "suggested_action.label");
    string(actionItem.command, "suggested_action.command");
    string(actionItem.rationale, "suggested_action.rationale");
    string(actionItem.intent, "suggested_action.intent");
    if (actionItem.confidence !== null) {
      finiteNumber(
        actionItem.confidence,
        "suggested_action.confidence",
      );
    }
  });
}

function workbenchDiagnostic(value: unknown): void {
  const item = record(value, "diagnostic");
  string(item.code, "diagnostic.code");
  if (
    item.severity !== "info" &&
    item.severity !== "warning" &&
    item.severity !== "error"
  ) {
    invalid("diagnostic.severity");
  }
  string(item.message, "diagnostic.message");
}

function record(value: unknown, field: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    invalid(field);
  }
  return value as Record<string, unknown>;
}

function array(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value)) {
    invalid(field);
  }
  return value;
}

function string(value: unknown, field: string): asserts value is string {
  if (typeof value !== "string") {
    invalid(field);
  }
}

function nullableString(
  value: unknown,
  field: string,
): asserts value is string | null {
  if (value !== null && typeof value !== "string") {
    invalid(field);
  }
}

function boolean(value: unknown, field: string): asserts value is boolean {
  if (typeof value !== "boolean") {
    invalid(field);
  }
}

function finiteNumber(value: unknown, field: string): asserts value is number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    invalid(field);
  }
}

function literal<T>(
  value: unknown,
  expected: T,
  field: string,
): asserts value is T {
  if (value !== expected) {
    invalid(field);
  }
}

function invalid(field: string): never {
  throw new Error(`Invalid workbench snapshot field: ${field}`);
}
