import { z } from "zod";
import { AxiomSchema } from "../models/Axiom.ts";
import { DecisionSchema } from "../models/Decision.ts";
import { PlanSchema } from "../models/Plan.ts";
import { FeedbackThreadSchema } from "../models/Feedback.ts";

export const CurrentPhaseStateSchema = z.object({
  epochId: z.string(),
  phaseId: z.string(),
  title: z.string(),
});

export type CurrentPhaseState = z.infer<typeof CurrentPhaseStateSchema>;

export const ProjectStateSchema = z.object({
  axioms: z.array(AxiomSchema),
  decisions: z.array(DecisionSchema),
  plan: PlanSchema,
  currentPhase: CurrentPhaseStateSchema.optional(),
  feedback: z.array(FeedbackThreadSchema).default([]),
});

export type ProjectState = z.infer<typeof ProjectStateSchema>;
