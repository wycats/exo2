# Public-Readiness Scrub Candidates

This checkpoint turns `docs/research/public-readiness-findings.md` into a
file-level action map. It does not remove or rewrite source files; it records the
candidate decisions needed before scrub PRs begin.

## Summary

The current tree has no high-confidence tracked credential hit in the strict
regex scan after excluding the public-readiness reports' own example commands. The main
publication blockers are tracked private context, tracked local logs, missing
public metadata, generated CI permissions, and missing dedicated scanner/history
coverage.

Tracked candidate counts from this pass:

- `.context/`: 13 files.
- `docs/agent-context/`: 74 files: 1 session state file, 1 changelog, 9
  `current/` files, 4 `future/` files, 55 `research/` files, and 4 `specs/`
  files.
- `packages/exosuit-vscode/test_output.txt`: 1 tracked raw log.

## Scrub Candidate Map

| Path or Pattern | Evidence | Risk | Disposition | Recommended Change | Follow-up PR | Review Needed |
| --- | --- | --- | --- | --- | --- | --- |
| `.context/**` | `.gitignore` marks `.context/` private, but `git ls-files .context` returns 13 tracked files. Examples include `.context/HANDOFF.md` and `.context/research/vscode-file-sync-investigation.md`. | Private session state, local paths, stale handoff instructions, and environment-specific debugging evidence. | Remove from public tree. | Delete tracked `.context/**` from the public branch and preserve any needed raw material in a private archive. Reintroduce only sanitized durable notes under `docs/research/` or `docs/design/`. | Current-tree private-context scrub | No for deletion; yes before reintroducing any sanitized replacement. |
| `docs/agent-context/SESSION-STATE.md` | Session handoff for PR #75 and local CI/debug state. | Private workflow state and stale operational instructions. | Remove from public tree. | Delete from public branch. If a durable lesson remains useful, create a sanitized research note outside `docs/agent-context/`. | Current-tree private-context scrub | No. |
| `docs/agent-context/changelog.md` | Historical Exo phase changelog, including old cleanup and reactive work records. | Stale operational history presented as current project context. | Review then move or remove. | Keep only if converted into a curated historical note; otherwise delete with the rest of private context. | Current-tree private-context scrub | Yes. |
| `docs/agent-context/current/**` | 9 tracked task-level files for project identity and shadow-state investigation; contains real local home-directory path examples. | Private planning state mixed with design material. | Split. | Move durable project-identity conclusions into `docs/research/` or `docs/design/`; delete task-by-task working notes from the public branch. | Current-tree private-context scrub | Yes. |
| `docs/agent-context/future/**` | 4 tracked future-work files, including `ideas.md` and deferred work. | Private backlog and speculative planning context. | Review then move or remove. | Promote still-relevant public ideas into RFCs or `docs/research/`; delete backlog/session residue. | Current-tree private-context scrub | Yes. |
| `docs/agent-context/research/**` | 55 tracked research files, including old fresh-eyes reports and third-party AI-tool research. | Mixed durable research, old session synthesis, possible copied quotes, and private evaluation context. | Split. | Move selected public-safe research into `docs/research/`; delete private or stale working notes from the public branch. | Current-tree private-context scrub | Yes. |
| `docs/agent-context/specs/**` | 4 tracked specs, including `rfc-lifecycle.md` and `tooling-interface.md`; content is generally durable but stored under the old context directory. | Good public material hidden inside a directory now defined as generated/private context. | Move. | Move current specs into `docs/specs/` or `docs/design/`, updating links; delete obsolete specs if superseded. | Current-tree private-context scrub | Yes for destination and link updates. |
| `docs/agent-context/` directory policy | Current persistence docs define `docs/agent-context/*.sql` as generated repo-policy projection, not a human-authored docs home. Current tree has 74 human-authored files there and no tracked SQL projection files. | Directory meaning is inconsistent with current design and public-readiness policy. | Normalize. | Empty the tracked human-authored directory after moving/deleting candidates. Keep only intentional generated projection placeholders if current repo policy needs them. | Current-tree private-context scrub | Yes. |
| `packages/exosuit-vscode/test_output.txt` | Tracked raw E2E output includes a real Linux home-directory checkout path, warning output, stack traces, and failed test artifact paths. | Machine-local path leak and stale failure log. | Delete. | Remove the file from the public branch. If a fixture is needed, replace it with a small sanitized fixture committed under an explicit fixture path. | Current-tree scrub | No unless a replacement fixture is proposed. |
| Real local path references outside private-context dirs | Targeted scan found real paths in a local-upgrade bug report, a migration guidance inventory, RFC `10184`, package/tool tests, and implementation fixtures, in addition to public-readiness docs. | Personal path leakage and machine-specific examples. | Sanitize or accept deliberately. | Replace real personal paths with neutral examples in user-facing docs/RFCs. Keep test fixture paths when clearly synthetic, such as `/home/me/...`. | Private context scrub | Yes for RFC/design examples that may be intentional dogfood evidence. |
| Account-specific dogfood examples | Scan found personal owner/repository examples, project-specific sidecar state names, GitHub remote fixtures, and profile-sidecar examples in RFC `10187`, profile-sidecar design/research docs, docs site reference pages, and sidecar tests. | Exposes private dogfood naming and can imply unsupported public setup. | Review and sanitize. | Use neutral example owners/repos in docs and fixtures unless a specific public dogfood example is intentionally retained. | Private context scrub | Yes. |
| Root public metadata | `find` found no root `README*`, `LICENSE*`, or `SECURITY*`. `package.json` root is private. | Public repo lacks basic orientation, license grant, and vulnerability reporting path. | Add. | Add a minimal root README, root license, and security policy before changing visibility. | Public metadata baseline | Yes for license choice and security contact. |
| Package and plugin metadata | Package scan shows missing license/repository metadata for `@exosuit/core` and `@exosuit/rtd`; the VS Code package has repository metadata but no license field; Cargo workspace/crate manifests lack license/repository metadata; several packages are intentionally private. | Public package metadata is stale or incomplete. | Review and update. | Decide which packages, extension manifests, and Rust crates are public-facing, then add/update `license`, `repository`, and package visibility metadata consistently. | Public metadata baseline | Yes. |
| Generated CI workflow permissions | `.github/workflows/exo-ci.yml` has no explicit top-level `permissions`; it is auto-generated from `.config/exo/hooks.toml`. Other hand-written workflows already declare `permissions: contents: read`. | Public PRs would rely on default token permissions for generated CI. | Harden at generator/config source. | Update the exohook workflow emission path or config so generated CI includes least-privilege permissions and checkout credential persistence policy where applicable. | CI permissions hardening | No for least-privilege intent; yes if generator interface changes. |
| Dedicated scanner coverage | `gitleaks`, `trufflehog`, `actionlint`, and `detect-secrets` were not on `PATH`. Strict tracked-file regex produced no credential hits after excluding the public-readiness reports' own example commands. | Current-tree and history approval is incomplete without dedicated tooling. | Run before visibility change. | Run current-tree and retained-history scans on the intended public branch; record false positives and blockers. | Scanner/history audit | No for running scanners; yes for any cleanup strategy change. |
| Public history strategy | Current-tree evidence includes tracked private context and logs; history has not been scanned. | Making existing history public may expose removed private material. | Decide after scanner pass. | Choose cleaned-history publication, scrubbed branch publication, or fresh/squashed public import after scanner results. | Scanner/history audit | Yes. |

