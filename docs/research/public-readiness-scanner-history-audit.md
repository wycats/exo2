# Public-Readiness Scanner and History Audit

This checkpoint records dedicated current-tree, workflow, and retained-history
scans for the repository before public visibility. It also fixes the publication
strategy and the remaining gates for creating the public repository.

## Decision

The existing repository history will remain private. The current
`wycats/exo2` repository will become `wycats/exo2-private-history`, and a new
`wycats/exo2` repository will begin from a signed parentless commit whose tree
matches the final approved private `main`.

The repository is **not ready for public visibility yet**. Dedicated secret
scanning found no plausible credential, but the current tree still contains
private-context material and personal path examples, and the coverage workflow
still has a write-token execution path that must be hardened. The clean public
root will be created only after those blockers, the lane-centered design update,
and the final visibility review are complete.

## Audit Baseline

| Item | Value |
| --- | --- |
| Scanned commit | `5f454ca55a7cda840868f7f6fb50f9cec08df17b` |
| Branch | `origin/main` |
| Clone shape | Full-history, single-branch temporary clone |
| Main commits | 213 |
| Reachable objects | 7,776 |
| Current tracked files | 1,227 |
| Gitleaks | 8.30.1 |
| TruffleHog | 3.95.6 |
| detect-secrets | 1.5.0 |
| Actionlint | 1.7.12 |

Raw scanner output remained outside the repository. Gitleaks reports were
fully redacted. TruffleHog ran with verification disabled, and its `Raw` and
`RawV2` fields were removed before local storage. No candidate material was
sent to credential providers.

## Scanner Results

| Surface | Result | Classification | Disposition |
| --- | --- | --- | --- |
| Gitleaks current tree | 0 findings | `accepted_public` | Re-run against the final clean-root tree. |
| Gitleaks complete `main` history | 0 findings | `private_history_only` | Keep the scanned history private and re-run a delta scan after the final content changes. |
| detect-secrets current tree | 1,063 heuristic findings | `false_positive` | 1,047 are `pnpm-lock.yaml` integrity hashes. The remaining 16 are fixed ULID examples, test identifiers, and source/documentation digests. No credential-shaped result remained after review. |
| TruffleHog complete `main` history | 5 unverified findings | `false_positive` | Two are a GitLab URL template in a sidecar test, two are historical task names containing `Unify`, and one is a Rust `Box::new` expression. |
| Actionlint workflows | 0 errors | `accepted_public` | Re-run after CI hardening and before the clean root is created. |
| Plausible current-tree credentials | 0 | `accepted_public` | No credential rotation is required from this scan. |
| Plausible history-only credentials | 0 | `private_history_only` | No rotation or revocation is required from this scan. |

## Current-Tree Findings

| Area | Evidence | Classification | Required Action |
| --- | --- | --- | --- |
| Remaining `docs/agent-context` content | 37 tracked files remain: 2 root files, 9 `current` files, 4 `future` files, 18 `research` files, and 4 `specs` files. They mix session state, planning, handoffs, working research, and durable specifications. | `blocker` | Review the remaining files as one publication batch. Move public durable material to `docs/research`, `docs/design`, or `docs/specs`; remove private or stale working context. |
| Personal path examples | Real machine-local paths remain in `docs/bug-reports/locald-upgrade-order.md`, `docs/research/migration-aware-upgrade-guidance-inventory.md`, RFC `10184`, `packages/exosuit-vscode/eslint-rules/no-agent-context-toml-writes.test.ts`, and `tools/exo/src/phase_owner.rs`. Additional implementation fixtures use clearly synthetic paths. | `blocker` | Replace maintainer-specific paths, usernames, and worktree IDs with neutral examples in documentation and source fixtures while preserving the technical cases. Explicitly accept synthetic path fixtures during the final scan. |
| Account-specific dogfood context | Profile-sidecar design/RFC material, fixtures, and downstream bug reports use a specific maintainer account and repository as evidence. | `blocker` | Decide which examples are intentionally public dogfood evidence and generalize the rest. |
| Generated/runtime state | No tracked `.context` files, raw VS Code test log, SQLite database, runtime socket, or SQL projection was found. | `accepted_public` | Keep these surfaces ignored and verify the final clean-root tree. |
| Public metadata | Root README, dual MIT/Apache licensing, security policy, and package metadata are present. | `accepted_public` | Recheck links and repository URLs after the new repository is created. |
| Sidecar project identity | The live sidecar manifest project ID differs from the ID resolved from the preserved Git common directory. | `blocker` | Reconcile the manifest through reviewed `project move-root --dry-run` and apply output before freezing the cutover baseline. Preserve sidecar key `exo2` and the existing database/state root. |

The scanner findings and current-tree publication blockers are tracked by Exo
task `resolve-scanner-and-current-tree-publication-blockers`.

## Retained-History Footprint

The private history contains substantially more context than the current tree:

- 77 commits touch `.context`, `docs/agent-context`, or the removed raw VS Code
  test log.
- Those commits contain 1,043 unique paths: 13 under `.context`, 1,029 under
  `docs/agent-context`, and the raw test log.
