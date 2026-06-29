# Public-Readiness Stale Artifact Recon

This checkpoint identifies repo artifacts ready for public-readiness remediation.
It extends the scrub-candidate map with dependency evidence so follow-up PRs can
remove, move, sanitize, or keep files without re-litigating scope.

## Summary

The strongest removal candidates are tracked private context, a raw VS Code test
log, old phase-archive snapshots, and legacy verification scripts that are not
used by current manifests or Exo runtime code. Active workspace packages remain
connected to current product work and should be handled through metadata or
focused package review rather than broad cleanup.

## Decision Table

| Path or Pattern | Evidence Checked | Current Dependency | Medium-Term Relevance | Disposition | Recommended Change | Confidence | Follow-up PR |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `.context/**` | `git ls-files` reports 13 tracked files; `.gitignore` marks `.context/` private; public-readiness findings already classify it as private context. | No package, workspace, build, test, or Exo runtime dependency found. | Private session material, not durable product docs. | `delete` | Remove tracked `.context/**` from the public branch. Preserve any needed raw material outside the public repo. | High | Current-tree private-context scrub |
| `packages/exosuit-vscode/test_output.txt` | `git ls-files` reports one tracked raw log; reference search finds only public-readiness reports; the file contains local paths and failed-test output. | No manifest, test, package, or runtime dependency found. | Stale failure artifact. | `delete` | Delete the tracked log. Add a sanitized fixture only if a test later proves it is needed. | High | Current-tree private-context scrub |
| `src/scripts/**` | Two tracked shell scripts; package manifests do not call them; `verify-phase.sh` comments describe the script as redundant and still reference old `docs/agent-context/current/*.toml` checks. | No active package script, workflow, or Exo runtime dependency found. | Legacy verification path replaced by Exo commands and generated checks. | `delete` | Remove the scripts and update stale prose references to current verification commands where those references are user-facing. | High | Current-tree private-context scrub |
| `src/templates/.github/instructions/exo-core.instructions.md` | `tools/exo/src/templates.rs` embeds this file with `include_str!`; `exo init` and prompt refresh code install template-managed instructions. | Active Exo template dependency. | Current project bootstrap surface. | `keep` | Keep as source template; review content during public metadata/docs hardening only if wording needs public cleanup. | High | Public metadata baseline |
| `src/templates/AGENTS.md` | Reference search finds no active `include_str!`; `tools/exo/src/templates.rs` currently carries an embedded AGENTS template constant. Stage 3/4 classification still names this path as agent-guidance evidence. | No direct runtime dependency found for this file path. | Agent guidance remains product-relevant, but this duplicate source path may be stale. | `review` | Decide whether this file should become the source for the embedded AGENTS template or be removed as a stale duplicate. | Medium | Current-tree private-context scrub |
| `src/templates/docs/agent-context/**` | Template files describe human-authored `docs/agent-context` current/future docs; current persistence docs define that directory as generated SQL projection only. No active `include_str!` references found. | No active template dependency found. | Conflicts with current repo/sidecar/shadow persistence policy. | `delete` | Remove old human-doc agent-context templates. Keep repo-policy SQL projection behavior in code/templates that are actually active. | High | Current-tree private-context scrub |
| `src/templates/docs/design/**` | No active `include_str!` references found; files contain updated public-facing concepts that overlap with current design docs. | No active runtime dependency found. | Useful design content may remain, but the template source path is not currently wired. | `review` | Either wire these as intentional scaffold templates or move useful content into maintained docs and delete the stale template copies. | Medium | Current-tree private-context scrub |
| `docs/agent-context/SESSION-STATE.md` | Tracked file; public-readiness findings classify it as private workflow state; current design says human-authored docs do not belong under `docs/agent-context/`. | No runtime dependency on this specific file found. | Session handoff, not public docs. | `delete` | Delete from public branch. Convert only durable lessons into sanitized research notes if needed. | High | Current-tree private-context scrub |
| `docs/agent-context/current/**` and `docs/agent-context/future/**` | 13 tracked files; content is task-level planning, future backlog, and old project-identity investigation; current policy reserves `docs/agent-context` for generated SQL projection. | References appear in old prompts/docs and tests, not as current canonical source files. | Some conclusions may be durable; task-by-task notes are private working context. | `move_or_sanitize` | Extract durable project-identity or policy conclusions into `docs/research/` or `docs/design/`; delete working notes. | Medium | Current-tree private-context scrub |
| `docs/agent-context/specs/**` | Four tracked specs; content is closer to durable specification material than session state, but stored under the old context directory. | No current runtime dependency on the file location found. | Likely useful public material after relocation and link review. | `move_or_sanitize` | Move current specs into `docs/specs/` or `docs/design/`; delete obsolete specs if superseded. | Medium | Current-tree private-context scrub |
| `docs/agent-context/research/**` | 55 tracked files; includes AI-tool research, fresh-eyes reports, old phase synthesis, and copied working notes; current policy reserves `docs/agent-context` for generated projection. | No current runtime dependency on the file location found. | Mixed: some durable research, some private or stale working material. | `review` | Triage into keep/move/delete batches. Move public-safe durable research to `docs/research/`; delete private or stale session notes. | Medium | Current-tree private-context scrub |
| `docs/agent-context/changelog.md` | Tracked historical phase changelog; public-readiness map flags it as stale operational history. | Older prompts mention changelog-style context, but current Exo state is SQLite-backed. | May be useful only as curated historical narrative. | `review` | Convert selected public-safe history into a curated note or delete with the private context batch. | Medium | Current-tree private-context scrub |
| `docs/research/phase-archives/**` | One tracked archived MAP north-star snapshot with an explicit archive notice; current docs and research contain maintained planning/history surfaces. | No package, build, test, or runtime dependency found. | Historical snapshot; not a maintained source. | `delete` | Remove the archived snapshot from the public branch after confirming no unique public content remains. | High | Current-tree private-context scrub |
| `docs/brainstorming/**` | 14 tracked brainstorming files; reference search finds old prompts/RFCs pointing at brainstorming paths, including withdrawn RFC context. | No package, build, test, or runtime dependency found. | Mixed ideation and market/product notes; useful pieces should become curated research/docs. | `review` | Triage file-by-file. Move public-safe durable material to `docs/research/`; delete stale working notes. | Medium | Current-tree private-context scrub |
| Older standalone research and migration reports | `docs/research/` contains command-guidance, migration, sidecar, SQLite, and frontend spike reports. Several remain cited by current RFC/reconstruction docs. | Some reports are referenced by RFCs or current reconstruction notes; no runtime dependency expected. | Useful as evidence, but some reports include path/account-specific examples. | `move_or_sanitize` | Keep current reconstruction/public-readiness notes; sanitize or archive older reports with private examples during scrub. | Medium | Current-tree private-context scrub |
| `packages/exosuit-cockpit` | Workspace package via `packages/*`; package has build/check/test scripts; Stage 3/4 classification and docs name cockpit as current UI evidence. | Active workspace package. | Current/medium-term cockpit surface. | `keep` | Keep through public-readiness scrub. Address package metadata or product status separately. | High | Public metadata baseline |
| `packages/exosuit-rtd/playground` | Explicit workspace member in `pnpm-workspace.yaml`; package depends on `@exosuit/rtd` and has dev/build/check scripts. | Active workspace package. | Developer playground for RTD; publication posture needs package review. | `review` | Keep unless a focused package review decides the playground should move out of the public tree. | Medium | Public metadata baseline |

