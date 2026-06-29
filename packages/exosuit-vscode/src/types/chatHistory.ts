/**
 * TypeScript types for exohistory JSON output.
 *
 * These types mirror the Rust structs in crates/exohistory/src/output.rs
 * and provide type safety for chat history data in the VS Code extension.
 */

/**
 * A single conversation turn (user message + assistant response).
 */
export interface ChatHistoryTurn {
  /** 1-based index of this turn in the session */
  turn_index: number;
  /** ISO timestamp (currently always null - VS Code doesn't persist reliably) */
  timestamp: string | null;
  /** User's message text */
  user: string;
  /** Assistant's response text */
  assistant: string;
  /** Extended thinking content (only present if include-thinking was set) */
  thinking?: string;
  /** Tool invocation messages (only present if include-tools was set) */
  tools?: string[];
}

/**
 * Output from `exohistory recent` command.
 */
export interface RecentTurnsOutput {
  /** Session ID (VS Code's internal identifier) */
  session_id: string;
  /** Workspace name (extracted from path) */
  workspace: string | null;
  /** Total number of turns in the session */
  total_turns: number;
  /** Number of turns actually retrieved */
  retrieved_turns: number;
  /** The conversation turns */
  turns: ChatHistoryTurn[];
  /** Note about truncation (present if not all turns were retrieved) */
  note?: string;
}

/**
 * Output from `exo ai chat-history` CLI command.
 *
 * Wraps the exohistory output in a standard CLI envelope.
 */
export interface AiChatHistoryOutput {
  kind: "ai.chat-history";
  ok: boolean;
  error?: string;
  data?:
    | RecentTurnsOutput
    | AmbiguousSessionsOutput
    | BeforeSummaryOutput
    | NoSummaryOutput;
}

/**
 * Output when --before-summary is used and turns are found before the summary.
 */
export interface BeforeSummaryOutput {
  /** Session ID */
  session_id: string;
  /** Workspace name */
  workspace: string | null;
  /** Turn number where the summary was found */
  summary_at_turn: number;
  /** Total turns in the session */
  total_turns: number;
  /** Number of turns retrieved */
  retrieved_turns: number;
  /** The conversation turns before the summary */
  turns: ChatHistoryTurn[];
  /** Note about truncation */
  note?: string;
}

/**
 * Output when --before-summary is used but no summary was found.
 */
export interface NoSummaryOutput {
  /** Always true for this response type */
  no_summary: true;
  /** Human-readable explanation */
  message: string;
  /** Hint for what to do */
  hint: string;
}

/**
 * Output when multiple sessions are active within a short time window.
 *
 * Returned instead of RecentTurnsOutput when session selection is ambiguous
 * and no match-text was provided to disambiguate.
 */
export interface AmbiguousSessionsOutput {
  /** Always true for this response type */
  ambiguous: true;
  /** Human-readable explanation */
  message: string;
  /** Time window in seconds used for ambiguity detection */
  threshold_seconds: number;
  /** Sessions that are candidates for selection */
  candidates: CandidateSession[];
  /** Hint for how to resolve the ambiguity */
  hint: string;
}

/**
 * A candidate session in an ambiguous selection.
 */
export interface CandidateSession {
  /** Session ID */
  session_id: string;
  /** Workspace name (extracted from path) */
  workspace: string | null;
  /** Number of requests in the session */
  request_count: number;
  /** Last activity timestamp (ISO format) */
  last_active: string | null;
}

/**
 * Type guard to check if the response is ambiguous.
 */
export function isAmbiguousResponse(
  data: AiChatHistoryOutput["data"],
): data is AmbiguousSessionsOutput {
  return data !== undefined && "ambiguous" in data && data.ambiguous === true;
}

/**
 * Type guard to check if the response indicates no summary was found.
 */
export function isNoSummaryResponse(
  data: AiChatHistoryOutput["data"],
): data is NoSummaryOutput {
  return data !== undefined && "no_summary" in data && data.no_summary === true;
}

/**
 * Type guard to check if the response is a before-summary output.
 */
export function isBeforeSummaryResponse(
  data: AiChatHistoryOutput["data"],
): data is BeforeSummaryOutput {
  return data !== undefined && "summary_at_turn" in data;
}

/**
 * Session reference for session listing/discovery.
 */
export interface SessionRef {
  /** Session ID */
  id: string;
  /** Workspace ID (VS Code internal) */
  workspace_id: string;
  /** Workspace filesystem path (if available) */
  workspace_path: string | null;
  /** Number of requests in the session */
  request_count: number;
  /** Last message timestamp (ISO format, may be null for large sessions) */
  last_message_at: string | null;
}
