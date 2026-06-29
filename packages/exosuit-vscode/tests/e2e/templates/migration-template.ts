import { test } from "../fixtures";

// 1. CONFIGURATION
const MOCK_FILE = "docs/agent-context/plan.toml"; // File to seed the Holodeck

test.describe("Migration: [Feature Name]", () => {
  // 2. SETUP (The Holodeck)
  test.beforeEach(async ({ exo }) => {
    await exo.holodeck
      .withFile(
        MOCK_FILE,
        `
# Mock Data
[test]
status = "in-progress"
`,
      )
      .apply();
  });

  // 3. THE TEST
  test("Standard Load & Verify", async ({ exo: _exo }) => {
    // A. Trigger the View
    // Use the high-level API to open the webview
    // Example for a generic view:
    // const page = await exo.openWebview(OPEN_COMMAND, VIEW_TITLE);
    // B. Verify Content
    // The page object provides helper methods
    // await page.isVisible();
    // You can also access the underlying locator
    // await expect(page.root.locator("#app")).toContainText("in-progress");
  });
});