## Evidence Commands

```sh
git ls-files .context docs/agent-context docs/brainstorming docs/research/phase-archives src/scripts src/templates packages/exosuit-vscode/test_output.txt
find . -maxdepth 4 \( -name 'package.json' -o -name 'Cargo.toml' -o -name 'pnpm-workspace.yaml' -o -name 'tsconfig.json' \) -print
rg -n 'src/scripts/check|verify-phase[.]sh|src/templates|docs/brainstorming|docs/research/phase-archives|packages/exosuit-vscode/test_output[.]txt|exosuit-cockpit|exosuit-rtd/playground' package.json pnpm-workspace.yaml Cargo.toml .github tools packages src docs --glob '!node_modules' --glob '!packages/exosuit-docs/dist/**'
rg -n 'templates' tools/exo/src tools/exo/tests src packages --glob '!node_modules'
rg -n -i '/Users/|/home/|/private/|/Volumes/|C:\\|V:\\|//Mac/|USERPROFILE|HOME|git@github[.]com:|github[.]com[:/][^[:space:])>]+' .context docs/agent-context docs packages plugins tools .github --glob '!node_modules'
```

## Recommended Remediation Order

1. Remove high-confidence deletion candidates: `.context/**`, raw VS Code test
   log, legacy scripts, stale agent-context templates, and phase-archive
   snapshot.
2. Split `docs/agent-context/**` into delete, move, and curated-history batches.
3. Triage `docs/brainstorming/**` and older standalone research reports for
   public-safe durable material.
4. Handle package metadata and active package publication posture in the public
   metadata baseline PR.
