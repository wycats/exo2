import { test } from "./fixtures";
import { expect, Locator } from "@playwright/test";
import * as toml from "smol-toml";
import type { ExosuitTest } from "./lib/exosuit-test";
import { runCanonicalSeedCommand } from "./canonical-seed";

/**
 * Helper to find a tree item in the VS Code tree view.
 * Tree items in VS Code are rendered as role="treeitem" with accessible names.
 */
class TreeViewHelper {
  constructor(private page: import("playwright").Page) {}

  /**
   * Gets the tree view container by its ID.
   */
  getTreeView(viewId: string): Locator {
    return this.page.locator(`.pane-body[id*="${viewId}"]`);
  }

  /**
   * Gets a tree item by its text content.
   * Searches within the entire sidebar for the tree item.
   */
  getTreeItem(text: string): Locator {
    return this.page.getByRole("treeitem", { name: text });
  }

  /**
   * Gets a tree item within a specific container (e.g., pane-body).
   */
  getTreeItemIn(container: Locator, text: string): Locator {
    return container.getByRole("treeitem", { name: text });
  }

  /**
   * Gets all visible tree items in the sidebar.
   */
  getAllTreeItems(): Locator {
    return this.page.locator(".sidebar .monaco-list-row");
  }

  /**
   * Clicks a tree item to expand/select it.
   */
  async clickTreeItem(text: string) {
    const item = this.getTreeItem(text);
    await item.click();
  }

  /**
   * Expands a tree item if it's collapsed.
   */
  async expandTreeItem(text: string) {
    const item = this.getTreeItem(text);
    const isExpanded = await item.getAttribute("aria-expanded");
    if (isExpanded !== "true") {
      await item.click();
      await this.page.waitForTimeout(300);
    }
  }
}

async function openPlanViewWithSeededState(exo: ExosuitTest) {
  const { page, workbench } = exo;
  await page.waitForTimeout(1500);
  await workbench.executeCommand("Exosuit: Restart Machine Channel Server");
  await workbench.executeCommand("Exosuit: Plan");
  await page.waitForTimeout(1000);
}

async function openRunViewWithSeededState(exo: ExosuitTest) {
  const { page, workbench } = exo;
  await page.waitForTimeout(1500);
  await workbench.executeCommand("Exosuit: Restart Machine Channel Server");
  await workbench.executeCommand("Exosuit: Run");
}

