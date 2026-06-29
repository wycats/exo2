import { z } from "zod";

// ULID validation regex (Crockford Base32, 26 characters)
// Excludes I, L, O, U as per Crockford Base32 spec
const ULID_REGEX = /^[0-9A-HJKMNP-TV-Z]{26}$/;

/**
 * Schema for validating ULID strings.
 * ULIDs are 26-character Crockford Base32 encoded strings.
 */
export const UlidSchema = z.string().regex(ULID_REGEX, "Invalid ULID format");
export type Ulid = z.infer<typeof UlidSchema>;

/**
 * Schema for canonical references (e.g., "phase@01HZVY3X4M5N6P7Q8R9S0TABC1").
 */
export const CanonicalRefSchema = z
  .string()
  .regex(
    /^[a-z]+@[0-9A-HJKMNP-TV-Z]{26}$/,
    "Invalid canonical reference format (expected type@ULID)",
  );
export type CanonicalRef = z.infer<typeof CanonicalRefSchema>;

/**
 * Status values for goals in SQLite plan state.
 * Goals are planning-level objectives within a phase.
 */
export const GoalStatusSchema = z.enum([
  "pending",
  "in-progress",
  "completed",
  "skipped",
  "deferred",
  "abandoned",
  // Strike goal statuses (RFC 00229)
  "red",
  "green",
]);
export type GoalStatus = z.infer<typeof GoalStatusSchema>;

/**
 * Schema for goals in SQLite plan state.
 * Goals are planning-level objectives; execution-level work items are nested
 * under goals in the derived phase details state.
 */
export const GoalSchema = z.object({
  id: z.string(),
  label: z.string(),
  status: GoalStatusSchema,
  // ULID fields for canonical identification
  ulid: UlidSchema.optional(),
  slug: z.string().optional(),
  aliases: z.array(z.string()).default([]),
});

export type Goal = z.infer<typeof GoalSchema>;

// Backward compatibility aliases
export const TaskStatusSchema = GoalStatusSchema;
export type TaskStatus = GoalStatus;
export const TaskSchema = GoalSchema;
export type Task = Goal;

export const PhaseRfcRelationSchema = z.enum(["driving", "related", "blocked"]);
export type PhaseRfcRelation = z.infer<typeof PhaseRfcRelationSchema>;

export const PhaseRfcSchema = z.union([
  z.string(),
  z.object({
    id: z.string(),
    target: z.number().int().optional().nullable(),
    relation: PhaseRfcRelationSchema.optional(),
  }),
]);
export type PhaseRfc = z.infer<typeof PhaseRfcSchema>;

export const PhaseStatusSchema = z.enum([
  "pending",
  "in-progress",
  "completed",
  "deferred",
  "abandoned",
]);
export type PhaseStatus = z.infer<typeof PhaseStatusSchema>;

export const PhaseSchema = z
  .object({
    id: z.string(),
    title: z.string(),
    status: PhaseStatusSchema,
    // Goals from SQLite plan state
    goals: z.array(GoalSchema).default([]),
    // Legacy field name (backward compatibility)
    tasks: z.array(GoalSchema).optional(),
    rfcs: z.array(PhaseRfcSchema).optional(),
    // ULID fields for canonical identification
    ulid: UlidSchema.optional(),
    slug: z.string().optional(),
    aliases: z.array(z.string()).default([]),
  })
  .transform((phase) => {
    // Merge legacy 'tasks' into 'goals' for backward compatibility
    const goals = phase.goals.length > 0 ? phase.goals : (phase.tasks ?? []);
    // Return without 'tasks' field
    const { tasks: _tasks, ...rest } = phase;
    return { ...rest, goals };
  });

export type Phase = z.infer<typeof PhaseSchema>;

export const EpochStatusSchema = z.enum([
  "pending",
  "in-progress",
  "completed",
  "deferred",
  "abandoned",
]);
export type EpochStatus = z.infer<typeof EpochStatusSchema>;

export const EpochSchema = z.object({
  id: z.string(),
  title: z.string(),
  goal: z.string().optional(),
  status: EpochStatusSchema,
  phases: z.array(PhaseSchema),
  // ULID fields for canonical identification
  ulid: UlidSchema.optional(),
  slug: z.string().optional(),
  aliases: z.array(z.string()).default([]),
});

export type Epoch = z.infer<typeof EpochSchema>;

export const PlanSchema = z.object({
  epochs: z.array(EpochSchema),
});

export type Plan = z.infer<typeof PlanSchema>;

/**
 * Find the active epoch from a list of epochs.
 *
 * The `status` field in SQLite plan state is always the derived status
 * (written by Rust's `Epoch::derived_status()`), so this is a
 * simple lookup — no algorithm duplication needed.
 */
export function findActiveEpoch(epochs: Epoch[]): Epoch | undefined {
  return epochs.find((e) => e.status === "in-progress");
}
