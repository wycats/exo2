export const MACHINE_CHANNEL_PROTOCOL_VERSION = 1 as const;

export type MachineChannelStatus =
  | "ok"
  | "needs_input"
  | "confirm_required"
  | "error";

export type MachineChannelOpKind = "help" | "list" | "call" | "preview";

export type MachineChannelAddress =
  | { kind: "root" }
  | { kind: "namespace"; path: string[] }
  | { kind: "operation"; path: string[] };

export type MachineChannelHelpParams = {
  address: MachineChannelAddress;
};

export type MachineChannelListParams = {
  address: MachineChannelAddress;
  kind: string;
  page: { cursor?: string | null; limit?: number };
};

export type MachineChannelCallParams = {
  address: MachineChannelAddress;
  input: unknown;
};

export const WORKFLOW_COMPLETION_CONFIRMATION_KIND =
  "workflow_completion_confirmation" as const;

export type WorkflowCompletionConfirmationKind =
  typeof WORKFLOW_COMPLETION_CONFIRMATION_KIND;

export type MachineChannelRequestEnvelope = {
  protocol_version: number;
  id: string;
  workspace_root?: string;
  op: {
    kind: MachineChannelOpKind;
    params:
      | MachineChannelHelpParams
      | MachineChannelListParams
      | MachineChannelCallParams;
  };
  auth?: {
    ticket: string;
    confirm?: boolean;
  };
  workflow_confirmation?: {
    kind: WorkflowCompletionConfirmationKind;
    entity_type: string;
    entity_id: string;
    decision:
      | "yes_complete"
      | "revise_outcome"
      | "not_complete_yet"
      | "discuss";
    outcome: string;
  };
  /** Agent session identity (chatSessionResource URI). Omit for sidebar/CLI. */
  agent_id?: string;
};

export interface Steering {
  next_call?: { kind: MachineChannelOpKind; params?: Record<string, unknown> };
  priority?: string;
  confidence?: number;
  context_note?: string;
  completion_digests?: Array<{
    entity_type: string;
    entity_id: string;
    count: number;
    claims: Array<{
      id: string;
      status: string;
      source: string;
      priority: string;
      confidence?: string;
      agent_id?: string;
      subject: string;
      body: string;
      created: string;
    }>;
    drill_in: string;
  }>;
}

/** Preview-specific display metadata returned by Op::Preview. */
export type MachineChannelPreviewDisplay = {
  /** Message shown while the operation is running */
  invocation_message: string;
  /** Past-tense message shown after tool completion (proposed API: chatParticipantPrivate) */
  past_tense_message?: string;
  /** Optional confirmation dialog for destructive operations */
  confirmation?: MachineChannelConfirmationInfo;
};

/** Confirmation dialog metadata for destructive operations. */
export type MachineChannelConfirmationInfo = {
  /** Short title for the confirmation dialog */
  title: string;
  /** Longer explanation of what this action does */
  message: string;
};

export type WorkflowConfirmationOption = {
  label: string;
  value: string;
  description?: string;
};

export type CompletionOutcomeDigest = {
  entity_type: string;
  entity_id: string;
  count: number;
  claims: Array<{
    id: string;
    status: string;
    source: string;
    priority: string;
    confidence?: string;
    agent_id?: string;
    subject: string;
    body: string;
    created: string;
  }>;
  drill_in: string;
};

export type WorkflowCompletionConfirmation = {
  kind: WorkflowCompletionConfirmationKind;
  entity_type?: string;
  entity_id?: string;
  completion_input?: {
    kind: WorkflowCompletionConfirmationKind;
    entity_type: string;
    entity_id: string;
    decision:
      | "yes_complete"
      | "revise_outcome"
      | "not_complete_yet"
      | "discuss";
    outcome: string;
  };
  completion_digest?: CompletionOutcomeDigest;
  header: string;
  question: string;
  message: string;
  readiness_rationale: string;
  proposed_outcome: string;
  options: WorkflowConfirmationOption[];
  branch_instructions?: {
    yes_complete?: string;
    revise_outcome?: string;
    not_complete_yet?: string;
    discuss?: string;
  };
};

export type MachineChannelResponseEnvelope = {
  protocol_version: number;
  id: string;
  status: MachineChannelStatus;
  result?: unknown;
  error?: {
    code: string;
    message: string;
    details?: unknown;
  };
  ticket?: string;
  steering?: Steering;
  reminders?: Array<{
    kind: string;
    severity: "warning" | "error";
    message: string;
    details?: unknown;
  }>;
  display?: {
    /** Short message shown while the operation is running */
    invocation_message: string;
    /** One-line summary of the result */
    summary: string;
    /** Full human-readable body (markdown). If absent, summary is the body. */
    body?: string;
  };
  /** Preview-only display metadata. Present on Op::Preview responses. */
  preview?: MachineChannelPreviewDisplay;
  /** The command's declared effect (pure/write/exec). */
  effect?: "pure" | "write" | "exec";
  /** Reactive trace captured during command execution (opaque token). */
  trace?: unknown;
};
