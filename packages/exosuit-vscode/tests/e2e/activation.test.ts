import { test } from "./fixtures";
import { expect } from "@playwright/test";
import { testLogger } from "./test-logger";

test("Extension activates and window loads", async ({ exo }) => {
  const { page, workbench } = exo;

  // Wait for VS Code to be ready
  // The workbench is the main container
  await page.waitForSelector(".monaco-workbench", { timeout: 20000 });

  // Check title to ensure we are in VS Code
  const title = await page.title();
  // Title usually contains "Visual Studio Code" or the workspace name
  testLogger.debug(`Window Title: ${title}`);
  expect(title).toContain("Visual Studio Code");

  // Open Command Palette
  await workbench.openCommandPalette();

  // Type a command to check if extension is registered (optional, might need activation)
  await workbench.typeCommand("Exosuit: Show Context");

  // Wait a bit to see if it appears (this confirms the extension contributed the command)
  // Note: This doesn't confirm activation until we execute it, but it confirms registration.
  await page.waitForSelector(
    ".quick-input-list-entry .monaco-highlighted-label",
  );
});