- 27 commits add or remove machine-local path references.
- 38 commits add or remove account-specific project context.
- The reachable `main` history contains one maintainer author identity and one
  test identity.

This material includes private planning and operational context even though the
secret scanners did not identify credentials. Keeping the existing repository
private provides a clear boundary: historical context remains available to the
maintainer without becoming part of the public artifact.

## CI and Repository Settings

The repository is private at the audited baseline. GitHub Actions is enabled,
the default workflow token permission is read-only, workflows cannot approve
pull-request reviews, and no repository, environment, Dependabot, or Codespaces
secrets are configured.

Required checks live in repository ruleset `main`:

- `Test`
- `Rust Test`
- `Vercel`
- `Binary Artifacts Gate`
- `Windows Compatibility Gate`

The ruleset also protects signed commits, pull-request review, review-thread
resolution, deletion, and non-fast-forward updates. Legacy required status
checks are disabled.

| CI surface | Finding | Classification | Required Action |
| --- | --- | --- | --- |
| Generated `CI (HEAD)` | Explicit `contents: read` and non-persisted checkout credentials are present. | `accepted_public` | Preserve the generator-owned policy. |
| Binary and Windows workflows | Heavy jobs run for `main`, manual dispatch, matching paths, or `run-platform-builds`; lightweight gates are always emitted. | `accepted_public` | Recreate the workflows and gate requirements in the new repository. |
| Label-triggered PR coverage | Automatic label-gated coverage is limited to same-repository PRs and reports through a trusted helper checkout. | `accepted_public` | Preserve the same-repository guard. |
| Manually dispatched PR coverage | `manual-pr-coverage` accepts any PR number, checks out that PR head, and runs it in a job with `issues: write` without verifying that the PR belongs to the same repository. | `blocker` | Apply the same-repository guard before executing the PR head, or split untrusted coverage execution into a read-only job whose result is reported by a trusted write-scoped job. |
| Main/manual coverage | The job has repository write permissions, persists checkout credentials, and can execute a manually selected ref. | `blocker` | Separate coverage execution from trusted reporting or restrict the executable ref, and disable persisted checkout credentials before public visibility. |
| Public fork policy | Fork approval controls are not available for live verification while the repository is private. | `blocker` | Verify fork approval and token behavior in a temporary public test repository configured with the same workflows and settings. After the final visibility change, run one fork smoke PR before accepting external contributions. |
| Environments | `copilot`, `Preview`, and `Production` exist without environment protection rules or secrets. | `review` | Recreate only environments needed by the public repository and confirm Vercel behavior on the smoke PR. |

## Clean-Root Publication Strategy

The cutover preserves the private repository and the existing local Git common
directory while giving the public project a deliberately bounded history.

1. Resolve the current-tree and CI blockers, then complete the final visibility
   review.
2. Freeze one private `main` SHA and give that exact baseline to the PR #161
   lane-centered design work.
3. Rebase, update, and merge PR #161. Update `docs/vision.md` to link the merged
   local design package.
4. Re-run current-tree scans and scan the delta from this audit baseline.
5. Create a signed parentless commit whose tree is byte-for-byte identical to
   the final private `main` tree.
6. Rename the existing GitHub repository to `wycats/exo2-private-history` and
   keep it private.
7. In the preserved local Git common directory, rename the old remote and set
   its URL explicitly to the renamed private repository:

   ```sh
   git remote rename origin private-history
   git remote set-url private-history https://github.com/wycats/exo2-private-history.git
   ```

   Fetch and verify the archive remote before adding the new repository.
8. Confirm that no worktree has local `main` checked out. Rename the existing
   private-history branch, keep it tracking `private-history/main`, and create
   a new public ref at the signed parentless commit:

   ```sh
   git branch -m main private-main
   git branch --set-upstream-to=private-history/main private-main
   git branch main <clean-root-sha>
   ```

9. Add the new repository as `origin`, then push the exact public ref:

   ```sh
   git remote add origin https://github.com/wycats/exo2.git
   git push --set-upstream origin main:main
   ```

   Verify that the remote `main` commit is the signed clean root before pushing
   any descendant branch.
10. Recreate the main ruleset, read-only Actions defaults, required gates, and
   Vercel integration while the new repository is private.
11. Verify CI on a smoke PR and verify sidecar portability using a temporary
    public-repository clone plus an isolated sidecar copy.
12. Verify fork approval and token behavior in a temporary public test
    repository with the same workflow and ruleset configuration.
13. Change the new repository to public after tree equality, scanner, settings,
    CI, sidecar, worktree, and thread-continuity checks pass. Run a fork smoke
    PR before accepting external contributions.

Cutover execution is tracked by Exo task
`execute-clean-public-repository-cutover`.

## Go/No-Go

**No-go for public visibility at this checkpoint.**

The scanner result is satisfactory: no plausible credential was found in the
current tree or retained `main` history. Publication remains blocked by the
remaining current-tree private context, personal/account-specific examples,
coverage workflow permissions, sidecar identity reconciliation, and the final
clean-root verification sequence.

The audited SHA is an evidence baseline, not the final PR #161 rebase baseline.
The final baseline will be chosen only after blocker remediation and the
visibility review have frozen private `main`.
