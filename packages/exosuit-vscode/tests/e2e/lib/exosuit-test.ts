import { Page, Frame, Locator, expect } from "@playwright/test";
import * as path from "path";
import { fileURLToPath } from "url";
import { WorkbenchFixture } from "../workbench-fixture";
import { NotebookFixture } from "../notebook-fixture";
import {
  CanonicalSeedBuilder,
  ExoCliSeedCommandRunner,
  resolveExoSeedBinary,
} from "../canonical-seed";
import { ScenarioBuilder } from "../scenarios";
import { findWebviewFrame, WebviewMonitor } from "../utils";
import { testLogger } from "../test-logger";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "../../../../..");

/**
 * Base class for all Exosuit Page Objects.
 * Wraps a Frame (for Webviews) or Page (for main window).
 */
export abstract class BasePage {
  constructor(protected root: Frame | Page | Locator) {}

  protected get locator() {
    return this.root.locator.bind(this.root);
  }

  async isVisible() {
    // If it's a frame, we check if the frame element is attached?
    // Or we check for a known element inside.
    // Default to checking #app
    return await this.root.locator("#app").isVisible();
  }
}

/**
 * Page Object for the Exosuit Studio (Rich Context Editor).
 */
export class StudioPage extends BasePage {
  constructor(frame: Frame) {
    super(frame);
  }

  async getContent() {
    // root is always a Frame for StudioPage (set in constructor)
    return await (this.root as Frame).content();
  }

  /**
   * Checks if text is visible anywhere in the page (including textboxes).
   * First checks regular text content, then falls back to input values.
   * Uses polling to wait for reactive updates.
   */
  async expectContent(text: string, options?: { timeout?: number }) {
    const timeout = options?.timeout ?? 10000;
    const app = this.root.locator("#app");
    const mounted = this.root.locator(
      'body[data-exosuit-studio-mounted="true"]',
    );
    await expect(mounted).toBeVisible({ timeout });
    await expect(app).toBeVisible({ timeout });

    // Use toPass for polling to handle reactive updates
    await expect(async () => {
      // Try regular text content first
      const hasText = await app.locator(`text=${text}`).count();
      if (hasText > 0) {
        await expect(app.locator(`text=${text}`).first()).toBeVisible({
          timeout: 1000,
        });
        return;
      }

      // Fall back to checking textbox values
      const textboxes = app.getByRole("textbox");
      const count = await textboxes.count();
      for (let i = 0; i < count; i++) {
        const value = await textboxes.nth(i).inputValue();
        if (value.includes(text)) {
          return; // Found the text in a textbox
        }
      }

      // If neither found, throw to trigger retry
      throw new Error(
        `Expected to find "${text}" in page content or input values`,
      );
    }).toPass({ timeout });
  }

  /**
   * Checks if text is visible in a textbox input.
   */
  async expectInputValue(text: string) {
    const app = this.root.locator("#app");
    await expect(app).toBeVisible({ timeout: 10000 });

    const textboxes = app.getByRole("textbox");
    const count = await textboxes.count();
    for (let i = 0; i < count; i++) {
      const value = await textboxes.nth(i).inputValue();
      if (value.includes(text)) {
        return; // Found
      }
    }
    throw new Error(
      `Expected to find "${text}" in a textbox input, but it was not found.`,
    );
  }

  // --- Semantic Elements ---

  get toc() {
    return this.root.locator(".sticky-toc");
  }

  get activeTocItem() {
    return this.root.locator(".toc-item.active");
  }

  get rfcContent() {
    return this.root.locator(".rfc-content");
  }

  get author() {
    return this.root.locator(".meta-item", { hasText: "GitHub Copilot" }); // Default author for tests
  }

  get shiki() {
    return this.root.locator("pre.shiki");
  }

  get header() {
    return this.root.locator("h2", { hasText: "Walkthrough" });
  }

  getEntry(text: string) {
    return this.root.locator(".card", { hasText: text });
  }

  // --- Actions ---

  async scrollTo(selector: string) {
    const element = this.root.locator(selector);
    await element.evaluate((el) => el.scrollIntoView({ block: "center" }));
  }
}

/**
 * The Exosuit Test Harness.
 * Provides a high-level API for setting up the Holodeck and interacting with the UI.
 */
export class ExosuitTest {
  public readonly monitor: WebviewMonitor;

