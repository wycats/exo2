import { describe, expect, it } from "vitest";

import {
  exohookValidateArgs,
  includesCheckInValidation,
} from "./ExohookTestController";

describe("ExohookTestController validation arguments", () => {
  it("keeps manual lane runs complete", () => {
    expect(exohookValidateArgs("coherence")).toEqual([
      "validate",
      "coherence",
      "--format=jsonl",
    ]);
  });

  it("limits continuous lane runs to observe checks", () => {
    expect(exohookValidateArgs("coherence", "continuous")).toEqual([
      "validate",
      "coherence",
      "--format=jsonl",
      "--category",
      "observe",
    ]);
  });

  it("leaves mutate results untouched during continuous finalization", () => {
    expect(includesCheckInValidation("continuous", "observe")).toBe(true);
    expect(includesCheckInValidation("continuous", undefined)).toBe(true);
    expect(includesCheckInValidation("continuous", "mutate")).toBe(false);
    expect(includesCheckInValidation("manual", "mutate")).toBe(true);
  });
});
