import {
  decodeWorkbenchLaneInspection,
  decodeWorkbenchPlanningResult,
  decodeWorkbenchSnapshot,
  type WorkbenchCommandRequest,
  type WorkbenchLaneInspection,
  type WorkbenchPlanningRequest,
  type WorkbenchPlanningResult,
  type WorkbenchSessionResult,
  type WorkbenchSnapshot,
} from "./workbench";

const SESSION_HISTORY_KEY = "exoWorkbenchSessionKey";
const SVELTEKIT_HISTORY_INDEX_KEY = "sveltekit:history";
const SVELTEKIT_NAVIGATION_INDEX_KEY = "sveltekit:navigation";
const SVELTEKIT_PAGE_STATE_KEY = "sveltekit:states";
const CROCKFORD_BASE32 = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const REQUEST_TIMEOUT_MS = 10_000;

export type WorkbenchFailureKind =
  | "session_required"
  | "session_expired"
  | "client_update_required"
  | "server_busy"
  | "workspace_unavailable"
  | "transport_error"
  | "command_failed";

export class WorkbenchClientError extends Error {
  constructor(
    readonly kind: WorkbenchFailureKind,
    message: string,
    readonly retryable = false,
    private readonly sameRequestId = kind === "transport_error",
    readonly detailKind: string | null = null,
  ) {
    super(message);
    this.name = "WorkbenchClientError";
  }

  get retryWithSameRequestId(): boolean {
    return this.sameRequestId;
  }
}

type Fetcher = typeof fetch;

interface BrowserCommandEnvelope {
  protocol_version: number;
  id: string;
  status: "ok" | "needs_input" | "confirm_required" | "error";
  result?: unknown;
  error?: {
    code?: string;
    message?: string;
    details?: {
      kind?: string;
      retry_with_same_request_id?: boolean;
    };
  };
}

interface HttpErrorBody {
  kind?: string;
  message?: string;
}

export function launchTicketFromHash(hash: string): string | null {
  const fragment = hash.startsWith("#") ? hash.slice(1) : hash;
  const ticket = new URLSearchParams(fragment).get("ticket")?.trim();
  return ticket || null;
}

export function sessionKeyFromHistory(state: unknown): string | null {
  const sessionKey = workbenchHistoryState(state)[SESSION_HISTORY_KEY];
  return typeof sessionKey === "string" && sessionKey.length > 0
    ? sessionKey
    : null;
}

export function workbenchHistoryState(
  state: unknown,
): Record<string, unknown> {
  if (typeof state !== "object" || state === null || Array.isArray(state)) {
    return {};
  }

  const historyState = state as Record<string, unknown>;
  const pageState = historyState[SVELTEKIT_PAGE_STATE_KEY];
  if (
    typeof pageState === "object" &&
    pageState !== null &&
    !Array.isArray(pageState)
  ) {
    return {
      ...Object.fromEntries(
        Object.entries(historyState).filter(
          ([key]) => !key.startsWith("sveltekit:"),
        ),
      ),
      ...(pageState as Record<string, unknown>),
    };
  }
  if (
    SVELTEKIT_HISTORY_INDEX_KEY in historyState ||
    SVELTEKIT_NAVIGATION_INDEX_KEY in historyState ||
    SVELTEKIT_PAGE_STATE_KEY in historyState
  ) {
    return Object.fromEntries(
      Object.entries(historyState).filter(([key]) => !key.startsWith("sveltekit:")),
    );
  }
  return historyState;
}

export function retainSessionSelector(
  state: unknown,
  sessionKey: string,
): Record<string, unknown> {
  return {
    ...workbenchHistoryState(state),
    [SESSION_HISTORY_KEY]: sessionKey,
  };
}

export function prepareWorkbenchTicketExchange(
  state: unknown,
): Record<string, unknown> {
  const prior = { ...workbenchHistoryState(state) };
  delete prior[SESSION_HISTORY_KEY];
  return prior;
}

