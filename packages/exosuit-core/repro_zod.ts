import { z } from "zod";
import * as toml from "smol-toml";

const TaskStatusSchema = z.enum([
  "pending",
  "in-progress",
  "completed",
  "skipped",
  "deferred",
]);

const TaskSchema = z.object({
  id: z.string(),
  label: z.string(),
  status: TaskStatusSchema,
});

const PhaseStatusSchema = z.enum(["pending", "active", "completed"]);

const PhaseSchema = z.object({
  id: z.string(),
  title: z.string(),
  status: PhaseStatusSchema,
  tasks: z.array(TaskSchema),
});

const EpochStatusSchema = z.enum(["pending", "active", "completed"]);

const EpochSchema = z.object({
  id: z.string(),
  title: z.string(),
  goal: z.string().optional(),
  status: EpochStatusSchema,
  phases: z.array(PhaseSchema),
});

const PlanSchema = z.object({
  epochs: z.array(EpochSchema),
});

const tomlString = `[[epochs]]
id = "epoch-1-genesis"
title = "Epoch 1: Genesis"
status = "active"

[[epochs.phases]]
id = "phase-1-setup"
title = "Phase 1: Setup"
status = "active"
tasks = []
`;

const isDebug = process.env.EXOSUIT_DEBUG === "true";
const writeOut = (message: string) => {
  if (isDebug) {
    process.stdout.write(`${message}\n`);
  }
};
const writeErr = (message: string) => {
  if (isDebug) {
    process.stderr.write(`${message}\n`);
  }
};

try {
  const data = toml.parse(tomlString);
  writeOut(`Parsed TOML: ${JSON.stringify(data, null, 2)}`);
  PlanSchema.parse(data);
  writeOut("Zod Validation Success!");
} catch (e) {
  writeErr(`Zod Validation Failed: ${String(e)}`);
}
