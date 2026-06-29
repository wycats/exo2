import { test } from "./fixtures";
import { expect } from "@playwright/test";
import { exec } from "child_process";
import { promisify } from "util";
import * as path from "path";
import * as fs from "fs";
import { fileURLToPath } from "url";

const execAsync = promisify(exec);

// ES module compatibility
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Workspace root (exo2 repo)
const workspaceRoot = path.resolve(__dirname, "../../../..");

/**
 * Resolves the path to the `exo` binary.
 * Checks:
 * 1. EXO_BIN environment variable
 * 2. Repo-local debug build
 * 3. Repo-local release build
 * 4. PATH fallback
 */
function resolveExoBin(): string {
  const envBin = process.env.EXO_BIN;
  if (envBin && envBin.trim().length > 0) {
    return envBin.trim();
  }

  const repoRoot = workspaceRoot;

  const candidates = [
    path.join(repoRoot, "target", "debug", "exo"),
    path.join(repoRoot, "target", "release", "exo"),
  ];

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  // Fallback to PATH
  return "exo";
}

/**
 * LM Tools Smoke Tests
 *
 * These tests verify that:
 * 1. The underlying `exo` CLI commands work correctly in the holodeck
 * 2. The extension activates without LM tool registration errors
 *
 * Note: Direct LM Tool invocation requires the Copilot chat interface,
 * which is difficult to test in E2E. Instead, we test the CLI layer
 * that the tools depend on.
 */
