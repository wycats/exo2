import { Page, expect } from "@playwright/test";
import { testLogger } from "./test-logger";

export class NotebookFixture {
  constructor(private page: Page) {}

  async open(filename: string) {
    // Force activation
    testLogger.debug("Forcing activation...");
    await this.page.keyboard.press("F1");
    await this.page.keyboard.type("Exosuit: Focus Dashboard");
    await this.page.waitForTimeout(1000);
    await this.page.keyboard.press("Enter");
    await this.page.waitForTimeout(2000);

    // 1. Open file via Quick Open
    testLogger.debug(`Opening ${filename} via Quick Open...`);
    const isMac = process.platform === "darwin";
    const modifier = isMac ? "Meta" : "Control";
    await this.page.keyboard.press(`${modifier}+P`);
    await this.page.waitForSelector(".quick-input-widget");
    await this.page.keyboard.type(filename);
    await this.page.waitForTimeout(500);
    await this.page.keyboard.press("Enter");
    await this.page.waitForTimeout(2000);

    // 2. Check if it opened as a notebook
    if ((await this.page.locator(".notebook-editor").count()) > 0) {
      testLogger.debug("Opened directly as notebook.");
      return;
    }

    // 3. If not, try "Reopen With..."
    testLogger.debug("Not a notebook. Trying 'Reopen Editor With...'");
    await this.page.keyboard.press("F1");
    await this.page.waitForSelector(".quick-input-widget");
    await this.page.keyboard.type("View: Reopen Editor With...");
    await this.page.keyboard.press("Enter");
    await this.page.waitForTimeout(1000);

    // Select "Exosuit Plan"
    await this.page.keyboard.type("Exosuit Plan");
    await this.page.waitForTimeout(500);
    await this.page.keyboard.press("Enter");
    await this.page.waitForTimeout(3000);

    if ((await this.page.locator(".notebook-editor").count()) > 0) {
      testLogger.debug("Notebook editor found after Reopen With.");
      return;
    }

    throw new Error("Notebook editor not found.");
  }

  async selectKernel(name: string = "Exosuit") {
    await this.page.keyboard.press("F1");
    await this.page.waitForSelector(".quick-input-widget");
    await this.page.keyboard.type("Notebook: Select Notebook Kernel");
    await this.page.keyboard.press("Enter");
    await this.page.waitForTimeout(1000);
    await this.page.keyboard.type(name);
    await this.page.waitForTimeout(500);
    await this.page.keyboard.press("Enter");
    await this.page.waitForTimeout(1000);
  }

  async getCellCount(): Promise<number> {
    const rows = this.page.locator(".notebook-editor .monaco-list-row");
    await expect(rows).not.toHaveCount(0, { timeout: 10000 });
    return await rows.count();
  }

  async focusCell(index: number) {
    const editor = this.page.locator(
      ".notebook-editor.notebook-editor-editable",
    );
    await editor.waitFor({ state: "visible" });
    await editor.click();

    // Reset to top
    await this.page.keyboard.press("Control+Home");
    await this.page.waitForTimeout(200);

    // Move down to target cell
    for (let i = 0; i < index; i++) {
      await this.page.keyboard.press("ArrowDown");
      await this.page.waitForTimeout(200);
    }
  }

  async setCellContent(index: number, content: string) {
    await this.focusCell(index);

    // Enter edit mode
    await this.page.keyboard.press("Enter");
    await this.page.waitForTimeout(500);

    // Clear content (Select All + Backspace)
    const isMac = process.platform === "darwin";
    const modifier = isMac ? "Meta" : "Control";
    await this.page.keyboard.press(`${modifier}+A`);
    await this.page.keyboard.press("Backspace");

    // Type content
    await this.page.keyboard.type(content);

    // Exit edit mode
    await this.page.keyboard.press("Escape");
  }

  async runAll() {
    await this.page.keyboard.press("F1");
    await this.page.waitForSelector(".quick-input-widget");
    await this.page.keyboard.type("Notebook: Run All");
    await this.page.keyboard.press("Enter");
  }

  async assertOutputContains(text: string, timeout: number = 5000) {
    let found = false;
    const startTime = Date.now();

    while (Date.now() - startTime < timeout) {
      for (const frame of this.page.frames()) {
        try {
          const content = await frame.content();
          if (content.includes(text)) {
            found = true;
            break;
          }
        } catch (e) {
          // Ignore detached frames
        }
      }
      if (found) break;
      await this.page.waitForTimeout(500);
    }

    if (!found) {
      testLogger.error(
        `Expected output '${text}' not found in any frame! Dumping frames...`,
      );
      for (const frame of this.page.frames()) {
        testLogger.error(`Frame: ${frame.url()}`);
        try {
          const content = await frame.content();
          testLogger.error(content.substring(0, 200) + "...");
        } catch (e) {
          testLogger.error("Could not read frame content");
        }
      }
    }

    expect(found).toBe(true);
  }
}
