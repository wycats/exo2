<!-- exo:10158 ulid:01kmzxbd00pkw8jj1zenkmqpcg -->


# RFC 10158: Structured Context API

- **Superseded by**: RFC 0007


- **Status**: Withdrawn
- **Stage**: 4
- **Reason**:

## Summary

Define `exosuit-core` as a library for managing Agent Context as a structured database ("Markdown ORM").

## Motivation

To robustly manipulate the plan and tasks, we need a structured approach that relies on ASTs rather than regex, and stable IDs rather than string matching.

## Detailed Design

### Philosophy

- **Markdown as Database**: The file system is the DB.
- **AST over Regex**: Use `unified` / `remark`.
- **Tooling Independence**: Core logic in `exosuit-core`.

### Schema

- **IDs**: Stored in HTML comments `<!-- id: "..." -->`.
- **Relations**: `<!-- relates-to: "..." -->`.

### API

- `addPlanItem`, `completePlanItem`
- `addTask`, `completeTask`
- `getPlan`, `getTasks`

## Testing Strategy

- **Integration-First**: Test full Markdown strings.
- **No Snapshots**: Explicit expectations.
