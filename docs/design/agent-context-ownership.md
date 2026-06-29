# Agent Context Ownership

`docs/agent-context/` is a generated SQL projection location for repo-policy projects. It is not a durable human-doc home.

## Ownership model

| Surface                    | Owner                       | Location                              |
| -------------------------- | --------------------------- | ------------------------------------- |
| Operational state          | `exo` SQLite state          | `{state_root}/cache/exo.db`           |
| Repo-policy SQL projection | `exo` generator             | `docs/agent-context/*.sql`            |
| Sidecar SQL projection     | `exo` generator             | sidecar project `agent-context/*.sql` |
| Shadow state               | local user state            | no workspace projection               |
| RFC prose                  | humans + `exo rfc` workflow | `docs/rfcs/`                          |
| Research notes             | humans/agents               | `docs/research/`                      |
| Design notes               | humans/agents               | `docs/design/`                        |
| Specifications             | humans/agents               | `docs/specs/`                         |

## State policy paths

- Repo policy resolves `state_root` to the repository-owned `.exo/` directory and writes SQL projection files to `docs/agent-context/*.sql`.
- Sidecar policy resolves `state_root` to `<sidecar_root>/projects/<sidecar_key>` and writes SQL projection files to `<state_root>/agent-context/*.sql`.
- Shadow policy resolves `state_root` to local user state under `$HOME/.exo/projects/<project_id>` and does not write a workspace projection.

## Rules

- Use `exo` commands to read and mutate operational state.
- Treat SQL projection files as generated infrastructure.
- Do not create human-authored notes under `docs/agent-context/`.
- Use `docs/design/`, `docs/research/`, or `docs/specs/` for durable prose.
- Template and bootstrap material belongs in templates and adoption flows, not in generated projection directories.
