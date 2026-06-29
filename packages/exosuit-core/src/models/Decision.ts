import { z } from "zod";

export const DecisionSchema = z.object({
  id: z.string(),
  date: z.string(), // YYYY-MM-DD
  title: z.string(),
  context: z.string(),
  decision: z.string(),
  consequences: z.object({
    pros: z.array(z.string()),
    cons: z.array(z.string()),
  }),
  status: z
    .enum(["proposed", "accepted", "deprecated", "rejected"])
    .default("accepted"),
});

export type Decision = z.infer<typeof DecisionSchema>;

export const DecisionFileSchema = z.object({
  decisions: z.array(DecisionSchema),
});

export type DecisionFile = z.infer<typeof DecisionFileSchema>;
