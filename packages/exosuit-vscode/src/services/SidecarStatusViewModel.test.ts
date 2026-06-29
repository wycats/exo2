import { describe, expect, it } from "vitest";

import {
  buildSidecarStatusViewModel,
  type SidecarDiscoveryJson,
  type SidecarRepoStatusJson,
  type SidecarStatusJson,
} from "./SidecarStatusViewModel";
import type { SidecarRepoSyncStatus } from "../types/sidecarStatus";

function linkedStatus(
  overrides: Partial<SidecarStatusJson> = {},
): SidecarStatusJson {
  return {
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
    ...overrides,
  };
}

function repoStatus(
  overrides: Partial<SidecarRepoStatusJson> = {},
): SidecarRepoStatusJson {
  return {
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
    ...overrides,
  };
}

function discovery(
  overrides: Partial<SidecarDiscoveryJson> = {},
): SidecarDiscoveryJson {
  return {
    kind: "sidecar.discovery",
    ok: true,
    repository: {
      host: "github.com",
      owner: "wycats",
      repo: "locald",
      remote: "git@github.com:wycats/locald.git",
    },
    identity: {
      source: "authenticated-user",
      login: "wycats",
    },
    registry: {
      source: "github-profile",
      label: "wycats profile",
      profile_repo: "wycats/wycats",
      path: ".exosuit/sidecars.toml",
      version: 1,
    },
    match: {
      kind: "exact",
      key: "locald",
    },
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
    ...overrides,
  };
}