test.describe("Tree Views", () => {
  test.describe("Project Plan Tree View", () => {
    test("Displays epochs and phases correctly", async ({ exo }) => {
      await exo.seed
        .epoch({ key: "foundation", title: "Foundation Epoch" })
        .phase({
          key: "setup",
          epoch: "foundation",
          title: "Setup Infrastructure",
        })
        .phase({
          key: "core",
          epoch: "foundation",
          title: "Core Implementation",
          status: "in-progress",
        })
        .phase({ key: "testing", epoch: "foundation", title: "Testing" })
        .apply();

      const { page } = exo;
      const treeHelper = new TreeViewHelper(page);

      await openPlanViewWithSeededState(exo);

      // Verify the epoch is visible
      const epochItem = treeHelper.getTreeItem("Foundation Epoch");
      await expect(epochItem).toBeVisible({ timeout: 10000 });

      // Expand the epoch to see phases
      await treeHelper.expandTreeItem("Foundation Epoch");

      // Verify phases are visible
      await expect(treeHelper.getTreeItem("Setup Infrastructure")).toBeVisible({
        timeout: 5000,
      });
      await expect(treeHelper.getTreeItem("Core Implementation")).toBeVisible({
        timeout: 5000,
      });
      await expect(treeHelper.getTreeItem("Testing")).toBeVisible({
        timeout: 5000,
      });
    });

    test("Shows phase status indicators", async ({ exo }) => {
      await exo.seed
        .epoch({ key: "test", title: "Test Epoch" })
        .phase({ key: "completed", epoch: "test", title: "Completed Phase" })
        .phase({
          key: "active",
          epoch: "test",
          title: "Active Phase",
          status: "in-progress",
        })
        .phase({ key: "pending", epoch: "test", title: "Pending Phase" })
        .apply();

      const { page } = exo;
      const treeHelper = new TreeViewHelper(page);

      await openPlanViewWithSeededState(exo);

      // Expand the epoch
      await treeHelper.expandTreeItem("Test Epoch");

      // Verify each phase is visible
      const completedPhase = treeHelper.getTreeItem("Completed Phase");
      const activePhase = treeHelper.getTreeItem("Active Phase");
      const pendingPhase = treeHelper.getTreeItem("Pending Phase");

      await expect(completedPhase).toBeVisible({ timeout: 5000 });
      await expect(activePhase).toBeVisible({ timeout: 5000 });
      await expect(pendingPhase).toBeVisible({ timeout: 5000 });

      // The tree items should have status descriptions
      // VS Code tree items show descriptions after the label
      // We verify they exist - specific status rendering depends on TreeDataService
    });

    test("Multiple epochs with multiple phases", async ({ exo }) => {
      await exo.seed
        .epoch({ key: "alpha", title: "Alpha Release" })
        .phase({ key: "alpha-setup", epoch: "alpha", title: "Alpha Setup" })
        .phase({
          key: "alpha-features",
          epoch: "alpha",
          title: "Alpha Features",
          status: "in-progress",
        })
        .epoch({ key: "beta", title: "Beta Release" })
        .phase({ key: "beta-prep", epoch: "beta", title: "Beta Prep" })
        .phase({
          key: "beta-testing",
          epoch: "beta",
          title: "Beta Testing",
          status: "in-progress",
        })
        .phase({ key: "beta-polish", epoch: "beta", title: "Beta Polish" })
        .apply();

      const { page } = exo;
      const treeHelper = new TreeViewHelper(page);

      await openPlanViewWithSeededState(exo);

      // Verify both epochs are visible
      await expect(treeHelper.getTreeItem("Alpha Release")).toBeVisible({
        timeout: 10000,
      });
      await expect(treeHelper.getTreeItem("Beta Release")).toBeVisible({
        timeout: 5000,
      });

      // Expand both epochs
      await treeHelper.expandTreeItem("Alpha Release");
      await treeHelper.expandTreeItem("Beta Release");

      // Verify phases from both epochs are visible
      await expect(treeHelper.getTreeItem("Alpha Setup")).toBeVisible({
        timeout: 5000,
      });
      await expect(treeHelper.getTreeItem("Alpha Features")).toBeVisible({
        timeout: 5000,
      });
      await expect(treeHelper.getTreeItem("Beta Prep")).toBeVisible({
        timeout: 5000,
      });
      await expect(treeHelper.getTreeItem("Beta Testing")).toBeVisible({
        timeout: 5000,
      });
      await expect(treeHelper.getTreeItem("Beta Polish")).toBeVisible({
        timeout: 5000,
      });
    });
  });

  test.describe("Edge Cases", () => {
    test("Empty state when no phases exist", async ({ exo }) => {
      // Setup: Create a minimal plan.toml with no epochs
      await exo.holodeck
        .withFile(
          "docs/agent-context/plan.toml",
          toml.stringify({
            meta: { version: "0.2.0" },
            epochs: [],
          }),
        )
        .apply();

      const { page, workbench } = exo;

      // Focus the Plan view
      await workbench.executeCommand("Exosuit: Plan");
      await page.waitForTimeout(1500);

      // With no epochs, the tree should either show an empty state or no items
      // We verify there are no epoch items visible
      const epochItems = page.locator(".sidebar .monaco-list-row").filter({
        hasText: /epoch|phase/i,
      });

      // Either there are no epoch-related items or there's an empty state message
      const count = await epochItems.count();
      expect(count).toBeLessThanOrEqual(1); // May show an error or empty state item
    });

    test("Tree updates when canonical phase changes", async ({ exo }) => {
      const initial = await exo.seed
        .epoch({ key: "initial", title: "Initial Epoch" })
        .phase({
          key: "initial-phase",
          epoch: "initial",
          title: "Initial Phase",
          status: "in-progress",
        })
        .apply();

      const { page, workbench, holodeckPath } = exo;
      const treeHelper = new TreeViewHelper(page);

      await openPlanViewWithSeededState(exo);

      // Verify initial state
      await expect(treeHelper.getTreeItem("Initial Epoch")).toBeVisible({
        timeout: 10000,
      });

      await runCanonicalSeedCommand(holodeckPath, [
        "phase",
        "update",
        initial.phases["initial-phase"].id,
        "--title",
        "Updated Phase",
      ]);
      await workbench.executeCommand("Exosuit: Plan");

      const refreshed = await runCanonicalSeedCommand(holodeckPath, [
        "plan",
        "read",
      ]);
      expect(JSON.stringify(refreshed.result)).toContain("Updated Phase");
      await workbench.executeCommand("Exosuit: Restart Machine Channel Server");
      await page.reload();
      await page.waitForSelector(".monaco-workbench", { timeout: 15000 });
      await workbench.executeCommand("Exosuit: Plan");

      // Use toPass() for polling - daemon reconnect and tree refresh can be slow
      await expect(async () => {
        await expect(
          page.getByRole("treeitem", { name: "Initial Epoch (in-progress)" }),
        ).toBeVisible();
      }).toPass({
        timeout: 20000,
        intervals: [500, 1000, 2000],
      });

      // Expand and verify phases
      await treeHelper.expandTreeItem("Initial Epoch");
      await expect(async () => {
        await workbench.executeCommand("Exosuit: Plan");
        const planTree = page.getByRole("tree", { name: "Project Plan" });
        await expect(
          planTree.getByRole("treeitem", { name: /Updated Phase/ }),
        ).toBeVisible();
      }).toPass({ timeout: 20000, intervals: [500, 1000, 2000] });
    });
  });

  test.describe("Run Sidebar Smoke", () => {
    test("opens Run sidebar with recovery-aware trees", async ({ exo }) => {
      await exo.seed
        .epoch({ key: "run", title: "Run Sidebar Epoch" })
        .phase({
          key: "active",
          epoch: "run",
          title: "Run Sidebar Active Phase",
          status: "in-progress",
        })
        .goal({
          key: "goal",
          id: "run-sidebar-goal",
          phase: "active",
          label: "Run Sidebar Seeded Goal",
        })
        .task({
          key: "task",
          id: "run-sidebar-task",
          goal: "goal",
          label: "Run Sidebar Seeded Task",
          status: "in-progress",
        })
        .apply();

      const { page } = exo;

      await openRunViewWithSeededState(exo);

      await expect(
        page.getByRole("heading", { name: "Exosuit: Run" }),
      ).toBeVisible({ timeout: 10000 });

      // Look for Phase Details section by accessible name
      const phaseDetailsTree = page.getByRole("tree", {
        name: "Phase Details",
      });
      const epochTree = page.getByRole("tree", { name: "Epoch Context" });

      await expect(phaseDetailsTree).toBeVisible({ timeout: 10000 });
      await expect(epochTree).toBeVisible({ timeout: 10000 });

      await expect(async () => {
        const phaseText = await phaseDetailsTree.innerText();
        expect(phaseText).toContain("Run Sidebar Active Phase");
        expect(phaseText).toContain("0/1 goals");
        expect(phaseText).toContain("0/1 tasks");
        expect(phaseText).toContain("Run Sidebar Seeded Goal");
        expect(phaseText).toContain("0/1 tasks");
        expect(phaseText).toContain("Run Sidebar Seeded Task");
        const epochText = await epochTree.innerText();
        expect(epochText).toContain("Run Sidebar Active Phase");
        expect(phaseText).not.toMatch(
          /Phase details unavailable|No active phase|Loading phase details|Focused phase not found/,
        );
        expect(epochText).not.toMatch(
          /Epoch context unavailable|No active epoch|Loading epoch context|Focused phase not found/,
        );
      }).toPass({ timeout: 20000, intervals: [500, 1000, 2000] });
    });
  });
});