  constructor(
    public readonly page: Page,
    public readonly workbench: WorkbenchFixture,
    public readonly holodeckPath: string,
  ) {
    this.monitor = new WebviewMonitor(page);
  }

  /**
   * Access the ScenarioBuilder to setup the Holodeck.
   */
  get holodeck() {
    return new ScenarioBuilder(this.holodeckPath);
  }

  /**
   * Access the canonical daemon/SQLite-backed seed builder.
   */
  get seed() {
    const exoBin = resolveExoSeedBinary(repoRoot);
    return new CanonicalSeedBuilder({
      workspaceRoot: this.holodeckPath,
      runner: new ExoCliSeedCommandRunner(exoBin, this.holodeckPath),
    });
  }

  get seedMore() {
    const exoBin = resolveExoSeedBinary(repoRoot);
    return new CanonicalSeedBuilder({
      workspaceRoot: this.holodeckPath,
      runner: new ExoCliSeedCommandRunner(exoBin, this.holodeckPath),
      reset: false,
    });
  }

  /**
   * Access the NotebookFixture for notebook interactions.
   */
  get notebook() {
    return new NotebookFixture(this.page);
  }

  /**
   * Dumps captured console logs from the webview.
   */
  dumpLogs() {
    const logs = this.monitor.getConsoleMessages();
    if (logs.length > 0) {
      testLogger.error("--- Webview Console Logs ---");
      for (const line of logs) {
        testLogger.error(line);
      }
      testLogger.error("--- end Webview Console Logs ---");
    }
    const errors = this.monitor.getPageErrors();
    if (errors.length > 0) {
      testLogger.error("--- Webview Page Errors ---");
      for (const e of errors) {
        testLogger.error(e.message);
      }
      testLogger.error("--- end Webview Page Errors ---");
    }
  }

  /**
   * Dumps the current state of all frames in the page.
   * Useful for debugging test failures or verifying frame hierarchy.
   */
  async dumpFrames() {
    const frames = this.page.frames();
    testLogger.error(`[Holodeck] Found ${frames.length} frames:`);

    for (const [i, frame] of frames.entries()) {
      try {
        const title = await frame.title();
        const url = frame.url();
        const name = frame.name();
        testLogger.error(
          `  Frame ${i}: Title="${title}" Name="${name}" URL="${url}"`,
        );

        // If it looks like a webview, dump a snippet of content
        if (url.startsWith("vscode-webview://")) {
          const content = await frame.content();
          testLogger.error(
            `    Content (First 500 chars): ${content
              .substring(0, 500)
              .replace(/\n/g, " ")}...`,
          );

          // Check for common key elements to give immediate clues
          const hasApp = (await frame.$("#app")) !== null;
          const hasActiveFrame =
            (await frame.$("iframe#active-frame")) !== null;
          testLogger.error(
            `    Selectors: #app=${hasApp}, iframe#active-frame=${hasActiveFrame}`,
          );
        }
      } catch (e) {
        testLogger.error(
          `  Frame ${i}: <Error accessing frame: ${(e as Error).message}>`,
        );
      }
    }
    testLogger.error(`[Holodeck] Dump Complete.`);
  }

  /**
   * Opens a file in the Studio and returns a StudioPage object.
   */
  async openInStudio(
    filename: string,
    viewTitle: string = "Exosuit Studio",
  ): Promise<StudioPage> {
    await this.workbench.openFile(filename);
    const frame = await this.workbench.openInStudio(viewTitle);
    return new StudioPage(frame);
  }

  /**
   * Finds an existing Studio frame for a specific context (URI).
   */
  async findStudio(contextUri: string): Promise<StudioPage> {
    const frame = await findWebviewFrame(this.page, {
      contextId: contextUri,
      timeout: 10000,
    });
    if (!frame)
      throw new Error(`Could not find Studio frame for ${contextUri}`);
    return new StudioPage(frame);
  }

  /**
   * Opens a generic Webview by command and returns a BasePage.
   */
  async openWebview(command: string, viewTitle: string): Promise<BasePage> {
    await this.workbench.executeCommand(command);
    const frame = await findWebviewFrame(this.page, {
      title: viewTitle,
      timeout: 10000,
    });
    if (!frame)
      throw new Error(`Could not find Webview frame for ${viewTitle}`);

    // Return a simple wrapper
    return new (class extends BasePage {
      constructor(f: Frame) {
        super(f);
      }
    })(frame);
  }
}
