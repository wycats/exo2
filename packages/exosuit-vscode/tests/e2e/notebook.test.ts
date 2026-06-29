import { test } from "./fixtures";
import { expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { testLogger } from "./test-logger";

test.describe("Exosuit Notebook", () => {
  test.beforeEach(async ({ exo }) => {
    // Use ScenarioBuilder to setup the Holodeck
    await exo.holodeck
      .withFile(
        "test.exo",
        `
[phase]
title = "Test Phase"
id = "test-phase"

[plan]
[[plan.goals]]
name = "Test Goal"
`,
      )
      .withFile(
        ".vscode/settings.json",
        JSON.stringify(
          {
            "workbench.editorAssociations": {
              "*.exo": "exosuit-plan",
            },
          },
          null,
          2,
        ),
      )
      .withFile("package.json", JSON.stringify({ name: "exosuit-context" })) // Needed for @file directive test
      .apply();
  });

  test("should open test.exo as a notebook", async ({ exo }) => {
    const { page, holodeckPath } = exo;

    // Wait for VS Code to settle and extension to activate
    testLogger.debug("Waiting for extension activation...");
    const successPath = path.join(holodeckPath, "activation-success.txt");
    const errorPath = path.join(holodeckPath, "activation-error.txt");
    const progressPath = path.join(holodeckPath, "activation-progress.txt");

    let printedProgress = false;
    for (let i = 0; i < 60; i++) {
      // Wait up to 60 seconds
      if (fs.existsSync(successPath)) {
        testLogger.debug("Extension activated successfully.");
        break;
      }
      if (fs.existsSync(errorPath)) {
        testLogger.error("Activation Error Found:");
        testLogger.error(fs.readFileSync(errorPath, "utf8"));
        throw new Error("Extension activation failed.");
      }
      if (fs.existsSync(progressPath) && !printedProgress) {
        testLogger.debug("Activation Progress (Snapshot):");
        testLogger.debug(fs.readFileSync(progressPath, "utf8"));
        printedProgress = true;
      }
      await page.waitForTimeout(1000);
    }

    if (!fs.existsSync(successPath)) {
      testLogger.error("Activation timed out.");
      if (fs.existsSync(progressPath)) {
        testLogger.error("Activation Progress:");
        testLogger.error(fs.readFileSync(progressPath, "utf8"));
      }
      throw new Error("Activation timed out");
    }

    // Check if serializer ran
    const serializerLogPath = path.join(holodeckPath, "serializer.txt");
    if (fs.existsSync(serializerLogPath)) {
      testLogger.debug("Serializer Log Found:");
      testLogger.debug(fs.readFileSync(serializerLogPath, "utf8"));
    } else {
      testLogger.debug("Serializer Log NOT Found - Serializer was not called.");
    }

    await exo.notebook.open("test.exo");

    // Capture console logs from the page to debug extension issues
    page.on("console", (msg) => {
      const text = msg.text();
      if (text.includes("Exosuit")) {
        testLogger.debug(`[Browser Console] ${text}`);
      }
    });

    await page.waitForTimeout(2000);
    const title = await page.title();
    testLogger.debug(`Page Title: ${title}`);

    // Verify Cells
    const count = await exo.notebook.getCellCount();
    expect(count).toBeGreaterThan(0);
  });

  test("should execute directive in notebook", async ({ exo }) => {
    await exo.notebook.open("test.exo");

    // Wait for editor with content
    testLogger.debug("Waiting for notebook editor...");
    await expect(async () => {
      const count = await exo.notebook.getCellCount();
      expect(count).toBeGreaterThan(0);
    }).toPass();

    // Edit second cell (index 1)
    await exo.notebook.setCellContent(1, "@file: package.json");

    // Select Kernel (Ensure Exosuit Kernel is selected)
    await exo.notebook.selectKernel("Exosuit");

    // Execute Cell via Command Palette (More Robust)
    await exo.notebook.runAll();

    // Verify Output
    await exo.notebook.assertOutputContains("exosuit-context");
  });
});
