import type {
  SidecarBindingView,
  SidecarCheckedAttempt,
  SidecarDiscoveryFailure,
  SidecarDiscoveryIdentity,
  SidecarDiscoveryMatch,
  SidecarDiscoveryProposal,
  SidecarDiscoveryRegistry,
  SidecarDiscoveryRepository,
  SidecarDiscoveryView,
  SidecarPaneAction,
  SidecarPaneDiagnostics,
  SidecarPaneSourceDiagnostic,
  SidecarRepoFile,
  SidecarRepoSyncStatus,
  SidecarRepositoryView,
  SidecarStatusViewModel,
} from "../types/sidecarStatus";

export const SIDECAR_STATUS_VIEW_MODEL_VERSION = 1;

export const SIDECAR_STATUS_LIMITATIONS = [
  "sidecar repo status is read-only at runtime but currently classified under a write command namespace.",
  "discovery is conditional and can be null.",
  "exo status.sidecar_sync lacks dirty file details.",
] as const;

export interface SidecarStatusViewModelSources {
  sidecarStatus?: SidecarStatusJson | null;
  sidecarRepoStatus?: SidecarRepoStatusJson | null;
  exoStatus?: ExoStatusSidecarSyncJson | null;
  discovery?: SidecarDiscoveryJson | null;
  diagnostics?: readonly SidecarPaneSourceDiagnostic[];
  limitations?: readonly string[];
}

export interface ExoStatusSidecarSyncJson {
  sidecar_sync?: SidecarRepoSyncStatus | null;
}

export interface SidecarStatusJson {
  kind: "sidecar.status";
  ok: boolean;
  linked: boolean;
  project_id?: string | null;
  policy?: string | null;
  sidecar_key?: string | null;
  sidecar_root?: string | null;
  auto_commit?: boolean | null;
  auto_push?: string | null;
  manifest_path?: string | null;
  projection_dir?: string | null;
  db_path?: string | null;
  runtime_dir?: string | null;
  discovery?: SidecarDiscoveryJson | null;
  next_actions?: readonly SuggestedActionJson[];
}

export interface SidecarRepoStatusJson {
  kind: "sidecar.repo.status";
  ok: boolean;
  sidecar_root: string;
  branch?: string | null;
  clean: boolean;
  has_remote: boolean;
  remote?: string | null;
  ahead?: number | null;
  behind?: number | null;
  files?: readonly SidecarRepoFileStatusJson[];
}

export interface SidecarRepoFileStatusJson {
  path: string;
  status: string;
}

export interface SidecarDiscoveryJson {
  kind: "sidecar.discovery";
  ok: boolean;
  repository?: SidecarDiscoveryRepositoryJson | null;
  identity: SidecarDiscoveryIdentityJson;
  registry: SidecarDiscoveryRegistryJson;
  match: SidecarDiscoveryMatchJson;
  confidence: string;
  proposal?: SidecarDiscoveryProposalJson | null;
  failure?: SidecarDiscoveryFailureJson | null;
  checked?: readonly SidecarDiscoveryCheckedAttemptJson[];
  attempt_index?: number | null;
  source_summary?: string | null;
  next_actions?: readonly SuggestedActionJson[];
}

export interface SidecarDiscoveryRepositoryJson {
  host: string;
  owner: string;
  repo: string;
  remote: string;
}

export interface SidecarDiscoveryIdentityJson {
  source: string;
  login?: string | null;
}

export interface SidecarDiscoveryRegistryJson {
  source: string;
  label: string;
  profile_repo?: string | null;
  path?: string | null;
  version?: number | null;
}

export interface SidecarDiscoveryMatchJson {
  kind: string;
  key?: string | null;
}

export interface SidecarDiscoveryProposalJson {
  key: string;
  root?: string | null;
  remote?: string | null;
  auto_push?: string | null;
  would_mutate_config: boolean;
  requires_remote_acceptance: boolean;
}

export interface SidecarDiscoveryFailureJson {
  classification: string;
  message: string;
  source?: string | null;
}

export interface SidecarDiscoveryCheckedAttemptJson {
  attempt_index: number;
  source: string;
  identity_source: string;
  identity_login?: string | null;
  label: string;
  profile_repo?: string | null;
  path?: string | null;
  status: string;
  message?: string | null;
}

