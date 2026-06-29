import { createHmac, timingSafeEqual } from "node:crypto";

export const TICKET_PREFIX = "exo1";

export function base64UrlEncode(buf: ArrayBufferView): string {
  const b = Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength);
  return b
    .toString("base64")
    .replace(/=/g, "")
    .replace(/\+/g, "-")
    .replace(/\//g, "_");
}

export function base64UrlDecode(text: string): Uint8Array {
  const padded = text.replace(/-/g, "+").replace(/_/g, "/");
  const padLength = (4 - (padded.length % 4)) % 4;
  const normalized = padded + "=".repeat(padLength);
  return new Uint8Array(Buffer.from(normalized, "base64"));
}

export function encodeJsonToBase64Url(value: unknown): string {
  return base64UrlEncode(Buffer.from(JSON.stringify(value), "utf8"));
}

export function decodeBase64UrlToJson<T>(b64Url: string): T {
  const bytes = base64UrlDecode(b64Url);
  const text = Buffer.from(bytes).toString("utf8");
  return JSON.parse(text) as T;
}

export function signPayloadB64(
  key: Uint8Array<ArrayBufferLike>,
  payloadB64Url: string
): Uint8Array<ArrayBufferLike> {
  const sig = createHmac("sha256", key).update(payloadB64Url).digest();
  return new Uint8Array(sig);
}

export function verifySignature(options: {
  key: Uint8Array<ArrayBufferLike>;
  payloadB64Url: string;
  sigBytes: Uint8Array;
}): boolean {
  const expected = signPayloadB64(options.key, options.payloadB64Url);
  if (options.sigBytes.length !== expected.length) {
    return false;
  }
  return timingSafeEqual(options.sigBytes, expected);
}

export function parseTicket(
  ticket: string
):
  | { ok: true; payloadB64Url: string; sigB64Url: string }
  | { ok: false; reason: string } {
  const parts = ticket.split(".");
  if (parts.length !== 3 || parts[0] !== TICKET_PREFIX) {
    return { ok: false, reason: "Ticket has invalid format." };
  }

  return { ok: true, payloadB64Url: parts[1], sigB64Url: parts[2] };
}
