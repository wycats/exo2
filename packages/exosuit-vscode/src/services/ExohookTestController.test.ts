import { describe, expect, it } from "vitest";

import { exohookValidateArgs } from "./ExohookTestController";

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
});
