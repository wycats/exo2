import * as fs from "fs/promises";
import * as path from "path";
import * as toml from "smol-toml";
import { testLogger } from "./test-logger";

export class ScenarioBuilder {
  private files: Map<string, string | Uint8Array> = new Map();

  constructor(private rootPath: string) {}

  private defaultSettings = {
    "security.workspace.trust.enabled": true,
    "notebook.output.textLineLimit": 1000,
    "notebook.output.scrolling": true,
    "notebook.confirmDeleteRunningCell": false,
    "workbench.startupEditor": "none",
    "exosuit.telemetry.enabled": false,
  };

  /**
   * Creates a basic "Genesis" state with AGENTS.md
   */
  withAgentsMd(
    mission: string = "Test Mission",
    mode: string = "pair-programmer",
  ) {
    const content = `# Agent Workflow\n\nMission: ${mission}\nMode: ${mode}\n`;
    this.files.set("AGENTS.md", content);
    return this;
  }

  /**
   * Adds a plan.toml with a specific phase
   */
  withPhase(phaseId: string, title: string, status: string = "in-progress") {
    const plan = {
      meta: {
        version: "0.2.0",
      },
      epochs: [
        {
          id: "epoch-1",
          title: "Epoch 1",
          status: "in-progress",
          ulid: "01TEST00000000000000000001",
          slug: "epoch-1",
          aliases: ["epoch-1"],
          phases: [
            {
              id: phaseId,
              title: title,
              status: status,
              ulid: "01TEST00000000000000000002",
              slug: phaseId,
              aliases: [phaseId],
              tasks: [],
            },
          ],
        },
      ],
    };
    this.files.set("docs/agent-context/plan.toml", toml.stringify(plan));
    return this;
  }

  /**
   * Adds a minimal implementation-plan.toml for the current phase
   */
  withImplementationPlan(
    phaseId: string = "phase-1",
    title: string = "Test Phase",
  ) {
    const plan = {
      phase: {
        id: phaseId,
        title: title,
        rfcs: [],
      },
      plan: {
        goals: [],
      },
      verification: {
        automated: [],
        manual: [],
      },
    };
    this.files.set(
      "docs/agent-context/current/implementation-plan.toml",
      toml.stringify(plan),
    );
    return this;
  }

  /**
   * Adds an RFC file
   */
  withRfc(stage: string, id: string, title: string, content: string = "") {
    const fileContent = `---
title: ${title}
stage: ${stage.replace("stage-", "")}
feature: ${title}
---

# RFC ${id}: ${title}

${content}
`;
    this.files.set(
      `docs/rfcs/${stage}/${id}-${title.toLowerCase().replace(/\s+/g, "-")}.md`,
      fileContent,
    );
    return this;
  }

  /**
   * Adds ideas.toml with the given ideas
   */
  withIdeas(
    ideas: Array<{
      id?: string;
      title: string;
      description?: string;
      status?: string;
      tags?: string[];
      created_at?: string;
      source?: string;
      related_tasks?: string[];
    }>,
  ) {
    const ideasData = {
      ideas: ideas.map((idea) => ({
        id: idea.id ?? crypto.randomUUID(),
        title: idea.title,
        description: idea.description ?? "",
        status: idea.status ?? "new",
        tags: idea.tags ?? [],
        created_at: idea.created_at ?? new Date().toISOString(),
        source: idea.source ?? "user",
        related_tasks: idea.related_tasks ?? [],
      })),
    };
    this.files.set("docs/agent-context/ideas.toml", toml.stringify(ideasData));
    return this;
  }