export interface SuggestedActionJson {
  label: string;
  command: string;
  rationale?: string | null;
  intent?: string | null;
  confidence?: number | null;
}

export function buildSidecarStatusViewModel(
  sources: SidecarStatusViewModelSources,
): SidecarStatusViewModel {
  const binding = normalizeBinding(sources.sidecarStatus ?? null);
  const repository = normalizeRepository(sources);
  const discovery = normalizeDiscovery(
    selectDiscoverySource(sources),
    repository.hasRemote === true,
  );
  const actions = normalizeActions(sources, binding, repository, discovery);

  return {
    kind: "sidecar.status.view",
    version: SIDECAR_STATUS_VIEW_MODEL_VERSION,
    binding,
    repository,
    discovery,
    actions,
    diagnostics: normalizeDiagnostics(sources),
  };
}

function normalizeBinding(
  status: SidecarStatusJson | null,
): SidecarBindingView {
  if (!status) {
    return {
      state: "unknown",
      ok: null,
      linked: false,
      projectId: null,
      policy: null,
      sidecarKey: null,
      sidecarRoot: null,
      autoCommit: null,
      autoPush: null,
      paths: {
        manifest: null,
        projectionDir: null,
        db: null,
        runtime: null,
      },
    };
  }

  return {
    state: status.linked ? "linked" : "unlinked",
    ok: status.ok,
    linked: status.linked,
    projectId: status.project_id ?? null,
    policy: status.policy ?? null,
    sidecarKey: status.sidecar_key ?? null,
    sidecarRoot: status.sidecar_root ?? null,
    autoCommit: status.auto_commit ?? null,
    autoPush: status.auto_push ?? null,
    paths: {
      manifest: status.manifest_path ?? null,
      projectionDir: status.projection_dir ?? null,
      db: status.db_path ?? null,
      runtime: status.runtime_dir ?? null,
    },
  };
}

function normalizeRepository(
  sources: SidecarStatusViewModelSources,
): SidecarRepositoryView {
  const repo = sources.sidecarRepoStatus;
  if (repo) {
    const ok = repo.ok;
    const files = (repo.files ?? []).map(normalizeRepoFile);
    return {
      available: true,
      source: "sidecar.repo.status",
      state: repositoryState(
        ok,
        repo.clean,
        repo.has_remote,
        repo.ahead ?? null,
        repo.behind ?? null,
      ),
      ok,
      sidecarRoot: repo.sidecar_root,
      branch: repo.branch ?? null,
      clean: repo.clean,
      hasRemote: repo.has_remote,
      remote: repo.remote ?? null,
      ahead: repo.ahead ?? null,
      behind: repo.behind ?? null,
      issue: null,
      files,
    };
  }

  const sync = sources.exoStatus?.sidecar_sync;
  if (sync) {
    return {
      available: true,
      source: "exo.status.sidecar_sync",
      state: repositoryState(
        sync.ok,
        sync.clean,
        sync.has_remote,
        sync.ahead,
        sync.behind,
      ),
      ok: sync.ok,
      sidecarRoot: sync.sidecar_root,
      branch: sync.branch,
      clean: sync.clean,
      hasRemote: sync.has_remote,
      remote: sync.remote,
      ahead: sync.ahead,
      behind: sync.behind,
      issue: sync.issue,
      files: [],
    };
  }

  return {
    available: false,
    source: "none",
    state: "unavailable",
    ok: null,
    sidecarRoot: null,
    branch: null,
    clean: null,
    hasRemote: null,
    remote: null,
    ahead: null,
    behind: null,
    issue: null,
    files: [],
  };
}

function repositoryState(
  ok: boolean | null,
  clean: boolean | null,
  hasRemote: boolean | null,
  ahead: number | null,
  behind: number | null,
): SidecarRepositoryView["state"] {
  if (clean === null && hasRemote === null && ok === null) {
    return "unavailable";
  }
  if (clean === false) {
    return "dirty";
  }
  if (hasRemote === false) {
    return "needs-remote";
  }
  if ((behind ?? 0) > 0) {
    return "behind";
  }
  if (
    clean === true &&
    hasRemote === true &&
    ((ahead ?? 0) > 0 || ok === false)
  ) {
    return "needs-push";
  }
  if (ok === true || (clean === true && hasRemote === true)) {
    return "clean";
  }
  return "error";
}

