export const VERSION = "0.0.1";
export * from "./parser.ts";
export * from "./serializer.ts";
export * from "./types.ts";
export * from "./interpolation.ts";
export * from "./projection.ts";
export {
  AxiomSchema,
  AxiomFileSchema,
  type Axiom as AxiomStruct,
  type AxiomFile,
} from "./models/Axiom.ts";
export * from "./models/Decision.ts";
export {
  PlanSchema,
  EpochSchema,
  PhaseSchema,
  PhaseRfcSchema,
  PhaseStatusSchema,
  GoalSchema,
  GoalStatusSchema,
  // Backward compatibility exports
  TaskSchema,
  TaskStatusSchema,
  findActiveEpoch,
  type Plan as PlanStruct,
  type Epoch,
  type Phase,
  type PhaseRfc,
  type PhaseStatus,
  type Goal,
  type GoalStatus,
  // Backward compatibility type aliases
  type Task as TaskStruct,
  type TaskStatus,
} from "./models/Plan.ts";
export * from "./models/ToolPresentation.ts";
export * from "./models/PhaseTask.ts";
export * from "./models/Feedback.ts";
export * from "./models/Inbox.ts";
export * from "./models/Idea.ts";
export * from "./models/PromptSpec.ts";
export * from "./state/ProjectState.ts";
export * from "./client/ExosuitClient.ts";
export * from "./PromptService.ts";
export * from "./Logger.ts";
