export type ExosuitListKind = "tasks" | "recipes" | "ports" | "artifacts";

export type ExosuitStatus =
  | "ok"
  | "needs_selection"
  | "needs_input"
  | "needs_confirmation"
  | "error";

export type ExosuitCode =
  | "OK"
  | "AMBIGUOUS"
  | "MISSING_INPUT"
  | "CONFIRM_REQUIRED"
  | "NOT_FOUND"
  | "INVALID_TICKET"
  | "FORBIDDEN"
  | "INVALID_INPUT"
  | "INTERNAL";

export type ExosuitToolInput =
  | {
      list: {
        kind: ExosuitListKind;
        prefix?: string | null;
        limit?: number;
      };
    }
  | {
      run: {
        targetKind?: "recipe" | "task" | null;
        targetId?: string | null;
        dryRun?: boolean;
      };
    }
  | {
      locate: {
        what?: "artifacts" | "context" | "rfc" | "docs" | null;
        id?: string | null;
      };
    }
  | {
      edit: {
        resource?: "plan" | "tasks" | "walkthrough" | "ideas" | null;
        action?: "add" | "update" | "append" | null;
        input?: Record<string, unknown>;
      };
    }
  | {
      use: {
        ticket: string;
        input?: Record<string, unknown>;
        confirm?: boolean;
      };
    };

export interface ExosuitToolItem {
  kind: string;
  id: string;
  label: string;
  description?: string;
  path?: string;
  exists?: boolean;
}

export type ExosuitToolResult = { type: string; data: unknown } | null;

export type ExosuitSteeringNextCall = ExosuitToolInput;

export interface ExosuitSteeringInternal {
  nextCall: ExosuitSteeringNextCall | null;
  alternatives?: Array<{ kind: string; id: string; label: string }>;
  listHint?: ExosuitToolInput | null;
}

export interface ExosuitToolOutputInternal {
  status: ExosuitStatus;
  code: ExosuitCode;
  message: string;
  result: ExosuitToolResult;
  ticket: string | null;
  steering: ExosuitSteeringInternal;
}

export function toWire(output: ExosuitToolOutputInternal): unknown {
  const { steering, ...rest } = output;
  return {
    ...rest,
    steering: {
      ["next_call"]: steering.nextCall,
      alternatives: steering.alternatives,
      ["list_hint"]: steering.listHint,
    },
  };
}

export function ok(
  result: ExosuitToolResult,
  message = "OK"
): ExosuitToolOutputInternal {
  return {
    status: "ok",
    code: "OK",
    message,
    result,
    ticket: null,
    steering: {
      nextCall: null,
    },
  };
}

export function needsInput(
  message: string,
  nextCall: ExosuitSteeringNextCall
): ExosuitToolOutputInternal {
  return {
    status: "needs_input",
    code: "MISSING_INPUT",
    message,
    result: null,
    ticket: null,
    steering: {
      nextCall,
    },
  };
}

export function invalidInput(
  message: string,
  nextCall: ExosuitSteeringNextCall
): ExosuitToolOutputInternal {
  return {
    status: "error",
    code: "INVALID_INPUT",
    message,
    result: null,
    ticket: null,
    steering: {
      nextCall,
    },
  };
}

export function notFound(
  message: string,
  nextCall: ExosuitSteeringNextCall
): ExosuitToolOutputInternal {
  return {
    status: "error",
    code: "NOT_FOUND",
    message,
    result: null,
    ticket: null,
    steering: {
      nextCall,
    },
  };
}

export function invalidTicket(
  message: string,
  nextCall: ExosuitSteeringNextCall
): ExosuitToolOutputInternal {
  return {
    status: "error",
    code: "INVALID_TICKET",
    message,
    result: null,
    ticket: null,
    steering: {
      nextCall,
    },
  };
}

export function needsConfirmation(
  message: string,
  ticket: string,
  nextCall: ExosuitSteeringNextCall
): ExosuitToolOutputInternal {
  return {
    status: "needs_confirmation",
    code: "CONFIRM_REQUIRED",
    message,
    result: null,
    ticket,
    steering: {
      nextCall,
    },
  };
}

export function internalError(
  message: string,
  nextCall: ExosuitSteeringNextCall
): ExosuitToolOutputInternal {
  return {
    status: "error",
    code: "INTERNAL",
    message,
    result: null,
    ticket: null,
    steering: {
      nextCall,
    },
  };
}

export function normalizeLimit(limit: number | undefined): number | undefined {
  if (limit === undefined) {return undefined;}
  if (!Number.isFinite(limit)) {return undefined;}
  const rounded = Math.floor(limit);
  if (rounded < 1 || rounded > 50) {return undefined;}
  return rounded;
}

export function applyPrefixAndLimit<T extends { id: string }>(
  items: T[],
  prefix: string | null | undefined,
  limit: number | undefined
): T[] {
  const normalizedPrefix = (prefix ?? "").trim();
  let filtered = items;

  if (normalizedPrefix.length > 0) {
    filtered = filtered.filter((item) => item.id.startsWith(normalizedPrefix));
  }

  const effectiveLimit = limit ?? 20;
  return filtered.slice(0, effectiveLimit);
}
