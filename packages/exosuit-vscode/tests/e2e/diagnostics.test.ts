import { test } from "./fixtures";
import { expect } from "@playwright/test";
import { testLogger } from "./test-logger";

test.describe("Diagnostics", () => {
  test("List installed extensions", async ({ exo }) => {
    const { page, workbench } = exo;
    // Open the extensions view
    await workbench.executeCommand("workbench.view.extensions");

    // Wait for the extensions view to be visible (Activity Bar icon is active)
    const extensionsView = page.locator(".extensions-viewlet");
    await expect(extensionsView).toBeVisible({ timeout: 5000 });

    // We can't easily scrape the extension list from the DOM because it's virtualized and complex.
    // Instead, let's look at the DOM for known extensions.

    // GitLens usually adds a view container.
    const gitlensIcon = page.locator(
      ".activitybar .action-label[aria-label*='GitLens']",
    );
    if ((await gitlensIcon.count()) > 0) {
      testLogger.warn("GitLens icon found in Activity Bar");
    } else {
      testLogger.debug("GitLens icon NOT found in Activity Bar");
    }

    // Check for Exosuit (expecting at least one, e.g. Run or Plan)
    const exosuitIcon = page
      .locator(".activitybar .action-label[aria-label*='Exosuit']")
      .first();
    await expect(exosuitIcon).toBeVisible();
    testLogger.debug("Exosuit icon found");
  });
});