## Recommended PR Sequence

1. Current-tree private-context scrub: remove `.context/**`, delete the raw VS
   Code log, and split `docs/agent-context/**` into delete/move/keep outcomes.
2. Public metadata baseline: add root README, license, security policy, and
   package/plugin metadata corrections.
3. CI permissions hardening: update generated CI permissions at the
   exohook/config source and verify emitted workflow output.
4. Scanner/history audit: run dedicated current-tree and retained-history
   scanners on the intended public branch.
5. Final visibility-readiness review: confirm no `must_fix` or `not_public`
   rows remain unresolved before changing repository visibility.

## Audit Commands Run

```sh
git ls-files .context docs/agent-context packages/exosuit-vscode/test_output.txt
rg -n -i '/Users/|/home/|/private/|/Volumes/|C:\\|V:\\|//Mac/|USERPROFILE|HOME' .context docs/agent-context docs packages plugins tools .github --glob '!node_modules'
rg -n -i 'secret|token|credential|password|cookie|private key|api key|billing|wallet|private repo|account-specific repo|profile sidecar|sidecar discovery|git@github[.]com:|github[.]com[:/][^[:space:])>]+' .context docs/agent-context docs packages plugins tools .github --glob '!node_modules'
git grep -n -I -E 'BEGIN (RSA|OPENSSH|EC|PRIVATE) KEY|ghp_|github_pat_|sk-[A-Za-z0-9]{20,}|xox[baprs]-|AKIA[0-9A-Z]{16}' -- . ':!docs/research/public-readiness-audit.md' ':!docs/research/public-readiness-scrub-candidates.md' ':!package-lock.json' ':!pnpm-lock.yaml'
command -v gitleaks trufflehog actionlint detect-secrets
```
