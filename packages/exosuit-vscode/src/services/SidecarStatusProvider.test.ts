import { describe, expect, it } from "vitest";

import { renderSidecarStatus } from "../SidecarStatusProvider";
import type { TraceCacheRootDiagnostic } from "./TraceCache";

function roots(values: Record<string, unknown>): ReadonlyMap<string, unknown> {
  return new Map(Object.entries(values));
}

function diagnostics(
  values: Record<string, TraceCacheRootDiagnostic | undefined> = {},
): ReadonlyMap<string, TraceCacheRootDiagnostic | undefined> {
  return new Map(Object.entries(values));
}

describe("renderSidecarStatus", () => {
  it("renders the basic sidecar pane from structured roots", () => {
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: true,
          project_id: "proj-1",
          policy: "sidecar",
          sidecar_key: "locald",
          sidecar_root: "/home/me/.exo/sidecars/locald",
          auto_commit: true,
          auto_push: "if_remote",
          manifest_path: "/workspace/exosuit.toml",
          projection_dir: "/workspace/docs/agent-context",
          db_path: "/home/me/.exo/sidecars/locald/state.db",
          runtime_dir: "/home/me/.exo/sidecars/locald/runtime",
          discovery: null,
          next_actions: [],
        },
        "sidecar-repo-status": {
          kind: "sidecar.repo.status",
          ok: true,
          sidecar_root: "/home/me/.exo/sidecars/locald",
          branch: "main",
          clean: true,
          has_remote: true,
          remote: "git@github.com:wycats/locald-exosuit-state.git",
          ahead: 0,
          behind: 0,
          files: [],
        },
        status: {},
      }),
      diagnostics(),
    );

    expect(items.map((item) => item.id)).toEqual([
      "sidecar-status-header",
      "sidecar-status-binding",
      "sidecar-status-repository",
    ]);
    expect(items[0]?.label).toBe("Sidecar Status");
    expect(items[0]?.description).toBe("linked • clean");
    expect(items[1]?.label).toBe("Linked sidecar");
    expect(items[1]?.description).toBe("locald");
    expect(items[2]?.label).toBe("Repository");
    expect(items[2]?.description).toBe("clean");
  });

  it("renders derived actions with sidecar action command wiring", () => {
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: true,
          project_id: "proj-1",
          policy: "sidecar",
          sidecar_key: "locald",
          sidecar_root: "/home/me/.exo/sidecars/locald",
          auto_commit: true,
          auto_push: "if_remote",
          discovery: null,
          next_actions: [],
        },
        "sidecar-repo-status": {
          kind: "sidecar.repo.status",
          ok: true,
          sidecar_root: "/home/me/.exo/sidecars/locald",
          branch: "main",
          clean: false,
          has_remote: true,
          remote: "git@github.com:wycats/locald-exosuit-state.git",
          ahead: 0,
          behind: 0,
          files: [{ path: "docs/agent-context/tasks.sql", status: "M" }],
        },
        status: {},
      }),
      diagnostics(),
    );

    const actions = items.find((item) => item.id === "sidecar-status-actions");
    expect(actions?.label).toBe("Actions (1)");
    expect(actions?.children).toHaveLength(1);
    expect(actions?.children[0]?.label).toBe("Commit sidecar changes");
    expect(actions?.children[0]?.command).toMatchObject({
      command: "exosuit.sidecar.runAction",
      title: "Commit sidecar changes",
      arguments: [
        expect.objectContaining({
          kind: "commit",
          command: 'exo sidecar repo commit --message "Update sidecar state"',
        }),
      ],
    });
    expect(actions?.children[0]?.tooltip).toContain("exo sidecar repo commit");
  });

  it("deduplicates repeated inspect actions in the pane", () => {
    const inspect = {
      label: "Inspect sidecar profile registry",
      command: "exo sidecar discover --verbose",
      rationale: "Discovery did not find a usable registry.",
      intent: "orient",
      confidence: 0.7,
    };
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: true,
          project_id: "proj-1",
          policy: "sidecar",
          sidecar_key: "exo2",
          sidecar_root: "/var/home/me/exo2-sidecar",
          auto_commit: true,
          auto_push: "if_remote",
          discovery: {
            kind: "sidecar.discovery",
            ok: false,
            repository: null,
            identity: { source: "authenticated-user", login: "wycats" },
            registry: {
              source: "github-profile",
              label: "github-profile:.exosuit/sidecars.toml",
              profile_repo: "github.com/wycats/wycats",
              path: ".exosuit/sidecars.toml",
              version: null,
            },
            match: { kind: "none", key: null },
            confidence: "none",
            proposal: null,
            failure: {
              classification: "registry-not-found",
              message: "No sidecar registry source produced a registry",
              source: "github.com/wycats/wycats:.exosuit/sidecars.toml",
            },
            checked: [],
            attempt_index: null,
            source_summary:
              "github-profile:.exosuit/sidecars.toml did not produce a usable sidecar discovery",
            next_actions: [inspect],
          },
          next_actions: [inspect],
        },
        "sidecar-repo-status": {
          kind: "sidecar.repo.status",
          ok: true,
          sidecar_root: "/var/home/me/exo2-sidecar",
          branch: "main",
          clean: true,
          has_remote: false,
          remote: null,
          ahead: null,
          behind: null,
          files: [],
        },
        status: {},
      }),
      diagnostics(),
    );

    const actions = items.find((item) => item.id === "sidecar-status-actions");
    expect(actions?.label).toBe("Actions (2)");
    expect(actions?.children.map((child) => child.label)).toEqual([
      "Inspect sidecar profile registry",
      "Set up sidecar remote",
    ]);
  });

  it("renders linked sidecar details as expandable children", () => {
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: true,
          project_id: "proj-1",
          policy: "sidecar",
          sidecar_key: "locald",
          sidecar_root: "/home/me/.exo/sidecars/locald",
          auto_commit: true,
          auto_push: "if_remote",
          manifest_path: "/workspace/exosuit.toml",
          projection_dir: "/workspace/docs/agent-context",
          db_path: "/home/me/.exo/sidecars/locald/state.db",
          runtime_dir: "/home/me/.exo/sidecars/locald/runtime",
          discovery: null,
          next_actions: [],
        },
        "sidecar-repo-status": {
          kind: "sidecar.repo.status",
          ok: true,
          sidecar_root: "/home/me/.exo/sidecars/locald",
          branch: "main",
          clean: true,
          has_remote: true,
          remote: "git@github.com:wycats/locald-exosuit-state.git",
          ahead: 0,
          behind: 0,
          files: [],
        },
        status: {},
      }),
      diagnostics(),
    );

    const binding = items.find((item) => item.id === "sidecar-status-binding");
    expect(binding?.label).toBe("Linked sidecar");
    expect(binding?.description).toBe("locald");
    expect(binding?.collapsibleState).toBe(2);
    expect(
      binding?.children.map((child) => [child.label, child.description]),
    ).toEqual([
      ["Policy", "sidecar"],
      ["Root", "/home/me/.exo/sidecars/locald"],
      ["Auto persist", "commit on • push if_remote"],
      ["Projection", "/workspace/docs/agent-context"],
    ]);
  });

  it("renders unlinked sidecar state with bootstrap guidance", () => {
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: false,
          project_id: "proj-1",
          policy: "local",
          sidecar_key: null,
          sidecar_root: null,
          auto_commit: false,
          auto_push: "never",
          manifest_path: "/workspace/exosuit.toml",
          projection_dir: null,
          db_path: null,
          runtime_dir: null,
          discovery: null,
          next_actions: [],
        },
        status: {},
      }),
      diagnostics(),
    );

    const header = items.find((item) => item.id === "sidecar-status-header");
    const binding = items.find((item) => item.id === "sidecar-status-binding");
    expect(header?.description).toBe("unlinked • unavailable");
    expect(binding?.label).toBe("Sidecar not linked");
    expect(binding?.description).toBe("local state");
    expect(binding?.collapsibleState).toBe(2);
    expect(
      binding?.children.map((child) => [child.label, child.description]),
    ).toEqual([
      ["Policy", "local"],
      ["Manifest", "/workspace/exosuit.toml"],
      ["Next", "bootstrap sidecar state"],
    ]);
  });

  it("renders unknown binding as a loading or unavailable source state", () => {
    const items = renderSidecarStatus(roots({}), diagnostics());

    const binding = items.find((item) => item.id === "sidecar-status-binding");
    expect(binding?.label).toBe("Sidecar status unknown");
    expect(binding?.description).toBe("waiting for status source");
    expect(binding?.collapsibleState).toBe(0);
  });

  it("renders repository sync details including remote, ahead behind, and dirty files", () => {
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: true,
          project_id: "proj-1",
          policy: "sidecar",
          sidecar_key: "locald",
          sidecar_root: "/home/me/.exo/sidecars/locald",
          auto_commit: true,
          auto_push: "if_remote",
          discovery: null,
          next_actions: [],
        },
        "sidecar-repo-status": {
          kind: "sidecar.repo.status",
          ok: true,
          sidecar_root: "/home/me/.exo/sidecars/locald",
          branch: "main",
          clean: false,
          has_remote: true,
          remote: "git@github.com:wycats/locald-exosuit-state.git",
          ahead: 2,
          behind: 1,
          files: [
            { path: "docs/agent-context/tasks.sql", status: "M" },
            { path: "docs/agent-context/goals.sql", status: "??" },
          ],
        },
        status: {},
      }),
      diagnostics(),
    );

    const repository = items.find(
      (item) => item.id === "sidecar-status-repository",
    );
    expect(repository?.collapsibleState).toBe(2);
    expect(
      repository?.children.map((child) => [child.label, child.description]),
    ).toEqual([
      ["Branch", "main"],
      ["Remote", "git@github.com:wycats/locald-exosuit-state.git"],
      ["Sync", "ahead 2 • behind 1"],
      ["Changed files", "2"],
      ["M docs/agent-context/tasks.sql", undefined],
      ["?? docs/agent-context/goals.sql", undefined],
    ]);
  });

  it("renders available discovery details with proposal and checked attempts", () => {
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: true,
          project_id: "proj-1",
          policy: "sidecar",
          sidecar_key: "locald",
          sidecar_root: "/home/me/.exo/sidecars/locald",
          auto_commit: true,
          auto_push: "if_remote",
          discovery: {
            kind: "sidecar.discovery",
            ok: true,
            repository: {
              host: "github.com",
              owner: "wycats",
              repo: "locald",
              remote: "git@github.com:wycats/locald.git",
            },
            identity: { source: "authenticated-user", login: "wycats" },
            registry: {
              source: "github-profile",
              label: "wycats profile",
              profile_repo: "wycats/wycats",
              path: ".exosuit/sidecars.toml",
              version: 1,
            },
            match: { kind: "exact", key: "locald" },
            confidence: "high",
            proposal: {
              key: "locald",
              root: "~/.exo/sidecars/locald",
              remote: "git@github.com:wycats/locald-exosuit-state.git",
              auto_push: "if_remote",
              would_mutate_config: true,
              requires_remote_acceptance: false,
            },
            failure: null,
            checked: [
              {
                attempt_index: 0,
                source: "github-profile",
                identity_source: "authenticated-user",
                identity_login: "wycats",
                label: "wycats profile",
                profile_repo: "wycats/wycats",
                path: ".exosuit/sidecars.toml",
                status: "fetched",
                message: null,
              },
            ],
            attempt_index: 0,
            source_summary: "wycats profile",
            next_actions: [],
          },
          next_actions: [],
        },
        "sidecar-repo-status": {
          kind: "sidecar.repo.status",
          ok: true,
          sidecar_root: "/home/me/.exo/sidecars/locald",
          branch: "main",
          clean: true,
          has_remote: false,
          remote: null,
          ahead: null,
          behind: null,
          files: [],
        },
        status: {},
      }),
      diagnostics(),
    );

    const discovery = items.find(
      (item) => item.id === "sidecar-status-discovery",
    );
    expect(discovery?.label).toBe("Discovery");
    expect(discovery?.description).toBe("available");
    expect(discovery?.collapsibleState).toBe(2);
    expect(
      discovery?.children.map((child) => [child.label, child.description]),
    ).toEqual([
      ["Source", "wycats profile"],
      ["Registry", "wycats profile"],
      ["Profile", "wycats/wycats"],
      ["Match", "exact locald"],
      ["Proposal", "locald"],
      ["Root", "~/.exo/sidecars/locald"],
      ["Remote", "git@github.com:wycats/locald-exosuit-state.git"],
      ["Checked attempts", "1"],
      ["0 github-profile", "fetched"],
    ]);
  });

  it("renders failed discovery details with failure summary", () => {
    const items = renderSidecarStatus(
      roots({
        "sidecar-status": {
          kind: "sidecar.status",
          ok: true,
          linked: true,
          project_id: "proj-1",
          policy: "sidecar",
          sidecar_key: "locald",
          sidecar_root: "/home/me/.exo/sidecars/locald",
          auto_commit: true,
          auto_push: "if_remote",
          discovery: {
            kind: "sidecar.discovery",
            ok: false,
            repository: null,
            identity: { source: "authenticated-user", login: "wycats" },
            registry: {
              source: "github-profile",
              label: "wycats profile",
              profile_repo: "wycats/wycats",
              path: ".exosuit/sidecars.toml",
              version: 1,
            },
            match: { kind: "none", key: null },
            confidence: "low",
            proposal: null,
            failure: {
              classification: "no-match",
              message: "No sidecar registry entry matched this repository.",
              source: "wycats profile",
            },
            checked: [],
            attempt_index: null,
            source_summary: "wycats profile",
            next_actions: [],
          },
          next_actions: [],
        },
        status: {},
      }),
      diagnostics(),
    );

    const discovery = items.find(
      (item) => item.id === "sidecar-status-discovery",
    );
    expect(discovery?.description).toBe("failed");
    expect(
      discovery?.children.map((child) => [child.label, child.description]),
    ).toEqual([
      ["Source", "wycats profile"],
      ["Registry", "wycats profile"],
      ["Profile", "wycats/wycats"],
      ["Match", "none"],
      ["Failure", "no-match"],
      ["Message", "No sidecar registry entry matched this repository."],
    ]);
  });

  it("renders source diagnostics when roots fail", () => {
    const items = renderSidecarStatus(
      roots({}),
      diagnostics({
        "sidecar-repo-status": {
          rootId: "sidecar-repo-status",
          namespace: "sidecar",
          operation: "repo",
          status: "error",
          input: { action: "status" },
          explicitInput: true,
          fetchedAt: 1_779_000_000_000,
          error: { message: "daemon failed" },
        },
      }),
    );

    expect(items[0]?.description).toBe("unknown • unavailable");
    const diagnosticsItem = items.find(
      (item) => item.id === "sidecar-status-diagnostics",
    );
    expect(diagnosticsItem?.label).toBe("Diagnostics (1)");
    expect(diagnosticsItem?.children[0]?.label).toBe("sidecar-repo-status");
    expect(diagnosticsItem?.children[0]?.tooltip).toBe("daemon failed");
  });
});
