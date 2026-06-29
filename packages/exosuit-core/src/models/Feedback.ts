import { z } from "zod";

export const FeedbackMessageSchema = z.object({
  id: z.string(),
  author: z.enum(["user", "agent"]),
  content: z.string(),
  created_at: z.string().datetime(),
});

export type FeedbackMessage = z.infer<typeof FeedbackMessageSchema>;

export const FeedbackThreadSchema = z.object({
  id: z.string(),
  target_file: z.string(),
  target_id: z.string().optional(),
  target_field: z.string().optional(),
  status: z.enum(["open", "proposed-resolved", "resolved", "archived"]),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  messages: z.array(FeedbackMessageSchema),
});

export type FeedbackThread = z.infer<typeof FeedbackThreadSchema>;

export const FeedbackFileSchema = z.object({
  threads: z.array(FeedbackThreadSchema),
});

export type FeedbackFile = z.infer<typeof FeedbackFileSchema>;
