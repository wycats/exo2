import { expect } from "@playwright/test";
import { test } from "./fixtures";

test.describe("Extension Debugging", () => {
  test("listExtensions command is registered", async ({ workbench }) => {
    // Execute the command via the workbench
    // Since we can't easily get the return value of a command executed via UI/keybinding in Playwright without a side channel,
    // we might need to rely on the fact that it doesn't crash, or use a side channel if available.
    // However, for this specific command, it's designed to return data to the caller (e.g. another extension or a test runner).
    // In the E2E context, we are running OUTSIDE VS Code.

    // Wait, the E2E tests run VS Code. We can use the `executeCommand` from the test runner if we had a way to bridge it.
    // But our E2E setup is black-box.

    // Let's just verify it's registered by trying to run it from the command palette.
    await workbench.openCommandPalette();
    await workbench.typeCommand("Exosuit: Debug List Extensions");

    // If the command exists, it should appear.
    // We can check if the command palette shows it.
    const quickPickItem = workbench.locator(
      ".quick-input-list-entry .label-name",
      { hasText: "Exosuit: Debug List Extensions" },
    );
    await expect(quickPickItem).toBeVisible();
  });
});
