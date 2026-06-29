<!-- exo:10154 ulid:01kmzxbcygfjj21jr4fe6y1e55 -->


# RFC 10154: Context Persistence

## Summary

Check `docs/agent-context` into version control.

## Motivation

The core philosophy of Exosuit is "Context is King". If the context is not in the repo, it is not shared, and therefore it is not the "King" for the team/project.

## Detailed Design

### Decision

We will check in `docs/agent-context`.

### Mitigation for Conflicts

- **Phased Workflow**: The strict phased approach minimizes concurrent editing of the _same_ phase files.
- **Archive Strategy**: The `complete-phase-transition.sh` script archives the current context. These archives serve as a permanent record and should definitely be checked in.
- **Current Context**: The `current/` directory is mutable. We should commit it, but perhaps squash "work in progress" updates to it when merging PRs.

## Alternatives Considered

**Git Ignore**:

- Pros: Clean history, no conflicts.
- Cons: Context loss, drift, no audit trail.

## Unresolved Questions

- How to handle merge conflicts in `task-list.md` effectively? (Mitigated by phased workflow).
