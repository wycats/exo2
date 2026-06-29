# Public-Readiness Final Visibility Review

This checkpoint reviews the repository after the current-tree publication
blocker scrub. It records the final pre-cutover scanner evidence, repository
settings posture, sidecar identity state, and publication decision.

## Decision

The repository is ready to proceed into final cutover preparation. Clean-root
minting is gated by sidecar project identity reconciliation.

The selected publication strategy remains:

1. Keep the existing development history private by renaming the current
   repository to `wycats/exo2-private-history`.
2. Create a new `wycats/exo2` repository from a signed parentless clean root.
3. Push only the clean root and branches descended from it to the new public
   repository.

The source and documentation tree is ready for the next cutover checkpoint.
Scanner output found no plausible credentials, tracked private context has been
removed from the current tree, stale standalone WASM artifacts have been
removed, workflow syntax is clean, and the open PR backlog targeting `main` has
been resolved. The cutover task still needs to reconcile sidecar project
identity, run final scans from the post-backlog baseline, mint the signed clean
root, recreate repository settings, and verify the temporary public-repo smoke
path before the visibility change.

## Review Baseline

| Item | Value |
| --- | --- |
| Reviewed private `main` SHA | `f80a72448425488a741aa19ec4fe9dbeb9d32d02` |
| Branch | `origin/main` |
| Current tracked files | 1,196 |
| Current `main` commits | 217 |
| Gitleaks | 8.30.1 |
| TruffleHog | 3.95.6 |
| detect-secrets | 1.5.0 |
| Actionlint | 1.7.12 |

Raw scanner output stayed outside the repository. The committed report contains
counts and classifications only.

## Scanner Results

| Surface | Result | Classification | Disposition |
| --- | --- | --- | --- |
| Gitleaks current tree | 0 findings | `accepted_public` | Re-run against the final clean-root tree before the visibility change. |
| Gitleaks retained `main` history | 0 findings | `private_history_only` | Existing history remains private; re-run on the final private baseline before clean-root creation. |
| TruffleHog retained `main` history | 5 reviewed unverified false positives carried forward from the full-history audit; 0 additional findings in the final branch scan with verification disabled | `private_history_only` | Existing history remains private; no credential rotation was identified by the reviewed findings. |
| detect-secrets tracked current tree | 1,060 heuristic findings | `false_positive` | 1,047 are `pnpm-lock.yaml` integrity hashes. The rest are fixed ULID fixtures, an upstream commit hash, and source/documentation examples. No credential-shaped result remained after review. |
| Strict tracked credential regex | 0 findings | `accepted_public` | Preserve as a quick pre-push check; scanner runs remain authoritative. |
| Actionlint workflows | 0 errors | `accepted_public` | Re-run after the clean root is pushed to the new private repository. |

## Current-Tree Review

| Area | Evidence | Classification | Disposition |
| --- | --- | --- | --- |
| `docs/agent-context` tracked files | `git ls-files docs/agent-context` returned 0 paths. | `accepted_public` | Human-authored agent-context material is no longer tracked in the current tree. |
| `.context` tracked files | `git ls-files .context` returned 0 paths. | `accepted_public` | Local inbox/state remains outside the public tree. |
| Raw VS Code test log | `git ls-files packages/exosuit-vscode/test_output.txt` returned 0 paths. | `accepted_public` | No stale raw log remains tracked. |
| Real maintainer path text scan | Targeted text scan for real maintainer home, mounted, and DevDrive paths returned 0 tracked source or documentation paths outside prior audit reports. | `accepted_public` | Synthetic fixture paths remain acceptable when they are part of tests or documentation examples. |
| Tracked WASM artifact path scan | The stale standalone `rfc-status.wasm` artifacts were removed; a binary-aware scan over tracked WASM artifacts is clean. | `accepted_public` | Keep the tracked-WASM scan in the final cutover validation. |
| Account-specific dogfood examples | Sidecar discovery RFCs, docs, and tests still use a maintainer-owned dogfood repository as fixture evidence. | `accepted_public` | Keep as intentional public dogfood evidence for RFC `10187` and its implementation tests. Future docs can add neutral examples, but this does not block the clean-root cutover. |
| Public metadata | Root README, dual license files, security policy, package metadata, and Cargo workspace metadata are present. | `accepted_public` | Re-check repository URLs after the new repository is created. |

## CI and Settings Review

