import { exoExec } from "./exo";

export type EditResource =
  | "plan"
  | "tasks"
  | "walkthrough"
  | "decisions"
  | "ideas"
  | "axioms";
export type EditAction = "add" | "update" | "append";

export async function applyEdit(options: {
  rootPath: string;
  resource: EditResource;
  action: EditAction;
  payload: unknown;
}): Promise<{ stdout: string } | { error: string }> {
  if (options.resource === "tasks") {
    const p = options.payload as any;

    if (options.action === "add") {
      const id = typeof p?.id === "string" ? p.id.trim() : "";
      const label = typeof p?.label === "string" ? p.label : undefined;

      if (!id) {
        return {
          error: "tasks.add requires payload { id: string, label?: string }",
        };
      }

      const args = ["task", "add", id];
      if (label && label.trim().length > 0) {
        args.push(label);
      }

      const stdout = await exoExec({ cwd: options.rootPath, args });
      return { stdout };
    }

    if (options.action === "update") {
      const id = typeof p?.id === "string" ? p.id.trim() : "";
      const status = typeof p?.status === "string" ? p.status : null;

      if (!id || !status) {
        return {
          error:
            "tasks.update requires payload { id: string, status: 'completed' }",
        };
      }

      if (status !== "completed") {
        return {
          error:
            "tasks.update currently supports only status='completed' (Phase 1).",
        };
      }

      const stdout = await exoExec({
        cwd: options.rootPath,
        args: ["task", "complete", id],
      });
      return { stdout };
    }

    return {
      error: `Edit not implemented yet for resource=${options.resource} action=${options.action}`,
    };
  }

  if (options.resource === "plan" && options.action === "update") {
    const p = options.payload as any;
    const id = typeof p?.id === "string" ? p.id.trim() : "";
    const status = typeof p?.status === "string" ? p.status.trim() : "";

    if (!id || !status) {
      return {
        error:
          "plan.update requires payload { id: string, status: 'pending'|'active'|'completed' }",
      };
    }

    const stdout = await exoExec({
      cwd: options.rootPath,
      args: ["plan", "update-status", id, status],
    });
    return { stdout };
  }

  if (options.resource === "ideas" && options.action === "add") {
    const p = options.payload as any;
    const title = typeof p?.title === "string" ? p.title : null;
    const description =
      typeof p?.description === "string" ? p.description : null;
    const tags = Array.isArray(p?.tags)
      ? p.tags.filter((t: unknown) => typeof t === "string")
      : typeof p?.tags === "string"
        ? [p.tags]
        : [];

    if (!title || title.trim().length === 0) {
      return {
        error:
          "ideas.add requires payload { title: string, description?: string, tags?: string[] }",
      };
    }

    const args = ["idea", "add", "--title", title];
    if (description && description.trim().length > 0) {
      args.push("--description", description);
    }
    for (const tag of tags) {
      const t = tag.trim();
      if (t.length > 0) {
        args.push("--tags", t);
      }
    }

    const stdout = await exoExec({ cwd: options.rootPath, args });
    return { stdout };
  }

  if (options.resource === "decisions" && options.action === "append") {
    return {
      error: "decisions.toml is deprecated. Use RFCs for design decisions.",
    };
  }

  if (options.resource === "walkthrough" && options.action === "add") {
    const p = options.payload as any;
    const type = typeof p?.type === "string" ? p.type : null;
    const description =
      typeof p?.description === "string" ? p.description : null;
    const details = typeof p?.details === "string" ? p.details : null;

    if (!type || !description) {
      return {
        error:
          "walkthrough add requires payload { type: 'feat|fix|test|design', description: string, details?: string }",
      };
    }

    const args = [
      "walkthrough",
      "add",
      "--type",
      type,
      "--description",
      description,
    ];

    if (details) {
      args.push("--details", details);
    }

    const stdout = await exoExec({ cwd: options.rootPath, args });
    return { stdout };
  }

  if (options.resource === "axioms" && options.action === "add") {
    const p = options.payload as any;
    const id = typeof p?.id === "string" ? p.id : null;
    const principle = typeof p?.principle === "string" ? p.principle : null;
    const why = typeof p?.why === "string" ? p.why : null;
    const implication =
      typeof p?.implication === "string" ? p.implication : null;
    const scope = typeof p?.scope === "string" ? p.scope : "workflow";

    if (!id || !principle || !why || !implication) {
      return {
        error:
          "axioms.add requires payload { id, principle, why, implication, scope? }",
      };
    }

    const args = [
      "axiom",
      "add",
      "--id",
      id,
      "--principle",
      principle,
      "--why",
      why,
      "--implication",
      implication,
      "--scope",
      scope,
    ];
    const stdout = await exoExec({ cwd: options.rootPath, args });
    return { stdout };
  }

  if (options.resource === "axioms" && options.action === "update") {
    const p = options.payload as any;
    const id = typeof p?.id === "string" ? p.id : null;
    const principle = typeof p?.principle === "string" ? p.principle : null;
    const why = typeof p?.why === "string" ? p.why : null;
    const implication =
      typeof p?.implication === "string" ? p.implication : null;
    const scope = typeof p?.scope === "string" ? p.scope : "workflow";

    if (!id || !principle || !why || !implication) {
      return {
        error:
          "axioms.update requires payload { id, principle, why, implication, scope? }",
      };
    }

    // Remove first (ignore error if it doesn't exist? No, update implies existence usually, but let's be safe)
    try {
      await exoExec({
        cwd: options.rootPath,
        args: ["axiom", "remove", id, "--scope", scope],
      });
    } catch (e) {
      // Ignore remove error, maybe it didn't exist
    }

    const args = [
      "axiom",
      "add",
      "--id",
      id,
      "--principle",
      principle,
      "--why",
      why,
      "--implication",
      implication,
      "--scope",
      scope,
    ];
    await exoExec({ cwd: options.rootPath, args });
    return { stdout: `Updated axiom ${id}` };
  }

  return {
    error: `Edit not implemented yet for resource=${options.resource} action=${options.action}`,
  };
}
