import { describe, expect, it } from "vitest";

import {
  CanonicalSeedBuilder,
  type CanonicalSeedCommandResult,
  type CanonicalSeedCommandRunner,
} from "../../tests/e2e/canonical-seed";

class RecordingRunner implements CanonicalSeedCommandRunner {
  readonly commands: string[][] = [];
  #nextEpoch = 1;
  #nextPhase = 1;

  async run(args: string[]): Promise<CanonicalSeedCommandResult> {
    this.commands.push(args);

    if (args[0] === "init") {
      return ok({ kind: "init" });
    }

    if (args[0] === "epoch" && args[1] === "add") {
      return ok({ id: `epoch-${this.#nextEpoch++}` });
    }

    if (args[0] === "phase" && args[1] === "add") {
      return ok({ id: `phase-${this.#nextPhase++}` });
    }

    return ok({ kind: args.slice(0, 2).join(".") });
  }
}

describe("CanonicalSeedBuilder", () => {
  it("runs canonical commands and returns stable test-key mappings", async () => {
    const runner = new RecordingRunner();
    const gitRunner = new RecordingRunner();
    const result = await new CanonicalSeedBuilder({
      workspaceRoot: "/tmp/canonical-seed-builder-test",
      runner,
      gitRunner,
    })
      .epoch({ key: "main", title: "Main Epoch", status: "in-progress" })
      .phase({
        key: "active",
        epoch: "main",
        title: "Active Phase",
        status: "in-progress",
      })
      .goal({
        key: "goal",
        id: "seed-goal",
        phase: "active",
        label: "Seeded Goal",
      })
      .task({
        key: "task",
        id: "seed-task",
        goal: "goal",
        label: "Seeded Task",
        status: "in-progress",
      })
      .apply();

    expect(result.epochs.main).toEqual({ id: "epoch-1", title: "Main Epoch" });
    expect(result.phases.active).toEqual({
      id: "phase-1",
      title: "Active Phase",
      epochId: "epoch-1",
    });
    expect(result.goals.goal).toEqual({
      id: "seed-goal",
      label: "Seeded Goal",
      phaseId: "phase-1",
    });
    expect(result.tasks.task).toEqual({
      id: "seed-task",
      label: "Seeded Task",
      goalId: "seed-goal",
    });
    expect(gitRunner.commands).toContainEqual(["init"]);
    expect(runner.commands).toContainEqual(["init", "--defaults"]);
    expect(runner.commands).toContainEqual([
      "epoch",
      "add",
      "--title",
      "Main Epoch",
      "--format",
      "json",
    ]);
    expect(runner.commands).toContainEqual([
      "phase",
      "add",
      "--title",
      "Active Phase",
      "--epoch",
      "epoch-1",
      "--kind",
      "regular",
      "--format",
      "json",
    ]);
    expect(runner.commands).toContainEqual([
      "goal",
      "add",
      "Seeded Goal",
      "--id",
      "seed-goal",
      "--phase",
      "phase-1",
      "--format",
      "json",
    ]);
    expect(runner.commands).toContainEqual([
      "task",
      "add",
      "Seeded Task",
      "--id",
      "seed-task",
      "--goal",
      "seed-goal",
      "--format",
      "json",
    ]);
    expect(runner.commands).toContainEqual([
      "task",
      "start",
      "seed-task",
      "--format",
      "json",
    ]);
  });
});

function ok(result: unknown): CanonicalSeedCommandResult {
  return { status: "ok", result };
}
