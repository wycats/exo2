import { Page, Frame } from "playwright";
import { testLogger } from "./test-logger";

export async function findWebviewFrame(
  page: Page,
  options: {
    title?: string;
    content?: string;
    contextId?: string;
    timeout?: number;
  },
): Promise<Frame> {
  const start = Date.now();
  const timeout = options.timeout || 30000;
  const targetTitle = options.title;
  const targetContent = options.content;
  const targetContextId = options.contextId;

  testLogger.log(
    `Searching for Webview frame (Title: ${targetTitle}, Content: ${targetContent}, ContextId: ${targetContextId})...`,
  );

  while (Date.now() - start < timeout) {
    const frames = page.frames();

    for (const frame of frames) {
      try {
        // Check Context ID (Most specific)
        if (targetContextId) {
          const meta = await frame.$(
            `meta[name="exosuit-context-id"][content="${targetContextId}"]`,
          );
          if (meta) {
            testLogger.log(`Found frame via contextId: "${targetContextId}"`);
            return frame;
          }
        }

        // Check Title
        if (targetTitle) {
          const title = await frame.title();
          if (title === targetTitle) {
            // Extra verification for Dashboard to avoid false positives
            if (title === "Exosuit Dashboard") {
              const hasApp = await frame.$("#app");
              if (hasApp) {
                testLogger.log(
                  `Found frame via title: "${title}" (verified content #app)`,
                );
                return frame;
              }
            } else {
              testLogger.log(`Found frame via title: "${title}"`);
              return frame;
            }
          }
        }

        // Check Content
        if (targetContent) {
          const app = await frame.$("#app");
          if (app) {
            const text = await frame.innerText("body");
            if (text.includes(targetContent)) {
              testLogger.log(`Found frame via content: "${targetContent}"`);
              return frame;
            }
          }
        }
      } catch (e) {
        // Frame might be detached or navigating, ignore
      }
    }

    await page.waitForTimeout(500);
  }
  throw new Error(
    `Could not find Webview Frame (Title: ${targetTitle}, ContextId: ${targetContextId}) within timeout`,
  );
}

export async function findDashboardFrame(page: Page): Promise<Frame> {
  // Try title first, then content fallback
  try {
    return await findWebviewFrame(page, {
      title: "Exosuit Dashboard",
      timeout: 5000,
    });
  } catch (e) {
    testLogger.log("Dashboard not found by title, trying content...");
    return await findWebviewFrame(page, { content: "CURRENT PHASE" });
  }
}

export async function findStudioFrame(page: Page): Promise<Frame> {
  return await findWebviewFrame(page, { title: "Exosuit Studio" });
}

export class WebviewMonitor {
  private consoleMessages: string[] = [];
  private consoleErrors: string[] = [];
  private pageErrors: Error[] = [];
  private failedRequests: Array<{
    url: string;
    resourceType: string;
    errorText?: string;
  }> = [];
  private errorResponses: Array<{
    url: string;
    resourceType: string;
    status: number;
  }> = [];

  constructor(page: Page) {
    page.on("console", (msg) => {
      const text = msg.text();
      const locationUrl = msg.location().url;
      this.consoleMessages.push(text);
      if (isBenignConsoleError(text, locationUrl)) {
        return;
      }
      if (msg.type() === "error") {
        this.consoleErrors.push(text);
        testLogger.error(`[Webview Console Error]: ${text}`);
      }
    });
    page.on("pageerror", (err) => {
      this.pageErrors.push(err);
      testLogger.error(`[Webview Page Error]: ${err.message}`);
    });

    // Optional network-level tracing to help identify noisy 403/404 sources.
    // Enable with EXOSUIT_E2E_LOG_FAILED_REQUESTS=1.
    page.on("requestfailed", (request) => {
      const url = request.url();
      const failure = request.failure();
      const resourceType = request.resourceType();

      if (isBenignWebviewRequest(url)) {
        return;
      }

      this.failedRequests.push({
        url,
        resourceType,
        errorText: failure?.errorText,
      });

      if (process.env.EXOSUIT_E2E_LOG_FAILED_REQUESTS) {
        testLogger.warn(
          `[Webview Request Failed]: ${resourceType} ${url} ${
            failure?.errorText ?? ""
          }`.trim(),
        );
      }
    });

    page.on("response", (response) => {
      const url = response.url();
      const status = response.status();
      const resourceType = response.request().resourceType();

      // Only track responses likely originating from webviews.
      if (status < 400) {
        return;
      }
      if (isBenignWebviewRequest(url)) {
        return;
      }
      if (!isLikelyWebviewUrl(url)) {
        return;
      }

      this.errorResponses.push({ url, resourceType, status });

      if (process.env.EXOSUIT_E2E_LOG_FAILED_REQUESTS) {
        testLogger.warn(`[Webview HTTP ${status}]: ${resourceType} ${url}`);
      }
    });
  }

  getConsoleMessages() {
    return this.consoleMessages;
  }

  getConsoleErrors() {
    return this.consoleErrors;
  }

  getPageErrors() {
    return this.pageErrors;
  }

  getFailedRequests() {
    return this.failedRequests;
  }

  getErrorResponses() {
    return this.errorResponses;
  }

  assertNoUnexpectedErrors() {
    if (
      this.consoleErrors.length === 0 &&
      this.pageErrors.length === 0 &&
      this.errorResponses.length === 0
    ) {
      return;
    }

    const details: string[] = [];
    if (this.consoleErrors.length > 0) {
      details.push(
        `Console Errors:\n${this.consoleErrors
          .map((e) => `- ${e}`)
          .join("\n")}`,
      );
    }
    if (this.pageErrors.length > 0) {
      details.push(
        `Page Errors:\n${this.pageErrors
          .map((e) => `- ${e.message}`)
          .join("\n")}`,
      );
    }
    if (this.errorResponses.length > 0) {
      details.push(
        `Webview HTTP Errors:\n${this.errorResponses
          .map((e) => `- HTTP ${e.status} ${e.resourceType} ${e.url}`)
          .join("\n")}`,
      );
    }

    throw new Error(
      `Unexpected Webview errors detected.\n\n${details.join("\n\n")}`,
    );
  }

  expectCspViolation(resourceType: string) {
    const violation = this.consoleMessages.find(
      (m) => m.includes("Content Security Policy") && m.includes(resourceType),
    );
    if (!violation) {
      throw new Error(
        `Expected CSP violation for '${resourceType}', but found none.\nConsole:\n${this.consoleMessages.join(
          "\n",
        )}`,
      );
    }
    return violation;
  }
}

function isBenignWebviewRequest(url: string): boolean {
  // Default browser request in many HTML documents.
  if (url.endsWith("/favicon.ico") || url.endsWith("favicon.ico")) {
    return true;
  }
  return false;
}

function isBenignConsoleError(text: string, locationUrl?: string): boolean {
  // VS Code/Electron sometimes emits this generic, URL-less console error for
  // workbench-owned resources during isolated extension-host startup. Do not
  // suppress the same generic text when it originates from a webview; missing
  // webview resources must fail via consoleErrors or errorResponses.
  if (
    text !==
    "Failed to load resource: the server responded with a status of 404 ()"
  ) {
    return false;
  }

  return !locationUrl || !isLikelyWebviewUrl(locationUrl);
}

function isLikelyWebviewUrl(url: string): boolean {
  // VS Code webviews and their asset loading.
  return (
    url.startsWith("vscode-webview://") ||
    url.startsWith("vscode-resource://") ||
    url.includes("vscode-resource.vscode-cdn.net")
  );
}