| Surface | Evidence | Classification | Disposition |
| --- | --- | --- | --- |
| Repository visibility | Current repository remains private. | `accepted_public` | Preserve private history until the clean-root repository is verified. |
| Ruleset `main` | Required checks are `Test`, `Rust Test`, `Vercel`, `Binary Artifacts Gate`, and `Windows Compatibility Gate`; deletion, non-fast-forward, signatures, pull-request review, review-thread resolution, and code-quality rules are active. | `accepted_public` | Recreate this ruleset in the new repository before the public flip. |
| Legacy branch-protection checks | Legacy required status checks are disabled. | `accepted_public` | Keep ruleset-owned required checks. |
| Actions permissions | Repository actions are enabled; no repository or environment secrets are configured. | `accepted_public` | Recreate read-only defaults in the new repository and verify fork PR token behavior before accepting external contributions. |
| Environments | `copilot`, `Preview`, and `Production` exist without secrets. | `review` | Recreate only the environments needed by Vercel and public workflows. |
| Heavy workflow gates | Binary and Windows gates are present and required; heavy jobs are selectable by path, main, manual dispatch, or `run-platform-builds`. | `accepted_public` | Preserve this cost/control model after cutover. |
| Rust coverage workflow | Trust-boundary hardening has landed; manual execution paths are read-only or reported through trusted jobs. | `accepted_public` | Preserve the security-contract check in generated CI. |

## Sidecar Identity Review

The sidecar key remains `exo2`, and all retained live worktrees checked in this
review resolve to the same computed project ID:

| Surface | Value |
| --- | --- |
| Sidecar key | `exo2` |
| Computed project ID from retained worktrees | `ceec7d11944377d6` |
| Sidecar manifest project ID | `a722ec8ac571839a` |
| State root | `<sidecar-root>/projects/exo2` |
| Database root | unchanged |

This confirms a single shared state root, but the manifest ID still differs from
the computed project ID. A reviewed `exo project move-root --dry-run` also reported
that both the previous and current workspace roots have active project state.

Cutover must reconcile the sidecar manifest and active workspace rows before
freezing the public-root baseline. The cutover task should preserve the key
`exo2`, preserve the current database/state root, and verify every retained
worktree resolves to the reconciled project ID after applying the move.

## Lane-Centered Design Baseline

PR `#161`, `Add lane-centered workbench design package`, was rebased, aligned
with the current workspace-centered vision, sidecar identity model, outcome
review language, reactive storage baseline, and public-readiness scrub state,
then merged into `main`.

| Item | Value |
| --- | --- |
| PR | `#161` |
| Final head SHA | `b7f50a7b19890449ace76b16fa430841c562455a` |
| Merge commit | `b94172f3070721cacc3c12166410da74746e4ee0` |

The lane-centered design package is now local durable design material under
`docs/design/lane-centered-workbench/`, and `docs/vision.md` links to that
package instead of the historical private PR.

## Open PR Backlog Review

The clean public root receives resolved content from the private `main` line and
future branches created from the clean root. Existing private PR branches remain
available through `exo2-private-history`; closed private PRs become future
public work only through explicit follow-up tasks and fresh clean-root branches.

The backlog sweep started from the PR inventory captured after PR `#228` landed
on `main` at `47c1b2535676c0a41f624c6dee17540f1eee4a0a`. The final sweep left
no open PRs targeting `main`.

| PR | Title | Final disposition | Final head SHA | Merge commit or close result |
| --- | --- | --- | --- | --- |
| `#218` | Clarify sidecar ownership status output | Merged into private `main`. | `e58e886f6325dd1962886944608059d5d7c6d537` | `619e1873ea12e7a60bd025b6256dd0fc4b5c4a74` |
| `#139` | Defer context loading for MCP startup | Merged into private `main`. | `18ff3efff6c041a7970f8143b1ef7809af379439` | `5b248d390f58645875a77703359622492d9ca472` |
| `#199` | Harden sidecar bootstrap guidance | Merged into private `main`. | `0d0858eb3e8b7e5b9cef0edc0ee37b590b44f95c` | `f1142846e93f55f6de4c2d36236fffc76c85894e` |
| `#198` | Bootstrap sidecar projects from remote first | Closed as superseded by the current sidecar bootstrap/discover behavior carried through `#199`. | `9db09e6bae5316feb368e951107ebf6776fab4a6` | Closed without merge. |
| `#174` | Add command guidance validation harness | Closed as superseded by current command guidance, generated CI, and review-surface work. | `cb04274b691ba0cf171a57181565ea9701eba438` | Closed without merge. |
| `#179` | Add command surface coherence builders | Closed as superseded by current command guidance, generated CI, and review-surface work. | `4529d16960a1d26c725005a38dd0a8a0a44d1523` | Closed without merge. |
| `#215` | Refine MCP Codex RFC cluster | Merged into private `main`. | `419b3b293f037d98643b3049825cc1360d3ec8bc` | `81ac35bbdd1da16452c4978a9f13f57f82a3bc98` |
| `#122` | Draft sidecar sync contract RFC | Closed as superseded by the current sidecar implementation, RFC `10184`, and public-readiness sidecar cutover requirements. | `de7a222c636ab260ac00e87195316c9b6dd05607` | Closed without merge. |
| `#119` | Add practical onboarding guides | Merged into private `main`. | `660c51ab449c360dac054c1637f52b61ff829f9c` | `1e0d606ec340977e865593336c8a6f0b66b5fc41` |
| `#161` | Add lane-centered workbench design package | Merged into private `main` as the final planned content PR before clean-root cutover. | `b7f50a7b19890449ace76b16fa430841c562455a` | `b94172f3070721cacc3c12166410da74746e4ee0` |