function normalizeRepoFile(file: SidecarRepoFileStatusJson): SidecarRepoFile {
  return {
    path: file.path,
    status: file.status,
  };
}

function selectDiscoverySource(
  sources: SidecarStatusViewModelSources,
): SidecarDiscoveryJson | null {
  if (sources.sidecarStatus?.discovery) {
    return sources.sidecarStatus.discovery;
  }
  return sources.discovery ?? null;
}

function normalizeDiscovery(
  discovery: SidecarDiscoveryJson | null,
  repositoryHasRemote: boolean,
): SidecarDiscoveryView {
  if (!discovery) {
    return {
      state: repositoryHasRemote ? "not-needed" : "not-run",
      ok: null,
      repository: null,
      identity: null,
      registry: null,
      match: null,
      confidence: null,
      proposal: null,
      failure: null,
      checked: [],
      attemptIndex: null,
      sourceSummary: null,
    };
  }

  return {
    state: discovery.ok ? "available" : "failed",
    ok: discovery.ok,
    repository: discovery.repository
      ? normalizeDiscoveryRepository(discovery.repository)
      : null,
    identity: normalizeDiscoveryIdentity(discovery.identity),
    registry: normalizeDiscoveryRegistry(discovery.registry),
    match: normalizeDiscoveryMatch(discovery.match),
    confidence: discovery.confidence,
    proposal: discovery.proposal
      ? normalizeDiscoveryProposal(discovery.proposal)
      : null,
    failure: discovery.failure
      ? normalizeDiscoveryFailure(discovery.failure)
      : null,
    checked: (discovery.checked ?? []).map(normalizeCheckedAttempt),
    attemptIndex: discovery.attempt_index ?? null,
    sourceSummary: discovery.source_summary ?? null,
  };
}

function normalizeDiscoveryRepository(
  repository: SidecarDiscoveryRepositoryJson,
): SidecarDiscoveryRepository {
  return {
    host: repository.host,
    owner: repository.owner,
    repo: repository.repo,
    remote: repository.remote,
  };
}

function normalizeDiscoveryIdentity(
  identity: SidecarDiscoveryIdentityJson,
): SidecarDiscoveryIdentity {
  return {
    source: identity.source,
    login: identity.login ?? null,
  };
}

function normalizeDiscoveryRegistry(
  registry: SidecarDiscoveryRegistryJson,
): SidecarDiscoveryRegistry {
  return {
    source: registry.source,
    label: registry.label,
    profileRepo: registry.profile_repo ?? null,
    path: registry.path ?? null,
    version: registry.version ?? null,
  };
}

function normalizeDiscoveryMatch(
  match: SidecarDiscoveryMatchJson,
): SidecarDiscoveryMatch {
  return {
    kind: match.kind,
    key: match.key ?? null,
  };
}

function normalizeDiscoveryProposal(
  proposal: SidecarDiscoveryProposalJson,
): SidecarDiscoveryProposal {
  return {
    key: proposal.key,
    root: proposal.root ?? null,
    remote: proposal.remote ?? null,
    autoPush: proposal.auto_push ?? null,
    wouldMutateConfig: proposal.would_mutate_config,
    requiresRemoteAcceptance: proposal.requires_remote_acceptance,
  };
}

function normalizeDiscoveryFailure(
  failure: SidecarDiscoveryFailureJson,
): SidecarDiscoveryFailure {
  return {
    classification: failure.classification,
    message: failure.message,
    source: failure.source ?? null,
  };
}

function normalizeCheckedAttempt(
  attempt: SidecarDiscoveryCheckedAttemptJson,
): SidecarCheckedAttempt {
  return {
    attemptIndex: attempt.attempt_index,
    source: attempt.source,
    identitySource: attempt.identity_source,
    identityLogin: attempt.identity_login ?? null,
    label: attempt.label,
    profileRepo: attempt.profile_repo ?? null,
    path: attempt.path ?? null,
    status: attempt.status,
    message: attempt.message ?? null,
  };
}

function normalizeActions(
  sources: SidecarStatusViewModelSources,
  binding: SidecarBindingView,
  repository: SidecarRepositoryView,
  discovery: SidecarDiscoveryView,
): SidecarPaneAction[] {
  const actions: SidecarPaneAction[] = [];
  const rawDiscovery = selectDiscoverySource(sources);

  for (const action of sources.sidecarStatus?.next_actions ?? []) {
    actions.push(normalizeAction(action, "sidecar.status.next_actions"));
  }
  for (const action of rawDiscovery?.next_actions ?? []) {
    actions.push(normalizeAction(action, "sidecar.discovery.next_actions"));
  }

  addDerivedActions(actions, binding, repository, discovery);
  return dedupeActions(actions);
}

