import type * as vscode from "vscode";
import { randomBytes } from "node:crypto";

import {
  TICKET_PREFIX,
  base64UrlDecode,
  base64UrlEncode,
  decodeBase64UrlToJson,
  encodeJsonToBase64Url,
  parseTicket,
  signPayloadB64,
  verifySignature,
} from "./ticketCodec";

export interface TicketPayload {
  v: 1;
  workspace: {
    roots: string[];
  };
  issuedAt: string;
  nonce: string;
  cap: {
    kind: string;
    data: unknown;
    constraints: {
      expiresAt: string;
      confirmRequired: boolean;
    };
    steerOnInvalid: unknown;
  };
}

const SECRET_KEY_ID = "exosuit.lmtool.ticketSigningKey.v1";
async function getOrCreateSigningKey(
  context: vscode.ExtensionContext
): Promise<Uint8Array<ArrayBufferLike>> {
  const existing = await context.secrets.get(SECRET_KEY_ID);
  if (existing) {
    return base64UrlDecode(existing);
  }

  const key = randomBytes(32);
  await context.secrets.store(SECRET_KEY_ID, base64UrlEncode(key));
  return new Uint8Array(key);
}

export async function mintTicket(options: {
  context: vscode.ExtensionContext;
  workspaceRoots: string[];
  capKind: string;
  capData: unknown;
  confirmRequired: boolean;
  expiresInMs: number;
  steerOnInvalid: unknown;
}): Promise<string> {
  const key = await getOrCreateSigningKey(options.context);

  const issuedAt = new Date();
  const expiresAt = new Date(issuedAt.getTime() + options.expiresInMs);

  const payload: TicketPayload = {
    v: 1,
    workspace: { roots: [...options.workspaceRoots].sort() },
    issuedAt: issuedAt.toISOString(),
    nonce: base64UrlEncode(randomBytes(12)),
    cap: {
      kind: options.capKind,
      data: options.capData,
      constraints: {
        expiresAt: expiresAt.toISOString(),
        confirmRequired: options.confirmRequired,
      },
      steerOnInvalid: options.steerOnInvalid,
    },
  };

  const payloadB64 = encodeJsonToBase64Url(payload);
  const sig = signPayloadB64(key, payloadB64);
  const sigB64 = base64UrlEncode(sig);

  return `${TICKET_PREFIX}.${payloadB64}.${sigB64}`;
}

export async function verifyTicket(options: {
  context: vscode.ExtensionContext;
  workspaceRoots: string[];
  ticket: string;
}): Promise<
  | { ok: true; payload: TicketPayload }
  | { ok: false; reason: string; steerOnInvalid: unknown }
> {
  const parsed = parseTicket(options.ticket);
  if (!parsed.ok) {
    return {
      ok: false,
      reason: parsed.reason,
      steerOnInvalid: null,
    };
  }

  const payloadB64 = parsed.payloadB64Url;
  const sigB64 = parsed.sigB64Url;

  let sigBytes: Uint8Array;

  try {
    // Verify base64 decoding works for payload (actual parsing done separately)
    base64UrlDecode(payloadB64);
    sigBytes = base64UrlDecode(sigB64);
  } catch {
    return {
      ok: false,
      reason: "Ticket base64 decode failed.",
      steerOnInvalid: null,
    };
  }

  let payload: TicketPayload;
  try {
    payload = decodeBase64UrlToJson<TicketPayload>(payloadB64);
  } catch {
    return {
      ok: false,
      reason: "Ticket payload JSON parse failed.",
      steerOnInvalid: null,
    };
  }

  const key = await getOrCreateSigningKey(options.context);

  if (!verifySignature({ key, payloadB64Url: payloadB64, sigBytes })) {
    return {
      ok: false,
      reason: "Ticket signature is invalid.",
      steerOnInvalid: payload?.cap?.steerOnInvalid ?? null,
    };
  }

  const workspaceRoots = [...options.workspaceRoots].sort();
  const payloadRoots = [...(payload.workspace?.roots ?? [])].sort();
  if (JSON.stringify(workspaceRoots) !== JSON.stringify(payloadRoots)) {
    return {
      ok: false,
      reason: "Ticket workspace binding does not match.",
      steerOnInvalid: payload?.cap?.steerOnInvalid ?? null,
    };
  }

  const expiresAt = new Date(payload.cap?.constraints?.expiresAt ?? 0);
  if (
    !Number.isFinite(expiresAt.getTime()) ||
    expiresAt.getTime() < Date.now()
  ) {
    return {
      ok: false,
      reason: "Ticket is expired.",
      steerOnInvalid: payload?.cap?.steerOnInvalid ?? null,
    };
  }

  return { ok: true, payload };
}