| Item | Value |
| --- | --- |
| Open PRs targeting `main` after sweep | `0` |
| Post-backlog candidate baseline before this checkpoint | `b94172f3070721cacc3c12166410da74746e4ee0` |
| Current tracked files at that baseline | 1,203 |
| Current `main` commits at that baseline | 227 |

After this checkpoint lands, its merge commit becomes the baseline for the final
scanner rerun and clean-root freeze, unless another approved cutover-preparation
change lands first.

## Cutover Gate

Proceed to `execute-clean-public-repository-cutover` with these required checks:

1. Reconcile the sidecar manifest project ID and active workspace state.
2. Re-run current-tree scanner coverage and a history delta scan from this
   checkpoint's private `main` merge commit.
3. Create a signed parentless clean-root commit whose tree exactly matches the
   final private `main`.
4. Rename the current repository to `wycats/exo2-private-history` and keep it
   private.
5. Add a new private `wycats/exo2`, push only the clean root, and recreate
   rulesets, Actions defaults, Vercel, environments, and required gates.
6. Verify a smoke PR and temporary public-fork behavior before changing
   visibility.
7. Verify sidecar portability with an isolated sidecar copy.

## Handoff Requirements

The final cutover handoff must report:

1. The clean public root strategy: new signed root in a new repository, with the
   existing repository retained as private history.
2. Whether RFC reconstruction/public-readiness is complete.
3. The exact frozen private-history commit used to mint the signed clean root,
   plus the signed clean-root SHA.
4. The reconciled sidecar project identity and confirmation that retained
   worktrees resolve to it.

## Validation Commands

```sh
gitleaks detect --source . --no-git --no-banner --redact --exit-code 0
gitleaks detect --source . --log-opts origin/main --no-banner --redact --exit-code 0
uvx detect-secrets scan
trufflehog git file://<common-checkout> --branch wycats/public-readiness-final-visibility-review --no-verification --no-update --json --results=verified,unknown,unverified
actionlint .github/workflows/*.yml
git grep -n -I -E 'BEGIN (RSA|OPENSSH|EC|PRIVATE) KEY|ghp_|github_pat_|sk-[A-Za-z0-9]{20,}|xox[baprs]-|AKIA[0-9A-Z]{16}' -- . ':!docs/research/public-readiness-*.md' ':!pnpm-lock.yaml'
git grep -n -I -E '/Users/[^[:space:]"`]+|/home/[^[:space:]"`]+|/private/[^[:space:]"`]+|/Volumes/[^[:space:]"`]+|(^|[^[:alpha:]])[A-Za-z]:\\|//Mac/' -- . ':!docs/research/public-readiness-*.md' | rg -v '/home/(dev|me|user)|/var/home/me|/Users/dev|/c/Users/alice|[Cc]:\\{1,2}(Repo|repo|Tools|work|project|tmp|Users\\{1,2}(dev|alice)|cargo-install-root)'
git ls-files '*.wasm' | while IFS= read -r file; do test -f "$file" && strings "$file"; done | rg -n '/Users/|/home/|/private/|/Volumes/|C:\\|V:\\|//Mac/'
rg -n 'pull/161|#161' docs -g '!docs/research/public-readiness-*.md'
gh pr list --state open --base main --limit 100 --json number,title,headRefName,headRefOid,baseRefName,isDraft,mergeStateStatus,reviewDecision,updatedAt,url
git ls-files docs/agent-context .context packages/exosuit-vscode/test_output.txt
exo project resolve
exo sidecar status
exo project move-root --key exo2 --to <current-worktree> --dry-run
```
