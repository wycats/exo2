import { expect } from "vitest";
import {
  UlidSchema,
  CanonicalRefSchema,
  TaskSchema,
  PhaseSchema,
  EpochSchema,
  PlanSchema,
} from "../src/models/Plan.js";

describe("UlidSchema", () => {
  it("should accept valid ULIDs", () => {
    const validUlid = "01HZVY3X4M5N6P7Q8R9S0TABC1";
    expect(UlidSchema.safeParse(validUlid).success).to.be.true;
  });

  it("should reject ULIDs with invalid characters (I, L, O, U)", () => {
    // I is invalid
    expect(UlidSchema.safeParse("01HZVY3X4M5N6P7Q8R9S0TABI1").success).to.be
      .false;
    // L is invalid
    expect(UlidSchema.safeParse("01HZVY3X4M5N6P7Q8R9S0TABL1").success).to.be
      .false;
    // O is invalid
    expect(UlidSchema.safeParse("01HZVY3X4M5N6P7Q8R9S0TABO1").success).to.be
      .false;
    // U is invalid
    expect(UlidSchema.safeParse("01HZVY3X4M5N6P7Q8R9S0TABU1").success).to.be
      .false;
  });

  it("should reject ULIDs with wrong length", () => {
    // 25 chars
    expect(UlidSchema.safeParse("01HZVY3X4M5N6P7Q8R9S0TAB").success).to.be
      .false;
    // 27 chars
    expect(UlidSchema.safeParse("01HZVY3X4M5N6P7Q8R9S0TABC12").success).to.be
      .false;
  });

  it("should reject empty strings", () => {
    expect(UlidSchema.safeParse("").success).to.be.false;
  });
});

describe("CanonicalRefSchema", () => {
  it("should accept valid canonical references", () => {
    expect(
      CanonicalRefSchema.safeParse("phase@01HZVY3X4M5N6P7Q8R9S0TABC1").success,
    ).to.be.true;
    expect(
      CanonicalRefSchema.safeParse("task@01HZVY3X4M5N6P7Q8R9S0TABC1").success,
    ).to.be.true;
    expect(
      CanonicalRefSchema.safeParse("epoch@01HZVY3X4M5N6P7Q8R9S0TABC1").success,
    ).to.be.true;
  });

  it("should reject references without type", () => {
    expect(CanonicalRefSchema.safeParse("@01HZVY3X4M5N6P7Q8R9S0TABC1").success)
      .to.be.false;
  });

  it("should reject references without ULID", () => {
    expect(CanonicalRefSchema.safeParse("phase@").success).to.be.false;
    expect(CanonicalRefSchema.safeParse("phase@invalid").success).to.be.false;
  });

  it("should reject uppercase type names", () => {
    expect(
      CanonicalRefSchema.safeParse("PHASE@01HZVY3X4M5N6P7Q8R9S0TABC1").success,
    ).to.be.false;
  });
});

describe("TaskSchema with ULID fields", () => {
  it("should accept task without ULID fields (backward compatible)", () => {
    const task = {
      id: "my-task",
      label: "My Task",
      status: "pending",
    };
    const result = TaskSchema.safeParse(task);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.ulid).to.be.undefined;
      expect(result.data.slug).to.be.undefined;
      expect(result.data.aliases).to.deep.equal([]);
    }
  });

  it("should accept task with ULID fields", () => {
    const task = {
      id: "my-task",
      label: "My Task",
      status: "pending",
      ulid: "01HZVY3X4M5N6P7Q8R9S0TABC1",
      slug: "my-task",
      aliases: ["old-task-name", "legacy-id"],
    };
    const result = TaskSchema.safeParse(task);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.ulid).to.equal("01HZVY3X4M5N6P7Q8R9S0TABC1");
      expect(result.data.slug).to.equal("my-task");
      expect(result.data.aliases).to.deep.equal(["old-task-name", "legacy-id"]);
    }
  });

  it("should reject task with invalid ULID", () => {
    const task = {
      id: "my-task",
      label: "My Task",
      status: "pending",
      ulid: "invalid-ulid",
    };
    expect(TaskSchema.safeParse(task).success).to.be.false;
  });
});

