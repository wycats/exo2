import { Page, Frame, expect } from "@playwright/test";
import { findWebviewFrame } from "./utils";
import { testLogger } from "./test-logger";

export class WorkbenchFixture {
  constructor(protected page: Page) {}

  /**
   * Creates a locator on the main VS Code page.
   */
  locator(selector: string, options?: Parameters<Page["locator"]>[1]) {
    return this.page.locator(selector, options);
  }

  /**
   * Opens the Command Palette and executes a command.
   */
  async executeCommand(command: string, retries = 3) {
    testLogger.debug(`Executing command: ${command}`);
    const isMac = process.platform === "darwin";
    const modifier = isMac ? "Meta" : "Control";

    for (let i = 0; i < retries; i++) {
      try {
        // Prefer Ctrl/Cmd+Shift+P, fall back to F1.
        if (i % 2 === 0) {
          await this.page.keyboard.press(`${modifier}+Shift+P`);
        } else {
          await this.page.keyboard.press("F1");
        }

        await this.page.waitForSelector(".quick-input-widget", {
          timeout: 5000,
        });

        // If we got here, the widget is open
        await this.page.keyboard.type(command);
        await this.page.waitForTimeout(500);
        await this.page.keyboard.press("Enter");
        await this.page.waitForTimeout(500);
        return; // Success
      } catch (e) {
        testLogger.warn(
          `Attempt ${i + 1} to execute command failed: ${
            e instanceof Error ? e.message : String(e)
          }`,
        );
        // Ensure we close any partial state if possible
        await this.page.keyboard.press("Escape");
        await this.page.waitForTimeout(1000);
      }
    }
    throw new Error(
      `Failed to execute command '${command}' after ${retries} attempts`,
    );
  }

  /**
   * Opens the Command Palette without executing a command.
   */
  async openCommandPalette(retries = 3) {
    const isMac = process.platform === "darwin";
    const modifier = isMac ? "Meta" : "Control";

    for (let i = 0; i < retries; i++) {
      try {
        // Prefer Ctrl/Cmd+Shift+P, fall back to F1.
        if (i % 2 === 0) {
          await this.page.keyboard.press(`${modifier}+Shift+P`);
        } else {
          await this.page.keyboard.press("F1");
        }
        await this.page.waitForSelector(".quick-input-widget", {
          timeout: 5000,
        });
        return;
      } catch (e) {
        testLogger.warn(
          `Attempt ${i + 1} to open command palette failed: ${
            e instanceof Error ? e.message : String(e)
          }`,
        );
        if (i < retries - 1) {
          await this.page.waitForTimeout(1000);
        }
      }
    }
    throw new Error(`Failed to open command palette after ${retries} attempts`);
  }

  /**
   * Types text into the Command Palette.
   */
  async typeCommand(text: string) {
    await this.page.keyboard.type(text);
  }

  /**
   * Opens a file using the Quick Open (Ctrl/Cmd+P) menu.
   * Includes retry logic to handle potential timing issues during startup.
   */
  async openFile(filename: string, retries = 5) {
    testLogger.debug(`Opening file: ${filename}`);
    const isMac = process.platform === "darwin";
    const modifier = isMac ? "Meta" : "Control";

    // Wait a bit for app to settle
    await this.page.waitForTimeout(1000);

    let fileOpened = false;
    for (let i = 0; i < retries; i++) {
      try {
        await this.page.keyboard.press(`${modifier}+P`);
        await this.page.waitForSelector(".quick-input-widget");

        // Search for the filename
        await this.page.keyboard.insertText(filename);
        await this.page.waitForTimeout(1000);

        // Check if we have results
        const quickInputList = this.page.locator(".quick-input-list");
        if (await quickInputList.isVisible()) {
          // Verify the result text to ensure we don't open a random file if possible
          // For now, just pressing Enter is what the original tests did
          await this.page.keyboard.press("Enter");
          fileOpened = true;
          break;
        } else {
          // Close and retry
          await this.page.keyboard.press("Escape");
          await this.page.waitForTimeout(1000);
        }
      } catch (e) {
        testLogger.warn(
          `Error opening file (attempt ${i + 1}/${retries}), retrying...: ${
            e instanceof Error ? e.message : String(e)
          }`,
        );
        await this.page.keyboard.press("Escape");
        await this.page.waitForTimeout(500);
      }
    }

    if (!fileOpened) {
      throw new Error(
        `Could not open file '${filename}' via Quick Open after ${retries} attempts.`,
      );
    }

    // Wait for editor to open (generic check for a tab with the filename)
    // Note: Filenames in tabs might be truncated or just the basename.
    // We'll assume the basename is unique enough for these tests.
    const basename = filename.split("/").pop() || filename;
    const tab = this.page.locator(".tab-label", { hasText: basename });
    try {
      await expect(tab).toBeVisible({ timeout: 10000 });
    } catch (e) {
      testLogger.warn(
        `Tab for ${basename} not found or not visible. Proceeding, but this might be an issue.`,
      );
    }
  }

  /**
   * Opens the Explorer view and clicks on a file.
   * This simulates a user navigating via the Side Bar.
   * Supports nested paths (e.g. "docs/rfcs/stage-0/test.md") by expanding folders.
   */
  async openExplorerAndClickFile(filePath: string) {
    testLogger.debug(`Opening Explorer and clicking file: ${filePath}`);

    // 1. Open Explorer
    await this.executeCommand("View: Show Explorer");

    // 2. Wait for the explorer to be visible
    await this.page.waitForSelector(".explorer-viewlet", { timeout: 5000 });

    // Refresh to ensure new files are visible
    await this.executeCommand("File: Refresh Explorer");
    await this.page.waitForTimeout(1000);

    // 3. Walk the path and expand folders
    const segments = filePath.split("/");

    for (let i = 0; i < segments.length; i++) {
      const segment = segments[i];
      const isFile = i === segments.length - 1;

      testLogger.debug(`Looking for ${isFile ? "file" : "folder"}: ${segment}`);

      // Find the item
      // We use the accessible 'treeitem' role which VS Code provides.
      const item = this.page
        .getByRole("treeitem", { name: segment, exact: true })
        .first();

      try {
        await item.waitFor({ state: "visible", timeout: 5000 });
      } catch (e) {
        testLogger.error(`Could not find '${segment}' in Explorer.`);
        const items = await this.page
          .locator(".monaco-list-row .label-name")
          .allInnerTexts();
        testLogger.error(`Visible Explorer Items: ${items.join(" | ")}`);
        throw new Error(
          `'${segment}' not found in Explorer. Path so far: ${segments
            .slice(0, i)
            .join("/")}`,
        );
      }

      if (isFile) {
        // Click the file to open it
        await item.locator("a").click();
      } else {
        // It's a folder, ensure it is expanded
        const expanded = await item.getAttribute("aria-expanded");
        if (expanded !== "true") {
          await item.click();
          // Wait for expansion (next item in path should become visible)
          // We'll just wait a short moment for animation/list update
          await this.page.waitForTimeout(500);
        }
      }
    }
  }

  /**
   * Runs the "Exosuit: Open in Studio" command and waits for the Webview to appear.
   */
  async openInStudio(viewTitle: string = "Exosuit Studio"): Promise<Frame> {
    await this.executeCommand("Exosuit: Open in Studio");

    return await findWebviewFrame(this.page, {
      title: viewTitle,
      timeout: 20000,
    });
  }
}
