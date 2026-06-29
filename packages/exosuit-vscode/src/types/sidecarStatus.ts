export interface SidecarStatusViewModel {
  kind: "sidecar.status.view";
  version: 1;
  binding: SidecarBindingView;
  repository: SidecarRepositoryView;
  discovery: SidecarDiscoveryView;
  actions: SidecarPaneAction[];
  diagnostics: SidecarPaneDiagnostics;
}

export interface SidecarBindingView {
  state: "linked" | "unlinked" | "unknown";
  ok: boolean | null;
  linked: boolean;
  projectId: string | null;
  policy: string | null;
  sidecarKey: string | null;
  sidecarRoot: string | null;
  autoCommit: boolean | null;
  autoPush: string | null;
  paths: {
    manifest: string | null;
    projectionDir: string | null;
    db: string | null;
    runtime: string | null;
  };
}

export interface SidecarRepositoryView {
  available: boolean;
  source: "sidecar.repo.status" | "exo.status.sidecar_sync" | "none";
  state:
    | "unavailable"
    | "clean"
    | "dirty"
    | "needs-remote"
    | "needs-push"
    | "behind"
    | "error";
  ok: boolean | null;
  sidecarRoot: string | null;
  branch: string | null;
  clean: boolean | null;
  hasRemote: boolean | null;
  remote: string | null;
  ahead: number | null;
  behind: number | null;
  issue: string | null;
  files: SidecarRepoFile[];
}

export interface SidecarRepoFile {
  path: string;
  status: string;
}

export interface SidecarDiscoveryView {
  state: "not-run" | "not-needed" | "available" | "failed";
  ok: boolean | null;
  repository: SidecarDiscoveryRepository | null;
  identity: SidecarDiscoveryIdentity | null;
  registry: SidecarDiscoveryRegistry | null;
  match: SidecarDiscoveryMatch | null;
  confidence: string | null;
  proposal: SidecarDiscoveryProposal | null;
  failure: SidecarDiscoveryFailure | null;
  checked: SidecarCheckedAttempt[];
  attemptIndex: number | null;
  sourceSummary: string | null;
}

export interface SidecarDiscoveryRepository {
  host: string;
  owner: string;
  repo: string;
  remote: string;
}

export interface SidecarDiscoveryIdentity {
  source: string;
  login: string | null;
}

export interface SidecarDiscoveryRegistry {
  source: string;
  label: string;
  profileRepo: string | null;
  path: string | null;
  version: number | null;
}

export interface SidecarDiscoveryMatch {
  kind: string;
  key: string | null;
}

export interface SidecarDiscoveryProposal {
  key: string;
  root: string | null;
  remote: string | null;
  autoPush: string | null;
  wouldMutateConfig: boolean;
  requiresRemoteAcceptance: boolean;
}

export interface SidecarDiscoveryFailure {
  classification: string;
  message: string;
  source: string | null;
}

export interface SidecarCheckedAttempt {
  attemptIndex: number;
  source: string;
  identitySource: string;
  identityLogin: string | null;
  label: string;
  profileRepo: string | null;
  path: string | null;
  status: string;
  message: string | null;
}

export interface SidecarPaneAction {
  kind:
    | "bootstrap"
    | "commit"
    | "configure-remote"
    | "push"
    | "inspect"
    | "repair";
  label: string;
  command: string;
  rationale: string | null;
  intent: string | null;
  confidence: number | null;
  source:
    | "sidecar.status.next_actions"
    | "sidecar.discovery.next_actions"
    | "derived";
}

export interface SidecarPaneDiagnostics {
  sources: SidecarPaneSourceDiagnostic[];
  limitations: string[];
}

export interface SidecarPaneSourceDiagnostic {
  id: string;
  status: "success" | "empty" | "error" | "unknown";
  message?: string;
  fetchedAt?: number;
}

export interface SidecarRepoSyncStatus {
  kind: "sidecar.repo.sync_status";
  ok: boolean;
  sidecar_root: string;
  branch: string | null;
  clean: boolean;
  has_remote: boolean;
  remote: string | null;
  ahead: number | null;
  behind: number | null;
  issue: string | null;
}
