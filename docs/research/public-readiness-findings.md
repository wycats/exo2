# Public-Readiness Findings

This checkpoint applies the audit requirements from
`docs/research/public-readiness-audit.md` to the current tracked repository
state. It records publication blockers and review decisions before any scrub
work or visibility change.

## Summary

The repository is not ready to make public yet.

The current tracked tree did not produce strict high-confidence token or private
key hits in the local regex scan, but the audit did find tracked private context,
tracked local logs, missing public-facing project metadata, CI hardening work,
and missing dedicated current-tree/history scanner coverage.

Dedicated scanners were not available on this machine during this pass:
`gitleaks`, `trufflehog`, `actionlint`, and `detect-secrets` were not found on
`PATH`. This checkpoint should therefore be treated as a structured findings
pass, not final publication approval.

## Findings

| Area | Finding | Evidence | Classification | Recommended Action | Follow-up PR |
| --- | --- | --- | --- | --- | --- |
| Scanner coverage | Dedicated secret scanners and action linting were unavailable locally, and Git history was not scanned. | `command -v gitleaks`, `command -v trufflehog`, `command -v actionlint`, and `command -v detect-secrets` returned no paths. | `must_fix` | Run dedicated current-tree and history scans before changing visibility; record any false positives. | Scanner/history audit |
| Current-tree secrets | The strict tracked-file token/key regex produced no hits. The checklist's broad `sk-` pattern produced false positives such as `task-status`, so strict follow-up matching was used for signal. | `git grep` strict token/private-key sweep returned 0 lines; broad checklist sweep returned false-positive matches. | `accepted` | Treat as provisional until dedicated scanners pass over the intended public branch and history. | Scanner/history audit |
| Tracked `.context/` material | `.context/` is ignored as private local material, but 13 tracked `.context/*` files remain and include handoff and local environment details. | `.gitignore` ignores `.context/`; tracked files include `.context/HANDOFF.md` and `.context/research/vscode-file-sync-investigation.md`. | `not_public` | Remove from the public branch/history or move to a private archive; reintroduce only sanitized durable docs if needed. | Private context scrub |
| `docs/agent-context/` content | `docs/agent-context/` contains session state, handoff material, research, and planning artifacts, not just generated portable projections. | 74 tracked files under `docs/agent-context/`, including `docs/agent-context/SESSION-STATE.md` and `docs/agent-context/research/**`. | `not_public` | Decide which files are source docs, generated projection residue, or private session context; scrub or archive private material. | Private context scrub |
| Raw local logs | A tracked VS Code extension test output file contains local usernames, absolute Linux paths, warning output, and failure stack details. | `packages/exosuit-vscode/test_output.txt` included real user-home paths before the current-tree scrub. | `must_fix` | Delete the log or replace it with a sanitized fixture if the content is still needed. | Current-tree scrub |
| Machine-local paths | Tracked docs contain real local paths such as macOS and Linux user-home paths; some are design examples and some are session evidence. | The path scan returned tracked matches in private context files, bug reports, and design examples before the current-tree scrub. | `review` | Generalize real personal paths; keep only intentional examples that are harmless and clearly illustrative. | Private context scrub |
| Private account and dogfood references | Some design/research docs use account-specific dogfood examples such as `wycats/locald` and profile sidecar discovery fixtures. | `docs/research/github-profile-sidecar-discovery-inventory.md` and `docs/design/github-profile-sidecar-discovery.md`. | `review` | Decide whether to keep as public dogfood evidence or rewrite to neutral owner/repo examples. | Private context scrub |
| Runtime and generated state | No tracked `.cache/`, `.exo/`, SQLite DB, runtime socket, or SQL projection files were found. Ignored runtime artifacts are present locally. | `git status --short --ignored` showed ignored `target/`, `node_modules/`, package build output, and `lcov.info`; no tracked DB/runtime projection files were identified. | `accepted` | Keep runtime and generated state ignored; review generated research/projection-like docs separately. | None unless scanner finds more |
| Public metadata baseline | The repository has no root `README.md`, no root `LICENSE`, no `SECURITY.md`, and GitHub reports no license metadata. | `find` found package/doc README files and `packages/exosuit-vscode/LICENSE`, but no root README/license/security file; `gh repo view` reports `licenseInfo: null`. | `must_fix` | Add minimal root README, root license, and security reporting policy before public visibility. | Public metadata PR |
| Package and extension metadata | Public package metadata is incomplete or stale: root package is private, several packages lack public license/repository fields, and the VS Code extension repository points at `https://github.com/exosuit/exosuit.git`. | `package.json`, `packages/*/package.json`, `packages/exosuit-vscode/package.json`, `plugins/exo/.codex-plugin/plugin.json`. | `review` | Decide which packages remain internal and update repository/license metadata for packages intended to be public-facing. | Public metadata PR |
| CI required checks | Required checks now live in repository ruleset `main`; legacy branch-protection status checks are disabled. | Ruleset `16556244` requires `Test`, `Rust Test`, `Vercel`, `Binary Artifacts Gate`, and `Windows Compatibility Gate`; branch protection required status checks return HTTP 404. | `accepted` | Keep ruleset-based required checks. | None |
| CI permissions | Generated `CI (HEAD)` workflow lacks explicit top-level `permissions`, so it relies on GitHub defaults; checkout also uses default credential persistence. | `.github/workflows/exo-ci.yml`. | `must_fix` | Add least-privilege workflow permissions and disable credential persistence where no write is needed, ideally at the generator source. | CI hardening PR |
| CI cost posture | Binary and Windows workflows now emit lightweight gates and run heavy jobs on `main`, manual dispatch, `run-platform-builds`, or path selection. Coverage is label/schedule/manual gated and same-repo guarded for PR coverage. | `.github/workflows/exo-binaries.yml`, `.github/workflows/windows-ci.yml`, `.github/workflows/rust-coverage-audit.yml`. | `accepted` | Keep current gate model; revisit fork policy once the repository is public and real public PR traffic exists. | None |
| CI history/fork posture | The coverage audit has job-level reduced permissions and trusted-helper checkout for PR reporting; full public fork behavior still needs a live public-repo review. | `.github/workflows/rust-coverage-audit.yml`. | `review` | Re-check Actions settings, fork PR approval policy, environments, and repository secrets before flipping visibility. | GitHub settings review |
| Public history strategy | Current-tree findings include tracked private context and logs, so current history should not be made public until a history scan and strategy decision are complete. | Tracked `.context/`, `docs/agent-context/`, and `packages/exosuit-vscode/test_output.txt`. | `must_fix` | Decide between publishing cleaned history, publishing a scrubbed branch, or rewriting selected history before visibility changes. | Scanner/history audit |

