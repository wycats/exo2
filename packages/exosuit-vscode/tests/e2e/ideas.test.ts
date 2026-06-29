import { test } from "./fixtures";
import { expect } from "@playwright/test";
import * as path from "path";
import * as fs from "fs/promises";
import * as toml from "smol-toml";

// 1. CONFIGURATION
const VIEW_TITLE = "Exosuit Studio";
const IDEAS_PATH = "docs/agent-context/ideas.toml";

/**
 * Generates a valid ideas.toml file with the given ideas.
 */
function generateIdeasToml(
  ideas: Array<{
    id?: string;
    title: string;
    description?: string;
    status?: string;
    tags?: string[];
    created_at?: string;
    source?: string;
  }>,
): string {
  return toml.stringify({
    ideas: ideas.map((idea) => ({
      id: idea.id ?? crypto.randomUUID(),
      title: idea.title,
      description: idea.description ?? "",
      status: idea.status ?? "new",
      tags: idea.tags ?? [],
      created_at: idea.created_at ?? new Date().toISOString(),
      source: idea.source ?? "user",
    })),
  });
}

test.describe("Ideas Feature", () => {
  // All Ideas tests need a minimal plan.toml to prevent extension errors
  test.beforeEach(async ({ exo }) => {
    await exo.holodeck.withPhase("test-phase", "Test Phase").apply();
  });

  test.describe("Happy Path", () => {
    test("Displays ideas list with single idea", async ({ exo }) => {
      // Setup: Create ideas.toml with a single idea
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "Improve Dashboard UX",
              description: "Add better visual feedback for user actions",
              status: "new",
              tags: ["ux", "dashboard"],
            },
          ]),
        )
        .apply();

      // Open in Studio
      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Verify content renders correctly
      await studio.expectContent("Ideas");
      await studio.expectContent("Improve Dashboard UX");
      await studio.expectContent("Add better visual feedback for user actions");
    });

    test("Displays multiple ideas with correct structure", async ({ exo }) => {
      // Setup: Create ideas.toml with multiple ideas
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "First Idea",
              description: "Description for first idea",
              status: "new",
              tags: ["tag1"],
            },
            {
              title: "Second Idea",
              description: "Description for second idea",
              status: "accepted",
              tags: ["tag2", "important"],
            },
            {
              title: "Third Idea",
              description: "Description for third idea",
              status: "implemented",
              tags: [],
            },
          ]),
        )
        .apply();

      // Open in Studio
      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Verify all ideas are visible
      await studio.expectContent("First Idea");
      await studio.expectContent("Second Idea");
      await studio.expectContent("Third Idea");

      // Verify descriptions
      await studio.expectContent("Description for first idea");
      await studio.expectContent("Description for second idea");
      await studio.expectContent("Description for third idea");
    });

    test("Displays tags correctly", async ({ exo }) => {
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "Tagged Idea",
              description: "Has multiple tags",
              tags: ["performance", "critical", "backend"],
            },
          ]),
        )
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Verify tags are rendered
      await studio.expectContent("Tagged Idea");
      await studio.expectContent("performance");
      await studio.expectContent("critical");
      await studio.expectContent("backend");
    });
  });

  test.describe("Edge Cases", () => {
    test("Displays empty state when no ideas exist", async ({ exo }) => {
      // Setup: Create empty ideas.toml (valid TOML but no ideas array)
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          `# Ideas file
# No ideas yet
`,
        )
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // The view should still load but show empty list
      // Check that the Ideas list header is visible but with no items
      await studio.expectContent("Ideas");

      // Verify the ideas-list is present but empty
      // The list should exist with 0 children rendered as idea-cards
      const app = studio["root"].locator("#app");
      await expect(app).toBeVisible();
    });

    test("Handles ideas with missing optional fields", async ({ exo }) => {
      // Manually create TOML without optional fields
      const minimalToml = `
[[ideas]]
id = "minimal-idea"
title = "Minimal Idea"
`;

      await exo.holodeck.withFile(IDEAS_PATH, minimalToml).apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Should render without crashing
      await studio.expectContent("Minimal Idea");
    });

    test("Handles ideas with special characters in title", async ({ exo }) => {
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: 'Feature: Add <code> support & "quotes"',
              description: "Test with <html> entities & ampersands",
            },
            {
              title: "Unicode: 日本語 émojis 🚀",
              description: "Supports international characters",
            },
          ]),
        )
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Special characters should be rendered (may be escaped in HTML)
      await studio.expectContent("Feature:");
      await studio.expectContent("Unicode:");
      await studio.expectContent("🚀");
    });

    test("Handles ideas with very long descriptions", async ({ exo }) => {
      const longDescription =
        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(50);

      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "Long Description Idea",
              description: longDescription,
            },
          ]),
        )
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Should render without layout issues
      await studio.expectContent("Long Description Idea");
      // Check that at least part of the description is visible
      await studio.expectContent("Lorem ipsum");
    });

    test("Handles ideas with multiline markdown descriptions", async ({
      exo,
    }) => {
      const markdownDescription = `
## Overview
This is a **bold** statement with _emphasis_.

### Features
- Feature 1
- Feature 2
- Feature 3

\`\`\`typescript
const code = "example";
\`\`\`
`;

      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "Markdown Idea",
              description: markdownDescription,
            },
          ]),
        )
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      await studio.expectContent("Markdown Idea");
      // Markdown should be rendered - check for some content
      await studio.expectContent("Overview");
      await studio.expectContent("Features");
    });

    test("Handles all status types correctly", async ({ exo }) => {
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "New Idea",
              status: "new",
            },
            {
              title: "Accepted Idea",
              status: "accepted",
            },
            {
              title: "Rejected Idea",
              status: "rejected",
            },
            {
              title: "Implemented Idea",
              status: "implemented",
            },
          ]),
        )
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // All ideas should render with their respective statuses
      await studio.expectContent("New Idea");
      await studio.expectContent("Accepted Idea");
      await studio.expectContent("Rejected Idea");
      await studio.expectContent("Implemented Idea");
    });
  });

  test.describe("Reactivity", () => {
    test("Updates view when a new idea is added", async ({ exo }) => {
      const { holodeckPath } = exo;

      // Initial state: one idea
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "Original Idea",
              description: "This was here first",
            },
          ]),
        )
        .apply();

      // Open in Studio
      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Verify initial state
      await studio.expectContent("Original Idea");

      // Add a new idea by writing to the file
      const ideasFilePath = path.join(holodeckPath, IDEAS_PATH);
      const newContent = generateIdeasToml([
        {
          title: "Original Idea",
          description: "This was here first",
        },
        {
          title: "Dynamically Added Idea",
          description: "This idea was added after initial load",
          tags: ["reactive", "new"],
        },
      ]);

      await fs.writeFile(ideasFilePath, newContent, "utf-8");

      // Wait for reactivity - the view should update
      // Use expectContent which checks both text and textbox values
      await studio.expectContent("Dynamically Added Idea");
    });

    test("Updates view when an idea is removed", async ({ exo }) => {
      const { holodeckPath } = exo;

      // Initial state: two ideas
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "Keeper Idea",
              description: "This one stays",
            },
            {
              title: "Removable Idea",
              description: "This one will be removed",
            },
          ]),
        )
        .apply();

      // Open in Studio
      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Verify both ideas are visible
      await studio.expectContent("Keeper Idea");
      await studio.expectContent("Removable Idea");

      // Remove the second idea
      const ideasFilePath = path.join(holodeckPath, IDEAS_PATH);
      const newContent = generateIdeasToml([
        {
          title: "Keeper Idea",
          description: "This one stays",
        },
      ]);

      await fs.writeFile(ideasFilePath, newContent, "utf-8");

      // Wait for the removed idea to disappear
      const app = studio["root"].locator("#app");
      await expect(app.getByText("Removable Idea")).not.toBeVisible({
        timeout: 15000,
      });

      // Keeper idea should still be there
      await studio.expectContent("Keeper Idea");
    });

    test("Updates view when idea content is modified", async ({ exo }) => {
      const { holodeckPath } = exo;

      // Initial state
      await exo.holodeck
        .withFile(
          IDEAS_PATH,
          generateIdeasToml([
            {
              title: "Mutable Idea",
              description: "Original description",
              status: "new",
            },
          ]),
        )
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Verify initial state
      await studio.expectContent("Mutable Idea");
      await studio.expectContent("Original description");

      // Modify the idea
      const ideasFilePath = path.join(holodeckPath, IDEAS_PATH);
      const newContent = generateIdeasToml([
        {
          title: "Updated Idea Title",
          description: "The description has been changed",
          status: "accepted",
        },
      ]);

      await fs.writeFile(ideasFilePath, newContent, "utf-8");

      // Wait for updates - use expectContent which checks textbox values
      await studio.expectContent("Updated Idea Title");
      await studio.expectContent("The description has been changed");
    });
  });

  test.describe("Large Datasets", () => {
    test("Handles many ideas without performance degradation", async ({
      exo,
    }) => {
      // Generate 20 ideas
      const manyIdeas = Array.from({ length: 20 }, (_, i) => ({
        title: `Idea Number ${i + 1}`,
        description: `This is the description for idea number ${i + 1}`,
        status: ["new", "accepted", "rejected", "implemented"][i % 4],
        tags: [`tag-${i % 5}`, `category-${Math.floor(i / 5)}`],
      }));

      await exo.holodeck
        .withFile(IDEAS_PATH, generateIdeasToml(manyIdeas))
        .apply();

      const studio = await exo.openInStudio("ideas.toml", VIEW_TITLE);

      // Verify first and last ideas are visible (tests scroll/virtualization if any)
      await studio.expectContent("Idea Number 1");
      await studio.expectContent("Idea Number 20");
    });
  });
});