function dedupeActions(actions: SidecarPaneAction[]): SidecarPaneAction[] {
  const seen = new Set<string>();
  return actions.filter((action) => {
    const key = `${action.kind}\u0000${action.command}`;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

function normalizeAction(
  action: SuggestedActionJson,
  source: SidecarPaneAction["source"],
): SidecarPaneAction {
  return {
    kind: actionKind(action.command),
    label: action.label,
    command: action.command,
    rationale: action.rationale ?? null,
    intent: action.intent ?? null,
    confidence: action.confidence ?? null,
    source,
  };
}

function actionKind(command: string): SidecarPaneAction["kind"] {
  if (/\bbootstrap\b/.test(command)) {
    return "bootstrap";
  }
  if (/\brepo\s+commit\b/.test(command)) {
    return "commit";
  }
  if (/\brepo\s+remote\b/.test(command)) {
    return "configure-remote";
  }
  if (/\brepo\s+push\b/.test(command)) {
    return "push";
  }
  if (/\bsidecar\s+setup\b/.test(command)) {
    return "repair";
  }
  if (/\b(status|discover)\b/.test(command)) {
    return "inspect";
  }
  return "repair";
}

function addDerivedActions(
  actions: SidecarPaneAction[],
  binding: SidecarBindingView,
  repository: SidecarRepositoryView,
  discovery: SidecarDiscoveryView,
): void {
  if (binding.state === "unlinked" && discovery.proposal) {
    addActionIfMissing(actions, {
      kind: "bootstrap",
      label: "Bootstrap discovered sidecar",
      command: "exo sidecar bootstrap --discover",
      rationale: "Discovery found sidecar configuration for this repository.",
      intent: "execute",
      confidence: 0.85,
      source: "derived",
    });
  }

  if (repository.state === "dirty") {
    addActionIfMissing(actions, {
      kind: "commit",
      label: "Commit sidecar changes",
      command: 'exo sidecar repo commit --message "Update sidecar state"',
      rationale: "The sidecar repository has uncommitted changes.",
      intent: "execute",
      confidence: 0.8,
      source: "derived",
    });
  }

  if (repository.state === "needs-remote") {
    const remote = discovery.proposal?.remote;
    addActionIfMissing(actions, {
      kind: remote ? "configure-remote" : "repair",
      label: remote ? "Add discovered sidecar remote" : "Set up sidecar remote",
      command: remote
        ? `exo sidecar repo remote --url ${remote}`
        : "exo sidecar setup",
      rationale: remote
        ? "Discovery found a concrete sidecar remote for this repository."
        : "Create missing GitHub sidecar setup and configure the sidecar repository remote.",
      intent: "execute",
      confidence: remote ? 0.9 : 0.75,
      source: "derived",
    });
  }

  if (repository.state === "needs-push") {
    addActionIfMissing(actions, {
      kind: "push",
      label: "Push sidecar changes",
      command: "exo sidecar repo push",
      rationale:
        "The sidecar repository is clean locally but not synced to its remote.",
      intent: "execute",
      confidence: 0.75,
      source: "derived",
    });
  }

  if (discovery.state === "failed") {
    addActionIfMissing(actions, {
      kind: "inspect",
      label: "Inspect sidecar discovery",
      command: "exo sidecar discover --format json",
      rationale: discovery.failure?.message ?? "Sidecar discovery failed.",
      intent: "orient",
      confidence: 0.7,
      source: "derived",
    });
  }
}

function addActionIfMissing(
  actions: SidecarPaneAction[],
  action: SidecarPaneAction,
): void {
  if (actions.some((existing) => existing.kind === action.kind)) {
    return;
  }
  actions.push(action);
}

function normalizeDiagnostics(
  sources: SidecarStatusViewModelSources,
): SidecarPaneDiagnostics {
  return {
    sources: [...(sources.diagnostics ?? [])],
    limitations: [
      ...SIDECAR_STATUS_LIMITATIONS,
      ...(sources.limitations ?? []),
    ],
  };
}
