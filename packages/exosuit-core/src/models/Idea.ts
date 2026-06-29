import { z } from "zod";

export const IdeaStatusSchema = z.enum([
  "new",
  "triaged",
  "accepted",
  "rejected",
  "deferred",
  "implemented",
  "archived",
]);

export type IdeaStatus = z.infer<typeof IdeaStatusSchema>;

export const IdeaSchema = z.object({
  id: z.string(),
  title: z.string(),
  description: z.string(),
  status: IdeaStatusSchema.default("new"),
  created_at: z.string().datetime(),
  source: z.enum(["user", "agent", "chat-context"]).default("user"),
  tags: z.array(z.string()).default([]),
  related_tasks: z.array(z.string()).default([]),
});

export type Idea = z.infer<typeof IdeaSchema>;

export const IdeasFileSchema = z.object({
  ideas: z.array(IdeaSchema).default([]),
});

export type IdeasFile = z.infer<typeof IdeasFileSchema>;
