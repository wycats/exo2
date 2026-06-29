import { test } from "./fixtures";
import { expect } from "@playwright/test";

// 1. CONFIGURATION
const MOCK_FILE = "docs/rfcs/stage-0/9999-visual-test-rfc.md";

test.describe("RFC View Visuals", () => {
  test.beforeEach(async ({ exo }) => {
    const filler = Array(50)
      .fill("Line of text to force scrolling.")
      .join("\n\n");

    await exo.holodeck
      .withFile(
        MOCK_FILE,
        `---
title: Visual Test RFC
status: Stage 0 (Draft)
authors: GitHub Copilot
epoch: 18 (Usability Slices)
---

# RFC 0156: Visual Test RFC

## Summary

This is a test RFC for visual verification.

## Motivation

We need to verify sticky headers and spacing.

${filler}

## Proposal

Here is some code to test Shiki:

\`\`\`typescript
function hello() {
  console.log("Hello World");
}
\`\`\`

${filler}

## Drawbacks

None.

${filler}
`
      )
      .apply();
  });

  test("Verify RFC View Layout Details", async ({ exo }) => {
    const { page } = exo;
    // A. Trigger the View
    const studio = await exo.openInStudio("9999-visual-test-rfc.md");

    // C. Verify Content
    // 1. Verify "GitHub Copilot" text (Author)
    await expect(studio.author).toBeVisible();

    // 2. Verify Sticky TOC
    await expect(studio.toc).toBeVisible();
    await expect(studio.toc).toHaveCSS("position", "sticky");
    await expect(studio.toc).toHaveCSS("top", "0px");
    await expect(studio.toc).toHaveCSS("z-index", "100");

    // Verify TOC styling (no shadow, background color)
    await expect(studio.toc).toHaveCSS("box-shadow", "none");

    // 3. Verify Spacing (Introduction/Summary)
    // We reduced .rfc-content gap to 1.5rem (24px)
    await expect(studio.rfcContent).toHaveCSS("gap", "24px");

    // 4. Verify Shiki Highlighting
    await expect(studio.shiki).toBeVisible();

    // 5. Verify TOC Active State
    // Scroll to "Proposal"
    await studio.scrollTo("#proposal");

    await page.waitForTimeout(1000); // Wait for observer

    // The "Proposal" TOC item should be active
    await expect(studio.activeTocItem).toHaveText("Proposal");
    // Check border bottom
    await expect(studio.activeTocItem).toHaveCSS("border-bottom-width", "2px");
  });
});
