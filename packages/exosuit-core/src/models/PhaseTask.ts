import { z } from "zod";

export const PhaseTaskStatusSchema = z.enum([
  "todo",
  "in-progress",
  "done",
  "abandoned",
]);
export type PhaseTaskStatus = z.infer<typeof PhaseTaskStatusSchema>;

export const PhaseTaskSchema = z.object({
  id: z.string(),
  title: z.string(),
  description: z.string().optional(),
  kind: z.literal("strike").optional(),
  status: PhaseTaskStatusSchema,
  // Per RFC 00177 Data Location Axiom: completion_log lives in SQLite state (permanent)
  completionLog: z.string().optional(),
});

export type PhaseTask = z.infer<typeof PhaseTaskSchema>;

export const PhaseTaskListSchema = z.object({
  tasks: z.array(PhaseTaskSchema),
});

export type PhaseTaskList = z.infer<typeof PhaseTaskListSchema>;
