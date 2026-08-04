export interface WorkbenchLaunchResult {
  kind: "workbench.launch";
  ok: true;
  schema_version: 1;
  url: string;
  expires_at: string;
  expires_in_seconds: 3600;
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
  schema_version: 2;
  observed_at: string;
  revision: number;
  project: {
    id: string;
  };
  daemon: {
    instance_id: string;
  };
  workspace: WorkbenchSnapshotWorkspace;
  lanes: WorkbenchLaneSummary[];
  focused_lane: WorkbenchLaneDetails | null;
  phase: WorkbenchPhase | null;
  between_phases_context: WorkbenchBetweenPhasesContext | null;
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
  planning_available: boolean;
  goals: WorkbenchGoal[];
}

export interface WorkbenchBetweenPhasesContext {
  epoch_id: string;
  epoch_title: string;
  completed_phase: WorkbenchCompletedPhaseSummary | null;
  next_phase: WorkbenchNextPhasePreview | null;
  pending_phases: number;
}

export interface WorkbenchCompletedPhaseSummary {
  id: string;
  title: string;
  completed_at: string;
  goal_count: number;
  completed_goals: number;
}

export interface WorkbenchNextPhasePreview {
  id: string;
  title: string;
  goal_count: number;
  rfc_count: number;
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
  progress?: WorkbenchTaskProgress[];
  progress_truncated?: boolean;
}

export interface WorkbenchTaskProgress {
  message: string;
  created_at: string;
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
  | {
      kind: "task_complete_approve";
      review_id: string;
      task_id: string;
      outcome: string;
    };

export interface WorkbenchPlanningBinding {
  expected_daemon_instance_id: string;
  expected_revision: number;
  expected_phase_id: string;
}

export interface WorkbenchPlanningRequest {
  protocol_version: 2;
  id: string;
  session_key: string;
  expected_daemon_instance_id: string;
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

export function workbenchPlanningBinding(
  snapshot: WorkbenchSnapshot,
): WorkbenchPlanningBinding | null {
  const lane = snapshot.focused_lane;
  const phase = snapshot.phase;
  if (
    lane === null ||
    phase === null ||
    !lane.focused_here ||
    lane.phase_id !== phase.id ||
    lane.phase_status !== "in-progress" ||
    phase.status !== "in-progress" ||
    !phase.planning_available ||
    snapshot.diagnostics.some(
      (diagnostic) => diagnostic.code === "lane.phase_focus_mismatch",
    )
  ) {
    return null;
  }
  return {
    expected_daemon_instance_id: snapshot.daemon.instance_id,
    expected_revision: snapshot.revision,
    expected_phase_id: phase.id,
  };
}

export function decodeWorkbenchSnapshot(value: unknown): WorkbenchSnapshot {
  const snapshot = record(value, "workbench snapshot");
  literal(snapshot.kind, "workbench.snapshot", "kind");
  literal(snapshot.ok, true, "ok");
  literal(snapshot.schema_version, 2, "schema_version");
  string(snapshot.observed_at, "observed_at");
  finiteNumber(snapshot.revision, "revision");
  project(snapshot.project);
  string(record(snapshot.daemon, "daemon").instance_id, "daemon.instance_id");
  workspace(snapshot.workspace);
  array(snapshot.lanes, "lanes").forEach((lane) => laneSummary(lane));
  if (snapshot.focused_lane !== null) {
    laneDetails(snapshot.focused_lane);
  }
  if (snapshot.phase !== null) {
    phase(snapshot.phase);
  }
  if (snapshot.between_phases_context !== null) {
    betweenPhasesContext(snapshot.between_phases_context);
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
  boolean(item.planning_available, "phase.planning_available");
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
      if (taskItem.progress !== undefined) {
        array(taskItem.progress, "task.progress").forEach((progress) => {
          const progressItem = record(progress, "task progress");
          string(progressItem.message, "task.progress.message");
          string(progressItem.created_at, "task.progress.created_at");
        });
      }
      if (taskItem.progress_truncated !== undefined) {
        boolean(
          taskItem.progress_truncated,
          "task.progress_truncated",
        );
      }
    });
  });
}

function betweenPhasesContext(value: unknown): void {
  const item = record(value, "between_phases_context");
  string(item.epoch_id, "between_phases_context.epoch_id");
  string(item.epoch_title, "between_phases_context.epoch_title");
  count(item.pending_phases, "between_phases_context.pending_phases");

  if (item.completed_phase !== null) {
    const completed = record(
      item.completed_phase,
      "between_phases_context.completed_phase",
    );
    string(completed.id, "between_phases_context.completed_phase.id");
    string(completed.title, "between_phases_context.completed_phase.title");
    string(
      completed.completed_at,
      "between_phases_context.completed_phase.completed_at",
    );
    count(
      completed.goal_count,
      "between_phases_context.completed_phase.goal_count",
    );
    count(
      completed.completed_goals,
      "between_phases_context.completed_phase.completed_goals",
    );
  }

  if (item.next_phase !== null) {
    const next = record(item.next_phase, "between_phases_context.next_phase");
    string(next.id, "between_phases_context.next_phase.id");
    string(next.title, "between_phases_context.next_phase.title");
    count(next.goal_count, "between_phases_context.next_phase.goal_count");
    count(next.rfc_count, "between_phases_context.next_phase.rfc_count");
  }
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

function count(value: unknown, field: string): asserts value is number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < 0
  ) {
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
