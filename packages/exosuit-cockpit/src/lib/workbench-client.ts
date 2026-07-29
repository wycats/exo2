import {
  decodeWorkbenchSnapshot,
  type WorkbenchCommandRequest,
  type WorkbenchSessionResult,
  type WorkbenchSnapshot,
} from "./workbench";

const SESSION_HISTORY_KEY = "exoWorkbenchSessionKey";
const CROCKFORD_BASE32 = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const REQUEST_TIMEOUT_MS = 10_000;

export type WorkbenchFailureKind =
  | "session_required"
  | "session_expired"
  | "workspace_unavailable"
  | "transport_error"
  | "command_failed";

export class WorkbenchClientError extends Error {
  constructor(
    readonly kind: WorkbenchFailureKind,
    message: string,
    readonly retryable = false,
  ) {
    super(message);
    this.name = "WorkbenchClientError";
  }

  get retryWithSameRequestId(): boolean {
    return this.kind === "transport_error";
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
  if (typeof state !== "object" || state === null || Array.isArray(state)) {
    return null;
  }
  const sessionKey = (state as Record<string, unknown>)[SESSION_HISTORY_KEY];
  return typeof sessionKey === "string" && sessionKey.length > 0
    ? sessionKey
    : null;
}

export function retainSessionSelector(
  history: Pick<History, "replaceState" | "state">,
  location: Pick<Location, "pathname" | "search">,
  sessionKey: string,
): void {
  const prior =
    typeof history.state === "object" &&
    history.state !== null &&
    !Array.isArray(history.state)
      ? history.state
      : {};
  history.replaceState(
    { ...prior, [SESSION_HISTORY_KEY]: sessionKey },
    "",
    `${location.pathname}${location.search}`,
  );
}

export function prepareWorkbenchTicketExchange(
  history: Pick<History, "replaceState" | "state">,
  location: Pick<Location, "pathname" | "search">,
): void {
  const prior =
    typeof history.state === "object" &&
    history.state !== null &&
    !Array.isArray(history.state)
      ? { ...history.state }
      : {};
  delete prior[SESSION_HISTORY_KEY];
  history.replaceState(prior, "", `${location.pathname}${location.search}`);
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
        "transport_error",
        "Exo returned an invalid workbench snapshot",
        true,
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

  eventSourceUrl(): string {
    return `/api/events?${new URLSearchParams({
      session_key: this.sessionKey,
    })}`;
  }

  private async dispatch(requestBody: WorkbenchCommandRequest): Promise<unknown> {
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
      throw new WorkbenchClientError(
        "command_failed",
        envelope.error?.message ?? "The workbench command failed",
        envelope.error?.code === "precondition_failed" ||
          envelope.error?.code === "internal",
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
  } finally {
    clearTimeout(timeout);
  }

  let value: unknown;
  try {
    value = await response.json();
  } catch {
    throw new WorkbenchClientError(
      "transport_error",
      "Exo returned an unreadable workbench response",
      true,
    );
  }

  if (!response.ok) {
    throw httpFailure(response.status, value);
  }
  return value;
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
      return new WorkbenchClientError("transport_error", message, true);
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