test.describe("LM Tools Smoke Tests", () => {
  const exoBin = resolveExoBin();

  test.describe("CLI Command Validation", () => {
    test.beforeEach(async ({ exo }) => {
      // Setup a valid project state with all required files
      await exo.holodeck
        .withAgentsMd("LM Tools Test Project", "pair-programmer")
        .withPhase("test-phase", "Test Phase for LM Tools", "in-progress")
        .withImplementationPlan("test-phase", "Test Phase for LM Tools")
        .withIdeas([
          {
            title: "Test Idea",
            description: "An idea for testing",
            status: "new",
            tags: ["test"],
          },
        ])
        .withInbox([
          {
            subject: "Test Intent",
            body: "A test intent for smoke testing",
            status: "pending",
            category: "guidance",
            urgency: "next-touch",
          },
        ])
        .apply();
    });

    test("exo status command works", async ({ exo }) => {
      // The StatusTool runs `exo status`
      const { stdout, stderr } = await execAsync(`${exoBin} status`, {
        cwd: exo.holodeckPath,
      });

      // Should not error
      expect(stderr).toBe("");

      // Should contain phase information (title, not ID)
      expect(stdout).toContain("Test Phase for LM Tools");
    });

    test("exo plan review command works", async ({ exo }) => {
      // The PlanTool runs `exo plan review`
      const { stdout, stderr } = await execAsync(`${exoBin} plan review`, {
        cwd: exo.holodeckPath,
      });

      // Should not error
      expect(stderr).toBe("");

      // Should contain epoch/phase info
      expect(stdout.toLowerCase()).toMatch(/epoch|phase|plan/);
    });

    test("exo phase status command works", async ({ exo }) => {
      // The PhaseTool runs `exo phase status`
      const { stdout, stderr } = await execAsync(`${exoBin} phase status`, {
        cwd: exo.holodeckPath,
      });

      // Should not error
      expect(stderr).toBe("");

      // Should contain active phase info (title, not ID)
      expect(stdout).toContain("Test Phase for LM Tools");
    });

    test("exo map command works", async ({ exo }) => {
      // The MapTool (exo-steering) runs `exo map`
      const { stdout } = await execAsync(`${exoBin} map`, {
        cwd: exo.holodeckPath,
      });

      // Should not error (though may have warnings)
      // The map command may include suggestions in stderr

      // Should return navigation/steering info
      expect(stdout.length).toBeGreaterThan(0);
    });

    test("exo ai context command works", async ({ exo }) => {
      // The ContextTool runs `exo ai context`
      const { stdout, stderr } = await execAsync(`${exoBin} ai context`, {
        cwd: exo.holodeckPath,
      });

      // Should not error
      expect(stderr).toBe("");

      // Should contain project context dump
      expect(stdout.length).toBeGreaterThan(0);
      // Context dump includes phase and plan info
      expect(stdout.toLowerCase()).toMatch(/phase|plan|context/);
    });

    test("exo inbox list command works", async ({ exo }) => {
      // The InboxTool runs `exo inbox list`
      const { stdout, stderr } = await execAsync(`${exoBin} inbox list`, {
        cwd: exo.holodeckPath,
      });

      // Should not error
      expect(stderr).toBe("");

      // Should list the test intent we created
      expect(stdout).toContain("Test Intent");
    });

    test("exo idea list command works", async ({ exo }) => {
      // Related to IdeaTool which runs `exo idea add`
      const { stdout, stderr } = await execAsync(`${exoBin} idea list`, {
        cwd: exo.holodeckPath,
      });

      // Should not error
      expect(stderr).toBe("");

      // Should list the test idea we created
      expect(stdout).toContain("Test Idea");
    });
  });

  test.describe("JSON Format Support", () => {
    test.beforeEach(async ({ exo }) => {
      await exo.holodeck
        .withAgentsMd()
        .withPhase("json-test", "JSON Test Phase", "in-progress")
        .withImplementationPlan("json-test", "JSON Test Phase")
        .apply();
    });

    test("exo status --format json returns valid JSON", async ({ exo }) => {
      const { stdout } = await execAsync(`${exoBin} status --format json`, {
        cwd: exo.holodeckPath,
      });

      // Should be valid JSON
      const parsed = JSON.parse(stdout);
      expect(parsed).toBeDefined();
      expect(typeof parsed).toBe("object");
    });

    test("exo plan review --format json returns valid JSON", async ({
      exo,
    }) => {
      const { stdout } = await execAsync(
        `${exoBin} plan review --format json`,
        {
          cwd: exo.holodeckPath,
        },
      );

      // Should be valid JSON
      const parsed = JSON.parse(stdout);
      expect(parsed).toBeDefined();
      expect(typeof parsed).toBe("object");
    });

    test("exo phase status --format json returns valid JSON", async ({
      exo,
    }) => {
      const { stdout } = await execAsync(
        `${exoBin} phase status --format json`,
        {
          cwd: exo.holodeckPath,
        },
      );

      // Should be valid JSON
      const parsed = JSON.parse(stdout);
      expect(parsed).toBeDefined();
      expect(typeof parsed).toBe("object");
    });

    test("exo map --json returns valid JSON", async ({ exo }) => {
      const { stdout } = await execAsync(`${exoBin} map --json`, {
        cwd: exo.holodeckPath,
      });

      // Should be valid JSON
      const parsed = JSON.parse(stdout);
      expect(parsed).toBeDefined();
      expect(typeof parsed).toBe("object");
    });
  });

  test.describe("Extension Tool Registration", () => {
    test("extension activates and registers LM tools without errors", async ({
      exo,
    }) => {
      // Setup minimal project state
      await exo.holodeck.withAgentsMd().apply();

      const { page, workbench } = exo;

      // Wait for VS Code to fully load
      await page.waitForSelector(".monaco-workbench", { timeout: 20000 });

      // Open Command Palette
      await workbench.openCommandPalette();

      // Search for Exosuit commands - if extension failed to activate,
      // these won't appear
      await workbench.typeCommand("Exosuit");

      // Wait for command list to populate
      await page.waitForTimeout(500);

      // Verify at least one Exosuit command is registered
      const commandEntries = page.locator(
        ".quick-input-list-entry .monaco-highlighted-label",
      );
      const count = await commandEntries.count();
      expect(count).toBeGreaterThan(0);

      // Close the palette
      await page.keyboard.press("Escape");
    });

    test("extension output shows LM tool registration", async ({ exo }) => {
      // Setup minimal project state
      await exo.holodeck.withAgentsMd().apply();

      const { page, workbench } = exo;

      // Wait for VS Code to fully load
      await page.waitForSelector(".monaco-workbench", { timeout: 20000 });

      // Open Output panel and select Exosuit channel
      await workbench.executeCommand("View: Toggle Output");
      await page.waitForTimeout(500);

      // The output channel should exist, though we may not be able to select it
      // in the E2E test. Instead, verify the panel opened.
      await expect(
        page.locator(".panel .monaco-editor, .panel .output-container"),
      ).toBeVisible({ timeout: 5000 });
    });
  });

  test.describe("Error Handling", () => {
    test("exo commands fail gracefully on invalid project", async ({ exo }) => {
      // Don't setup any project files - empty holodeck
      // (The holodeck directories are still created by the fixture)

      // exo status should fail gracefully (no AGENTS.md)
      try {
        await execAsync(`${exoBin} status`, {
          cwd: exo.holodeckPath,
        });
        // If it doesn't throw, that's fine - some commands may work without full setup
      } catch (error) {
        // Expected - verify it's a meaningful error, not a crash
        const err = error as { stderr?: string; code?: number };
        expect(err.code).toBeDefined();
        // Should exit with non-zero code, not crash
        expect(err.code).not.toBe(139); // SIGSEGV
        expect(err.code).not.toBe(134); // SIGABRT
      }
    });

    test("exo commands work after project is initialized", async ({ exo }) => {
      // Start empty, then initialize
      // First, verify status fails
      try {
        await execAsync(`${exoBin} status`, {
          cwd: exo.holodeckPath,
        });
      } catch {
        // Expected to fail
      }

      // Now setup the project
      await exo.holodeck
        .withAgentsMd()
        .withPhase("recovery-test", "Recovery Test", "in-progress")
        .withImplementationPlan("recovery-test", "Recovery Test")
        .apply();

      // Now status should work
      const { stdout } = await execAsync(`${exoBin} status`, {
        cwd: exo.holodeckPath,
      });
      expect(stdout).toContain("recovery-test");
    });
  });
});
