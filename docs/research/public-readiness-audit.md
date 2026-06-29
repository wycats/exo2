# Repository Public-Readiness Audit

This checklist defines the minimum work needed before making the repository
public. The target is not a polished launch. The target is a repo that can be
public without leaking private information, credentials, machine-local state, or
accidentally expensive CI behavior.

The audit is broader than secret scanning. It must also identify context that
was acceptable only because the repository was private: handoffs, agent notes,
personal operational details, local filesystem paths, account-specific setup,
private sidecar state, and research notes that quote or summarize private work.

## Status

This is a requirements and audit checklist. It does not claim that the repo has
been scrubbed yet.

Exo tracking:

- Goal: `prepare-repository-public-readiness-audit`
- First task: `create-public-readiness-checklist-artifact`
- Recon task: `recon-public-readiness-scrub-candidates`

## Public-Readiness Bar

Before the repository is made public:

- No secrets, credentials, private keys, access tokens, cookies, or signed
  capability tickets are present in the current tree or retained in intended
  public history.
- No machine-local Exo state, sidecar cache, shadow state, daemon runtime file,
  or private projection is committed as source material.
- Private-path references are either removed, generalized, or intentionally
  retained only when they are harmless examples.
- Private repo, account, billing, and infrastructure references are reviewed
  and classified as safe public context or scrub-required.
- Highly private planning, handoff, agent, and research context is reviewed as
  private content even when it contains no credential material.
- CI defaults are safe for a public repo and do not trigger avoidable paid work
  on every push or untrusted fork.
- The public repo has enough basic project metadata for external readers:
  license, readme, contribution/security posture, and supported setup path.

## Audit Checklist

| Area | Requirement | Suggested Evidence | Public-Ready Criteria | Status |
| --- | --- | --- | --- | --- |
| Current-tree secret scan | Scan tracked and intended-to-track files for high-confidence secrets. | `gitleaks detect --no-git` or equivalent; targeted `rg` for token/key patterns. | No live secrets. False positives documented or removed. | Pending |
| Git-history secret scan | Scan the public history range, not just the working tree. | `gitleaks detect` or `trufflehog git file://$PWD` over the intended public branch. | No live secrets in retained public history. Any required history rewrite is decided before visibility changes. | Pending |
| Gitignored local state | Confirm ignored runtime/cache files are not staged and cannot be accidentally added. | `git status --ignored --short`; inspect `.gitignore` and generated state locations. | Sidecar cache, daemon runtime, target artifacts, logs, local env files, and editor state are ignored. | Pending |
| Exo sidecar/shadow boundaries | Verify Exo operational state remains outside repo source unless intentionally projected. | `exo project resolve`; `exo sidecar status`; inspect `docs/agent-context` policy. | Repo policy projections are generated SQL only; sidecar/shadow state is not treated as human-authored public docs. | Pending |
| Generated projections | Review committed generated files for private data before publishing. | Inspect `docs/agent-context/**/*.sql` and generated docs. | Generated projections contain only intended portable project state. Private workspace roots, local usernames, and transient agent events are absent or intentionally handled. | Pending |
| Private path references | Find absolute paths and machine-specific references. | Targeted `rg` for `/Users/`, `/home/`, `/private/`, `/Volumes/`, Windows drive roots, mounted Mac paths, and home/profile variables. | Harmful paths removed or generalized. Harmless historical examples are explicitly accepted. | Pending |
| Private repo/account references | Find references to private repos, local sidecar roots, internal services, and account-specific infrastructure. | Targeted `rg` for GitHub/Vercel URLs, sidecar roots, private/billing/action references. | Each reference is either safe public context, generalized, or scrubbed. | Pending |
| Private context review | Identify private planning, handoff, agent, and research context that should not become public merely because it contains no secrets. | Review `.context/`, `docs/research/`, `docs/agent-context/`, inbox exports, handoffs, and generated projections for private work context. | Private-context material is removed, generalized, moved out of public history, or explicitly accepted for publication. | Pending |
| Personal data and logs | Check research notes, handoffs, inbox exports, logs, and transcripts for user-private content. | Review `.context/`, `docs/research/`, `docs/agent-context/`, and any committed logs. | No private conversation excerpts or personal operational details unless intentionally public. | Pending |
| License and legal baseline | Confirm project license and third-party dependency posture are acceptable for public release. | Inspect `LICENSE*`, package metadata, Cargo manifests, npm manifests. | License exists and matches intended distribution. No obvious incompatible vendored material. | Pending |
| README and setup path | Ensure external readers can identify what the repo is and how to run basic validation. | Inspect `README*`, `AGENTS.md`, setup docs, scripts. | README does not need to be launch-polished, but should not be misleading or private-only. | Pending |
| Security policy | Decide whether to add a minimal security reporting file before public visibility. | Check `.github/SECURITY.md` or `SECURITY.md`. | Public vulnerability/reporting expectations are explicit enough for external readers. | Pending |
| CI cost posture | Ensure public visibility does not make every external event run expensive workflows by default. | Inspect `.github/workflows/*.yml`, triggers, fork guards, label gates, concurrency, permissions. | Expensive checks are label/manual/scheduled as intended; fork PRs do not receive write tokens; permissions are least-privilege. | Pending |
| CI reproducibility | Confirm public CI can run without private secrets. | Inspect workflow `secrets.*`, external services, deploy/comment steps. | Required secrets are optional, guarded, or documented; public PRs fail gracefully when secrets are unavailable. | Pending |
| Package publishing metadata | Review crate/npm/extension metadata for public-facing names, URLs, and accidental private registry assumptions. | Inspect `Cargo.toml`, `package.json`, extension manifests, release scripts. | Metadata is either ready for public readers or clearly non-published/internal. | Pending |
| GitHub repository settings | Identify settings that must be changed with visibility. | Manual GitHub settings review: Actions, branch protections, fork policy, secrets, environments, pages/deployments. | Settings plan exists before visibility flips. | Pending |
| Public-history strategy | Decide whether to publish existing history, a cleaned branch, or a squashed/imported history. | Compare secret/history scan result with repo narrative value. | The visibility plan names the exact branch/history shape to publish. | Pending |

