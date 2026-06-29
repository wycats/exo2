import { z } from "zod";

export const ToolPresentationSchema = z.object({
  name: z.string(),
  alias: z.string().optional(),
  presentation: z.string(),
});

export type ToolPresentation = z.infer<typeof ToolPresentationSchema>;

export const ToolPresentationFileSchema = z.object({
  tools: z.array(ToolPresentationSchema),
});
