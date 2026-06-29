/**
 * Progress mode enum matching CLI's ProgressMode.
 * Serialized as kebab-case in JSON.
 *
 * @see RFC 00184: Mode-Aware Sidebar Cockpit
 * @see RFC 00187: Collapsed Transitioning into context-aware BetweenPhases
 */
import type { SidecarRepoSyncStatus } from "./sidecarStatus";

export type ProgressMode =
  | "roadmap-revision"
  | "between-epochs"
  | "between-phases"
  | "planning"
  | "executing"
  | "verifying";

/**
 * Git change summary from `exo status --json`.
 */
export interface GitSummary {
  modified: number;
  added: number;
  deleted: number;
  untracked: number;
}

/** Next phase preview for between-phases context */
export interface NextPhasePreview {
  id: string;
  title: string;
  goal_count: number;
  rfcs: string[];
}

/** Completed phase context for between-phases mode (RFC 00187) */
export interface CompletedPhaseContext {
  phase_id: string;
  phase_title: string;
  completion_log?: string;
  goal_count: number;
  completed_goals: number;
}

/** Context data for between-phases mode (RFC 00187: context-aware BetweenPhases) */
export interface BetweenPhasesContext {
  /** The most recently completed phase in the active epoch */
  completed_phase?: CompletedPhaseContext;
  /** The next pending phase to start */
  next_phase?: NextPhasePreview;
  /** Current epoch info */
  epoch_id: string;
  epoch_title: string;
  /** Whether this is the last phase in the epoch */
  is_epoch_finale: boolean;
}

/**
 * Perception summary from steering output.
 */
export interface PerceptionSummary {
  entity_type: string;
  entity_id?: string;
  count: number;
  highest_priority: string;
  sample_subject: string;
  subjects?: string[];
  drill_in: string;
}

/** Completion outcome body preserved for review surfaces. */
export interface CompletionOutcomeDigestClaim {
  id: string;
  status: string;
  source: string;
  priority: string;
  confidence?: string;
  agent_id?: string;
  subject: string;
  body: string;
  created: string;
}

/** Completion outcomes grouped by entity from steering output. */
export interface CompletionOutcomeDigestSummary {
  entity_type: string;
  entity_id: string;
  count: number;
  claims: CompletionOutcomeDigestClaim[];
  drill_in: string;
}

/** Steering block from `exo status --json`. */
export interface SteeringBlock {
  primary_intent: string;
  progress_mode: ProgressMode;
  situation: string;
  perception_summaries: PerceptionSummary[];
  completion_digests?: CompletionOutcomeDigestSummary[];
  rfc_context?: unknown[];
  session_boundary?: {
    boundary_type: string;
    confidence: number;
    rationale: string;
  };
  entity_context?: {
    entity_type: string;
    entity_id: string;
    ancestors: [string, string][];
  };
}

/**
 * Status response from `exo status --json`.
 *
 * Matches the Rust `StatusJson` struct in tools/exo/src/status.rs
 */
export interface ExoStatusResponse {
  /** Current phase ID, if any */
  phase_id?: string;
  /** Current phase title, if any */
  phase_title?: string;
  /** Current epoch title, if any */
  epoch_title?: string;
  /** Whether the git working tree is dirty */
  git_dirty: boolean;
  /** Summary of git changes */
  git_summary?: GitSummary;
  /** Sidecar git repository sync health, when this project uses sidecar state. */
  sidecar_sync?: SidecarRepoSyncStatus;
  /** Current progress mode */
  progress_mode: ProgressMode;
  /** Full steering block */
  steering: SteeringBlock;
  /** Count of pending goals in current phase */
  pending_goals: number;
  /** Count of completed goals in current phase */
  completed_goals: number;
  /** Between-phases context (RFC 00187: context-aware BetweenPhases) */
  between_phases_context?: BetweenPhasesContext;
}

/**
 * Check if a progress mode is a "between" state (strategic overview modes).
 */
export function isBetweenState(mode: ProgressMode): boolean {
  return (
    mode === "roadmap-revision" ||
    mode === "between-epochs" ||
    mode === "between-phases"
  );
}