describe("PhaseSchema with ULID fields", () => {
  it("should accept phase without ULID fields (backward compatible)", () => {
    const phase = {
      id: "phase-1",
      title: "Phase 1",
      status: "in-progress",
      tasks: [],
    };
    const result = PhaseSchema.safeParse(phase);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.ulid).to.be.undefined;
      expect(result.data.slug).to.be.undefined;
      expect(result.data.aliases).to.deep.equal([]);
    }
  });

  it("should accept phase with ULID fields and rfcs", () => {
    const phase = {
      id: "phase-1",
      title: "Phase 1",
      status: "in-progress",
      tasks: [],
      rfcs: ["10028", "0057"],
      ulid: "01HZVY3X4M5N6P7Q8R9S0TABC1",
      slug: "phase-1",
      aliases: [],
    };
    const result = PhaseSchema.safeParse(phase);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.rfcs).to.deep.equal(["10028", "0057"]);
      expect(result.data.ulid).to.equal("01HZVY3X4M5N6P7Q8R9S0TABC1");
    }
  });

  it("should accept mixed string and object RFCs", () => {
    const phase = {
      id: "phase-mixed-rfcs",
      title: "Phase Mixed",
      status: "in-progress",
      rfcs: ["001", { id: "002", target: 5 }],
      tasks: [],
    };
    const result = PhaseSchema.safeParse(phase);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.rfcs).to.deep.equal(["001", { id: "002", target: 5 }]);
    }
  });
});

describe("EpochSchema with ULID fields", () => {
  it("should accept epoch without ULID fields (backward compatible)", () => {
    const epoch = {
      id: "epoch-1",
      title: "Epoch 1",
      status: "in-progress",
      phases: [],
    };
    const result = EpochSchema.safeParse(epoch);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.ulid).to.be.undefined;
      expect(result.data.slug).to.be.undefined;
      expect(result.data.aliases).to.deep.equal([]);
    }
  });

  it("should accept epoch with ULID fields", () => {
    const epoch = {
      id: "epoch-1",
      title: "Epoch 1",
      goal: "Complete the project",
      status: "in-progress",
      phases: [],
      ulid: "01HZVY3X4M5N6P7Q8R9S0TABC1",
      slug: "epoch-1",
      aliases: ["old-epoch"],
    };
    const result = EpochSchema.safeParse(epoch);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.goal).to.equal("Complete the project");
      expect(result.data.ulid).to.equal("01HZVY3X4M5N6P7Q8R9S0TABC1");
      expect(result.data.aliases).to.deep.equal(["old-epoch"]);
    }
  });
});

describe("PlanSchema with nested ULID fields", () => {
  it("should accept full plan with ULID fields at all levels", () => {
    const plan = {
      epochs: [
        {
          id: "epoch-1",
          title: "Epoch 1",
          status: "in-progress",
          ulid: "01HZVY3X4M5N6P7Q8R9S0TABC1",
          phases: [
            {
              id: "phase-1",
              title: "Phase 1",
              status: "in-progress",
              ulid: "01HZVY3X4M5N6P7Q8R9S0TABC2",
              goals: [
                {
                  id: "task-1",
                  label: "Task 1",
                  status: "pending",
                  ulid: "01HZVY3X4M5N6P7Q8R9S0TABC3",
                },
              ],
            },
          ],
        },
      ],
    };
    const result = PlanSchema.safeParse(plan);
    expect(result.success).to.be.true;
    if (result.success) {
      expect(result.data.epochs[0].ulid).to.equal("01HZVY3X4M5N6P7Q8R9S0TABC1");
      expect(result.data.epochs[0].phases[0].ulid).to.equal(
        "01HZVY3X4M5N6P7Q8R9S0TABC2",
      );
      expect(result.data.epochs[0].phases[0].goals[0].ulid).to.equal(
        "01HZVY3X4M5N6P7Q8R9S0TABC3",
      );
    }
  });
});
