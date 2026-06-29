import { z } from "zod";

/**
 * Category of an intent.
 *
 * Based on RFC 0050: Async Intent Channel.
 */
export const IntentCategorySchema = z.enum([
  "correction", // Correcting a mistake or misconception
  "guidance", // Providing direction or preferences
  "question", // Asking a question that needs an answer
  "priority", // Changing what should be worked on
]);

export type IntentCategory = z.infer<typeof IntentCategorySchema>;

/**
 * Scope for contextual surfacing of an intent.
 *
 * Simple scopes are string literals, complex scopes (phase, file) are objects.
 * Matches Rust serde serialization of IntentScope enum.
 */
export const IntentScopeSchema = z.union([
  z.literal("global"),
  z.literal("rust"),
  z.literal("typescript"),
  z.object({ phase: z.string() }), // Phase-specific scope
  z.object({ file: z.string() }), // File-specific scope
]);

export type IntentScope = z.infer<typeof IntentScopeSchema>;

/**
 * Urgency level for when to surface an intent.
 */
export const IntentUrgencySchema = z.enum([
  "immediate", // Surface immediately in all steering output
  "next-touch", // Surface on next phase/task transition
  "when-relevant", // Surface only when scope matches current work
]);

export type IntentUrgency = z.infer<typeof IntentUrgencySchema>;

/**
 * Status of an inbox item in its lifecycle.
 */
export const InboxItemStatusSchema = z.enum([
  "pending", // Awaiting agent attention
  "acknowledged", // Agent has seen but not yet acted on
  "resolved", // Agent has addressed the intent
  "archived", // Moved to archive (no longer surfaced)
]);

export type InboxItemStatus = z.infer<typeof InboxItemStatusSchema>;

/**
 * A single intent item in the inbox.
 *
 * Represents a user-initiated message awaiting processing by the agent.
 */
export const InboxItemSchema = z.object({
  /** Unique identifier for this intent. */
  id: z.string(),

  /** When this intent was created (ISO 8601 datetime). */
  created: z.string(),

  /** Current status in the resolution lifecycle. */
  status: InboxItemStatusSchema.default("pending"),

  /** Category of intent (what kind of message is this?). */
  category: IntentCategorySchema.default("guidance"),

  /** Brief summary of the intent (like email subject). */
  subject: z.string(),

  /** Full content of the intent. */
  body: z.string().default(""),

  /** Scope for contextual surfacing. */
  scope: IntentScopeSchema.default("global"),

  /** When should this intent be surfaced? */
  urgency: IntentUrgencySchema.default("next-touch"),

  /** When this intent was last updated (ISO 8601 datetime). */
  updated: z.string().optional(),

  /** Resolution note (when status = resolved). */
  resolution: z.string().optional(),
});

export type InboxItem = z.infer<typeof InboxItemSchema>;

/**
 * The inbox file containing all intent items.
 *
 * Note: In TOML, items are stored as `[[intent]]` arrays.
 */
export const InboxFileSchema = z.object({
  /** All inbox items (pending, acknowledged, resolved, archived). */
  intent: z.array(InboxItemSchema).default([]),
});

export type InboxFile = z.infer<typeof InboxFileSchema>;

/**
 * Check if an inbox item is active (pending or acknowledged).
 */
export function isActiveItem(item: InboxItem): boolean {
  return item.status === "pending" || item.status === "acknowledged";
}

/**
 * Check if an inbox item is pending (needs attention).
 */
export function isPendingItem(item: InboxItem): boolean {
  return item.status === "pending";
}