export async function exchangeWorkbenchTicket(
  ticket: string,
  fetcher: Fetcher = fetch,
): Promise<WorkbenchSessionResult> {
  try {
    const response = await request(
      fetcher,
      "/api/session",
      { ticket },
      "The workbench session could not be opened",
    );
    return decodeSession(response);
  } catch (error) {
    if (
      error instanceof WorkbenchClientError &&
      error.kind === "transport_error"
    ) {
      throw new WorkbenchClientError(
        "session_required",
        "The launch link could not be confirmed. Open a fresh Exo workbench link.",
      );
    }
    throw error;
  }
}

export function createWorkbenchRequestId(now = Date.now()): string {
  let time = now;
  let encodedTime = "";
  for (let index = 0; index < 10; index += 1) {
    encodedTime =
      CROCKFORD_BASE32[time % 32] + encodedTime;
    time = Math.floor(time / 32);
  }

  const random = new Uint8Array(16);
  crypto.getRandomValues(random);
  let encodedRandom = "";
  for (const value of random) {
    encodedRandom += CROCKFORD_BASE32[value & 31];
  }
  return `${encodedTime}${encodedRandom}`;
}

export class WorkbenchClient {
  constructor(
    readonly sessionKey: string,
    private readonly fetcher: Fetcher = fetch,
  ) {}

  async renewSession(): Promise<WorkbenchSessionResult> {
    const session = decodeSession(
      await request(
        this.fetcher,
        "/api/session/renew",
        { session_key: this.sessionKey },
        "The workbench session could not be renewed",
      ),
    );
    if (session.session_key !== this.sessionKey) {
      throw new WorkbenchClientError(
        "transport_error",
        "Exo renewed a different workbench session",
        true,
      );
    }
    return session;
  }

  async snapshot(): Promise<WorkbenchSnapshot> {
    const result = await this.dispatch({
      protocol_version: 1,
      id: createWorkbenchRequestId(),
      session_key: this.sessionKey,
      operation: { kind: "snapshot" },
    });
    try {
      return decodeWorkbenchSnapshot(result);
    } catch {
      throw new WorkbenchClientError(
        "client_update_required",
        "This page cannot read the current Exo workbench snapshot. Reload to use the current workbench version.",
      );
    }
  }

  async inspectLane(laneId: string): Promise<WorkbenchLaneInspection> {
    const result = await this.dispatch({
      protocol_version: 1,
      id: createWorkbenchRequestId(),
      session_key: this.sessionKey,
      operation: { kind: "lane_inspect", lane_id: laneId },
    });
    try {
      return decodeWorkbenchLaneInspection(result);
    } catch {
      throw new WorkbenchClientError(
        "client_update_required",
        "This page cannot read the current lane inspection. Reload to use the current workbench version.",
      );
    }
  }

  async focusLane(laneId: string, requestId: string): Promise<void> {
    const command: WorkbenchCommandRequest = {
      protocol_version: 1,
      id: requestId,
      session_key: this.sessionKey,
      operation: { kind: "lane_focus", lane_id: laneId },
    };

    try {
      await this.dispatch(command);
    } catch (error) {
      if (!isTransportFailure(error)) {
        throw error;
      }
      await this.dispatch(command);
    }
  }

  async planning(
    command: WorkbenchPlanningRequest,
  ): Promise<WorkbenchPlanningResult> {
    const result = await this.dispatch(command);
    try {
      return decodeWorkbenchPlanningResult(result);
    } catch {
      throw new WorkbenchClientError(
        "transport_error",
        "Exo returned an invalid workbench planning result",
        true,
      );
    }
  }

  eventSourceUrl(): string {
    return `/api/events?${new URLSearchParams({
      session_key: this.sessionKey,
    })}`;
  }