describe("buildSidecarStatusViewModel", () => {
  it("normalizes a linked clean repository from sidecar status and repo status", () => {
    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus(),
      sidecarRepoStatus: repoStatus(),
    });

    expect(view).toMatchObject({
      kind: "sidecar.status.view",
      version: 1,
      binding: {
        state: "linked",
        ok: true,
        linked: true,
        projectId: "proj-1",
        sidecarKey: "locald",
        sidecarRoot: "/home/me/.exo/sidecars/locald",
        autoCommit: true,
        autoPush: "if_remote",
        paths: {
          manifest: "/workspace/exosuit.toml",
          projectionDir: "/workspace/docs/agent-context",
          db: "/home/me/.exo/sidecars/locald/state.db",
          runtime: "/home/me/.exo/sidecars/locald/runtime",
        },
      },
      repository: {
        available: true,
        source: "sidecar.repo.status",
        state: "clean",
        ok: true,
        clean: true,
        hasRemote: true,
        remote: "git@github.com:wycats/locald-exosuit-state.git",
        files: [],
      },
      discovery: {
        state: "not-needed",
        ok: null,
        proposal: null,
      },
      actions: [],
    });
  });

  it("normalizes a linked dirty repository with dirty files and a commit action", () => {
    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus(),
      sidecarRepoStatus: repoStatus({
        clean: false,
        files: [
          { path: "docs/agent-context/tasks.sql", status: "M" },
          { path: "docs/agent-context/goals.sql", status: "??" },
        ],
      }),
    });

    expect(view.repository).toMatchObject({
      state: "dirty",
      ok: true,
      clean: false,
      files: [
        { path: "docs/agent-context/tasks.sql", status: "M" },
        { path: "docs/agent-context/goals.sql", status: "??" },
      ],
    });
    expect(view.actions).toContainEqual(
      expect.objectContaining({
        kind: "commit",
        command: 'exo sidecar repo commit --message "Update sidecar state"',
        source: "derived",
      }),
    );
  });

  it("normalizes no-remote repository state with a discovery proposal and remote action", () => {
    const discovered = discovery();
    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus({
        discovery: discovered,
        next_actions: [
          {
            label: "Add discovered sidecar remote",
            command:
              "exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git",
            rationale:
              "Discovery found a concrete sidecar remote for this repository.",
            intent: "execute",
            confidence: 0.9,
          },
        ],
      }),
      sidecarRepoStatus: repoStatus({
        has_remote: false,
        remote: null,
      }),
    });

    expect(view.repository).toMatchObject({
      state: "needs-remote",
      hasRemote: false,
    });
    expect(view.discovery).toMatchObject({
      state: "available",
      ok: true,
      registry: {
        source: "github-profile",
        profileRepo: "wycats/wycats",
      },
      proposal: {
        key: "locald",
        autoPush: "if_remote",
        wouldMutateConfig: true,
        requiresRemoteAcceptance: false,
      },
      checked: [
        {
          attemptIndex: 0,
          identitySource: "authenticated-user",
          identityLogin: "wycats",
        },
      ],
      attemptIndex: 0,
      sourceSummary: "wycats profile",
    });
    expect(view.actions).toContainEqual(
      expect.objectContaining({
        kind: "configure-remote",
        command:
          "exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git",
        source: "sidecar.status.next_actions",
      }),
    );
  });

  it("deduplicates repeated status and discovery actions", () => {
    const inspect = {
      label: "Inspect sidecar profile registry",
      command: "exo sidecar discover --verbose",
      rationale: "Discovery did not find a usable registry.",
      intent: "orient",
      confidence: 0.7,
    };
    const failed = discovery({
      ok: false,
      proposal: null,
      failure: {
        classification: "registry-not-found",
        message: "No sidecar registry source produced a registry",
        source: "github.com/wycats/wycats:.exosuit/sidecars.toml",
      },
      match: { kind: "none", key: null },
      next_actions: [inspect],
    });

    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus({
        discovery: failed,
        next_actions: [inspect],
      }),
      sidecarRepoStatus: repoStatus({
        has_remote: false,
        remote: null,
      }),
    });

    expect(view.actions.map((action) => action.command)).toEqual([
      "exo sidecar discover --verbose",
      "exo sidecar setup",
    ]);
  });

  it("normalizes an unlinked project with advisory discovery into a bootstrap action", () => {
    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus({
        linked: false,
        policy: "local",
        sidecar_key: null,
        sidecar_root: null,
        db_path: null,
        runtime_dir: null,
        discovery: undefined,
      }),
      discovery: discovery(),
    });

    expect(view.binding).toMatchObject({
      state: "unlinked",
      linked: false,
      policy: "local",
      sidecarKey: null,
      sidecarRoot: null,
    });
    expect(view.repository).toMatchObject({
      available: false,
      state: "unavailable",
    });
    expect(view.discovery).toMatchObject({
      state: "available",
      proposal: {
        key: "locald",
        remote: "git@github.com:wycats/locald-exosuit-state.git",
      },
    });
    expect(view.actions).toContainEqual(
      expect.objectContaining({
        kind: "bootstrap",
        command: "exo sidecar bootstrap --discover",
        source: "derived",
      }),
    );
  });

  it("distinguishes null discovery as not-run when a repository has no remote", () => {
    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus({ discovery: null }),
      sidecarRepoStatus: repoStatus({
        has_remote: false,
        remote: null,
      }),
    });

    expect(view.discovery).toMatchObject({
      state: "not-run",
      ok: null,
      proposal: null,
      checked: [],
    });
    expect(view.actions).toContainEqual(
      expect.objectContaining({
        kind: "repair",
        command: "exo sidecar setup",
        source: "derived",
      }),
    );
  });

  it("derives needs-push from primary repo status ahead counts", () => {
    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus(),
      sidecarRepoStatus: repoStatus({
        ahead: 2,
      }),
    });

    expect(view.repository).toMatchObject({
      source: "sidecar.repo.status",
      state: "needs-push",
      ok: true,
      clean: true,
      hasRemote: true,
      ahead: 2,
    });
    expect(view.actions).toContainEqual(
      expect.objectContaining({
        kind: "push",
        command: "exo sidecar repo push",
        source: "derived",
      }),
    );
  });

  it("falls back to exo status sidecar_sync when repo status is absent", () => {
    const sidecarSync: SidecarRepoSyncStatus = {
      kind: "sidecar.repo.sync_status",
      ok: false,
      sidecar_root: "/home/me/.exo/sidecars/locald",
      branch: "main",
      clean: true,
      has_remote: true,
      remote: "git@github.com:wycats/locald-exosuit-state.git",
      ahead: 2,
      behind: 0,
      issue: "sidecar repo has local commits that are not pushed",
    };

    const view = buildSidecarStatusViewModel({
      sidecarStatus: linkedStatus(),
      exoStatus: { sidecar_sync: sidecarSync },
    });

    expect(view.repository).toMatchObject({
      available: true,
      source: "exo.status.sidecar_sync",
      state: "needs-push",
      ok: false,
      ahead: 2,
      issue: "sidecar repo has local commits that are not pushed",
      files: [],
    });
    expect(view.actions).toContainEqual(
      expect.objectContaining({
        kind: "push",
        command: "exo sidecar repo push",
        source: "derived",
      }),
    );
  });

  it("preserves source diagnostics and required limitations", () => {
    const view = buildSidecarStatusViewModel({
      diagnostics: [
        {
          id: "sidecar.repo.status",
          status: "error",
          message: "daemon failed",
          fetchedAt: 1_779_000_000_000,
        },
      ],
      limitations: ["extra limitation"],
    });

    expect(view.binding).toMatchObject({ state: "unknown", ok: null });
    expect(view.repository).toMatchObject({ state: "unavailable" });
    expect(view.diagnostics.sources).toEqual([
      {
        id: "sidecar.repo.status",
        status: "error",
        message: "daemon failed",
        fetchedAt: 1_779_000_000_000,
      },
    ]);
    expect(view.diagnostics.limitations).toEqual(
      expect.arrayContaining([
        "sidecar repo status is read-only at runtime but currently classified under a write command namespace.",
        "discovery is conditional and can be null.",
        "exo status.sidecar_sync lacks dirty file details.",
        "extra limitation",
      ]),
    );
  });
});