## Immediate Audit Commands

These commands are a starting point. They should be run from the intended
public branch or a fresh public-readiness branch.

```sh
git status --short --ignored
git ls-files
git grep -n -I -E 'BEGIN (RSA|OPENSSH|EC|PRIVATE) KEY|ghp_|github_pat_|sk-[A-Za-z0-9]|xox[baprs]-|AKIA[0-9A-Z]{16}'
rg -n -i 'secret|token|credential|password|cookie|private key|api key|billing|wallet|private repo'
rg -n '/Users/|/home/|/private/|/Volumes/|C:\\|V:\\|//Mac/|USERPROFILE|HOME'
find .github/workflows -type f -maxdepth 1 -print
```

If available, use a dedicated secret scanner over both the current tree and
history:

```sh
gitleaks detect --no-git
gitleaks detect
```

## CI Cost Requirements

The public visibility change should not rely on remembering to avoid expensive
checks manually. Workflows should encode the desired behavior.

Requirements:

- Normal PR/test gates should remain fast enough to run routinely.
- Coverage/audit-style expensive checks should be manual, scheduled, or
  label-gated.
- Fork PRs must not run untrusted code with repository write permissions.
- Workflow permissions should be job-scoped and least-privilege.
- Deploy/comment workflows should handle missing secrets safely.
- Concurrency should cancel superseded runs where cancellation is safe.

## Public Visibility Decision Points

The audit should produce explicit decisions for:

- whether to publish current history or a cleaned branch;
- whether committed research and RFC history are acceptable public context;
- whether `.context/` material is source, migration residue, or scrub-required;
- whether private planning context should be removed from history or preserved
  in a private archive before public publication;
- whether `docs/agent-context` projections should remain committed;
- whether CI coverage remains non-blocking after public visibility;
- whether README/license/security docs need a small public-facing pass before
  flipping visibility.

## Initial Output Expected

The first public-readiness pass should produce:

- a classified findings table: `must_fix`, `review`, `accepted`, `not_public`;
- concrete file/path references for every scrub-required item;
- a CI workflow cost and permissions summary;
- a recommended public-history strategy;
- a go/no-go recommendation for changing repository visibility.
