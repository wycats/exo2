import { describe, expect, it } from "vitest";

import { formatInboxAttentionTooltip } from "./InboxStatusBarService";

describe("InboxStatusBarService", () => {
  it("describes inbox attention without intent jargon", () => {
    expect(formatInboxAttentionTooltip(1)).toBe(
      "1 inbox item needing attention - click to review",
    );
    expect(formatInboxAttentionTooltip(2)).toBe(
      "2 inbox items needing attention - click to review",
    );
  });
});