## CI and Settings Notes

- The repository is currently private.
- The active required-check policy is the repository ruleset `main`, not legacy
  branch-protection status checks.
- Required checks are `Test`, `Rust Test`, `Vercel`, `Binary Artifacts Gate`,
  and `Windows Compatibility Gate`.
- `Exo Binaries` and `Windows CI` use classifier jobs plus always-emitted gate
  jobs. Heavy platform work runs for `main`, manual dispatch, the
  `run-platform-builds` label, or matching path changes.
- `Rust Coverage Audit` is not a normal required PR check. It is label-gated
  for same-repo PRs, scheduled/manual for main auditing, and uses trusted-helper
  checkout for reporting.
- The generated `CI (HEAD)` workflow still needs explicit least-privilege
  permissions before public visibility.

## Recommended Public-History Strategy

Do not publish the current history as-is.

The next public-readiness step should run dedicated scanner coverage over the
intended public branch and its retained history, then choose one of two concrete
paths:

1. Publish a cleaned branch with private context removed from the current tree
   and retained history.
2. Publish a fresh/squashed public branch after archiving private development
   history outside the public repository.

The current evidence favors a cleaned public branch unless the history scan
finds credential material or pervasive private context that makes a squash/import
cleaner.

## Go/No-Go

Recommendation: no-go for public visibility now.

Minimum work before visibility changes:

- remove or privatize tracked `.context/` material;
- classify and scrub `docs/agent-context/` private session/research content;
- delete or sanitize `packages/exosuit-vscode/test_output.txt`;
- add root README, root license, and security reporting policy;
- harden generated CI permissions;
- run dedicated secret and history scanners;
- decide and document the public-history strategy.