  /**
   * Adds inbox.toml with the given intents.
   *
   * Intent statuses: "pending", "acknowledged", "resolved", "rejected"
   * Categories: "guidance", "correction", "question", "feedback"
   * Urgency levels: "immediate", "next-touch", "when-convenient"
   */
  withInbox(
    intents: Array<{
      id?: string;
      subject: string;
      body?: string;
      status?: "pending" | "acknowledged" | "resolved" | "rejected";
      category?: "guidance" | "correction" | "question" | "feedback";
      scope?: string;
      urgency?: "immediate" | "next-touch" | "when-convenient";
      created?: string;
      updated?: string;
      resolution?: string;
    }>,
  ) {
    const inboxData = {
      intent: intents.map((intent, index) => ({
        id:
          intent.id ??
          `intent-test-${String(index + 1).padStart(4, "0")}-${crypto
            .randomUUID()
            .slice(0, 8)}`,
        subject: intent.subject,
        body: intent.body ?? "",
        status: intent.status ?? "pending",
        category: intent.category ?? "guidance",
        scope: intent.scope ?? "global",
        urgency: intent.urgency ?? "next-touch",
        created: intent.created ?? new Date().toISOString(),
        ...(intent.updated && { updated: intent.updated }),
        ...(intent.resolution && { resolution: intent.resolution }),
      })),
    };
    this.files.set("docs/agent-context/inbox.toml", toml.stringify(inboxData));
    return this;
  }

  /**
   * Adds a raw file at a specific path
   */
  withFile(relativePath: string, content: string | Uint8Array) {
    this.files.set(relativePath, content);
    return this;
  }

  /**
   * Creates empty optional files to prevent ENOENT errors in tests that
   * don't need ideas, inbox, or implementation plan content.
   *
   * This is a convenience method for tests that only need the basic
   * structure without specific content in optional files.
   */
  withEmptyOptionals() {
    // Empty ideas - no ideas to display
    const emptyIdeas = {
      ideas: [],
    };
    this.files.set("docs/agent-context/ideas.toml", toml.stringify(emptyIdeas));

    // Empty inbox - no pending intents
    const emptyInbox = {
      intent: [],
    };
    this.files.set("docs/agent-context/inbox.toml", toml.stringify(emptyInbox));

    // Minimal implementation plan - just structure, no content
    const minimalPlan = {
      phase: {
        id: "phase-1",
        title: "Test Phase",
        rfcs: [],
      },
      plan: {
        goals: [],
      },
      verification: {
        automated: [],
        manual: [],
      },
    };
    this.files.set(
      "docs/agent-context/current/implementation-plan.toml",
      toml.stringify(minimalPlan),
    );

    return this;
  }

  /**
   * Commits all queued files to the Holodeck
   */
  async apply() {
    // Inject default settings if not present
    if (!this.files.has(".vscode/settings.json")) {
      this.withFile(
        ".vscode/settings.json",
        JSON.stringify(this.defaultSettings, null, 2),
      );
    } else {
      // If present, we might want to merge, but for now let's assume the user knows what they are doing
      // or we can implement a deep merge later if needed.
      // A simple merge for now:
      try {
        const existingContent = this.files.get(".vscode/settings.json");
        if (typeof existingContent === "string") {
          const existing = JSON.parse(existingContent);
          const merged = { ...this.defaultSettings, ...existing };
          this.files.set(
            ".vscode/settings.json",
            JSON.stringify(merged, null, 2),
          );
        }
      } catch (e) {
        testLogger.warn(
          "Failed to merge settings, using provided content as is.",
        );
      }
    }

    // Ensure standard directories exist to prevent ENOENT in DashboardProvider
    const standardDirs = [
      "docs/rfcs/stage-0",
      "docs/rfcs/stage-1",
      "docs/rfcs/stage-2",
      "docs/rfcs/stage-3",
      "docs/rfcs/stage-4",
      "docs/agent-context/current",
    ];

    for (const dir of standardDirs) {
      const fullPath = path.join(this.rootPath, dir);
      testLogger.debug(`[Scenario] Creating directory: ${fullPath}`);
      await fs.mkdir(fullPath, { recursive: true });
    }

    for (const [relativePath, content] of this.files) {
      const fullPath = path.join(this.rootPath, relativePath);
      await fs.mkdir(path.dirname(fullPath), { recursive: true });
      await fs.writeFile(fullPath, content);
    }
    // Clear queue after apply to allow incremental updates
    this.files.clear();
  }
}
