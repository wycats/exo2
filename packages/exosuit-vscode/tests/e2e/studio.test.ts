import { test } from "./fixtures";
import { expect } from "@playwright/test";
import { StudioPage } from "./lib/exosuit-test";

// 1. CONFIGURATION
const VIEW_TITLE = "Exosuit Studio";
const MOCK_FILE = "docs/rfcs/stage-0/9997-studio-test-rfc.md";

test.describe("Studio Integration", () => {
  test.beforeEach(async ({ exo }) => {
    await exo.holodeck
      .withPhase("test-phase", "Test Phase")
      .withImplementationPlan("test-phase", "Test Phase")
      .withFile("docs/agent-context/feedback.toml", "threads = []")
      .withFile(
        MOCK_FILE,
        `---
title: Studio Test RFC
id: 9997
feature: Studio Test
---

## Summary

This is a test RFC for Studio integration.`,
      )
      .apply();
  });

  test("Open in Studio: Works for RFC files", async ({ exo }) => {
    // A. Trigger the View
    // Use the explorer to open the file reliably (Quick Open can be flaky with new files)
    await exo.workbench.openExplorerAndClickFile(MOCK_FILE);

    // Open in Studio
    const frame = await exo.workbench.openInStudio(VIEW_TITLE);
    const studioPage = new StudioPage(frame);

    // C. Verify Content
    // Use the page object to verify content
    await studioPage.expectContent("Studio Test RFC");
  });

  test("Open in Studio: Works for axioms.system.toml (namespaced axioms)", async ({
    exo,
  }) => {
    // This tests that axioms.*.toml files are properly recognized
    const axiomsFile = "docs/agent-context/axioms.system.toml";
    await exo.holodeck
      .withFile(
        axiomsFile,
        `[[axioms]]
id = "test-axiom"
title = "Test Axiom"
description = "A test axiom for Studio integration"
category = "system"
`,
      )
      .apply();

    await exo.workbench.openExplorerAndClickFile(axiomsFile);
    const frame = await exo.workbench.openInStudio(VIEW_TITLE);
    const studioPage = new StudioPage(frame);

    // Verify the axiom content renders
    await studioPage.expectContent("Test Axiom");
  });

  test("Implementation plan: renders Files as collapsible chips", async ({
    exo,
  }) => {
    const planPath = "docs/agent-context/current/implementation-plan.toml";
    const referenced =
      "docs/rfcs/stage-0/10020-exosuit-capability-tree-machine-channel.md";

    await exo.holodeck
      .withFile(referenced, "# Stub\n\nThis file exists for open-file tests.\n")
      .withFile(
        planPath,
        `
[phase]
title = "Phase 67: Machine Channel Projection + Canonical Execution Plan"
id = "phase-67-machine-channel"

[goals]
primary = "Test plan rendering"

[plan]
[[plan.goals]]
name = "Draft RFC 0131 + update RFC 0125"
details = """Create an immediate working draft for RFC 0131 (canonical implementation-plan execution artifact) and update RFC 0125 to reference it."""
type = "docs"
files = [
  "docs/rfcs/stage-0/10021-implementation-plan-as-canonical-execution-artifact.md",
  "docs/rfcs/stage-0/10020-exosuit-capability-tree-machine-channel.md",
]
`,
      )
      .apply();

    await exo.workbench.openExplorerAndClickFile(planPath);
    const frame = await exo.workbench.openInStudio(VIEW_TITLE);

    // Files list should be a compact collapsible with chips, not a markdown bullet list.
    const collapsible = frame.locator("details.readonly-collapsible");
    await expect(collapsible).toBeVisible({ timeout: 10000 });
    await expect(collapsible).toContainText("Files");
    await expect(frame.locator("button.file-chip")).toHaveCount(2);

    // Neutral type badge should not render a leading dot.
    await expect(frame.locator(".status-badge.neutral .dot")).toHaveCount(0);

    // Clicking a file chip should open the file in the editor.
    await frame.locator(`button.file-chip[data-path="${referenced}"]`).click();
    const basename = referenced.split("/").pop()!;
    await expect(
      exo.page.locator(".tab-label", { hasText: basename }),
    ).toBeVisible({
      timeout: 10000,
    });
  });
});