  private async dispatch(
    requestBody: WorkbenchCommandRequest | WorkbenchPlanningRequest,
  ): Promise<unknown> {
    const value = await request(
      this.fetcher,
      "/api/command",
      requestBody,
      "The workbench command could not reach Exo",
    );
    const envelope = decodeCommandEnvelope(value);
    if (envelope.id !== requestBody.id) {
      throw new WorkbenchClientError(
        "transport_error",
        "Exo returned a response for a different request",
        true,
      );
    }
    if (envelope.status !== "ok") {
      const sameRequestId =
        envelope.error?.details?.retry_with_same_request_id ?? false;
      const detailKind = envelope.error?.details?.kind ?? null;
      const receivedRetryPolicy =
        envelope.error?.details?.retry_with_same_request_id !== undefined;
      throw new WorkbenchClientError(
        detailKind === "workbench.busy" ? "server_busy" : "command_failed",
        envelope.error?.message ?? "The workbench command failed",
        sameRequestId ||
          (!receivedRetryPolicy &&
            (envelope.error?.code === "precondition_failed" ||
              envelope.error?.code === "internal")),
        sameRequestId,
        detailKind,
      );
    }
    return envelope.result;
  }
}

async function request(
  fetcher: Fetcher,
  path: string,
  body: unknown,
  transportMessage: string,
): Promise<unknown> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    let response: Response;
    try {
      response = await fetcher(path, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: controller.signal,
      });
    } catch {
      throw new WorkbenchClientError(
        "transport_error",
        transportMessage,
        true,
      );
    }

    let value: unknown;
    try {
      value = await response.json();
    } catch {
      const contentType =
        response.headers.get("content-type")?.split(";")[0]?.trim() ||
        "unknown content type";
      throw new WorkbenchClientError(
        "transport_error",
        `Exo returned an unreadable workbench response (HTTP ${response.status}, ${contentType})`,
        true,
      );
    }

    if (!response.ok) {
      throw httpFailure(response.status, value);
    }
    return value;
  } finally {
    clearTimeout(timeout);
  }
}

function httpFailure(status: number, value: unknown): WorkbenchClientError {
  const error =
    typeof value === "object" && value !== null && !Array.isArray(value)
      ? (value as HttpErrorBody)
      : {};
  const message = error.message ?? "The workbench request failed";
  switch (error.kind) {
    case "workbench.ticket_invalid":
    case "workbench.session_invalid":
      return new WorkbenchClientError("session_expired", message);
    case "workbench.workspace_unavailable":
      return new WorkbenchClientError("workspace_unavailable", message);
    case "workbench.busy":
      return new WorkbenchClientError("server_busy", message, true);
    default:
      return new WorkbenchClientError(
        status >= 500 ? "transport_error" : "command_failed",
        message,
        status >= 500,
      );
  }
}

function decodeSession(value: unknown): WorkbenchSessionResult {
  const session = asRecord(value);
  if (
    session.kind !== "workbench.session" ||
    session.ok !== true ||
    session.schema_version !== 1 ||
    typeof session.session_key !== "string" ||
    typeof session.project_id !== "string" ||
    typeof session.workspace_key !== "string" ||
    typeof session.expires_at !== "string"
  ) {
    throw new WorkbenchClientError(
      "transport_error",
      "Exo returned an invalid workbench session",
      true,
    );
  }
  return value as WorkbenchSessionResult;
}

function decodeCommandEnvelope(value: unknown): BrowserCommandEnvelope {
  const envelope = asRecord(value);
  if (
    envelope.protocol_version !== 1 ||
    typeof envelope.id !== "string" ||
    (envelope.status !== "ok" &&
      envelope.status !== "needs_input" &&
      envelope.status !== "confirm_required" &&
      envelope.status !== "error")
  ) {
    throw new WorkbenchClientError(
      "transport_error",
      "Exo returned an invalid workbench command response",
      true,
    );
  }
  return value as BrowserCommandEnvelope;
}

function asRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return {};
  }
  return value as Record<string, unknown>;
}

function isTransportFailure(error: unknown): boolean {
  return (
    error instanceof WorkbenchClientError &&
    error.kind === "transport_error"
  );
}
