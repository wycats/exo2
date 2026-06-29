<!-- exo:10199 ulid:01kmzxefczy7st56x8v82x637p -->

# RFC 10199: Implicit Walkthrough via Task Logs


# RFC 10199: Implicit Walkthrough via Task Logs

## Summary

The walkthrough narrative is an **aggregation of task logs**, not a separate artifact. Task logs (`[[plan.goals.tasks.log]]` in implementation-plan.toml) are the single source of narrative evidence. The walkthrough is a derived view rendered from them.

## Background

The walkthrough was previously a separate `walkthrough.toml` file with dedicated CLI commands. RFC 0064 deprecated that file; RFC 0131 moved narrative evidence into task-embedded `log[]` entries. This RFC completes the transition by defining the walkthrough as a pure derivation.

## Design

### Schema

Task logs live in implementation-plan.toml (defined by RFC 0131):

```toml
[[plan.goals.tasks.log]]
kind = "note"          # "note" | "progress" | "completion"
message = "Refactored validation to use the new hook schema"
when = "2026-02-12T22:15:00Z"
```

- **`kind = "note"`** — General narrative entry (the default)
- **`kind = "progress"`** — Intermediate progress checkpoint (see RFC 00242)
- **`kind = "completion"`** — Auto-appended when a task is completed via `exo task complete --log`

No new schema is needed. The `log[]` array on tasks IS the walkthrough data.

### Write Path

Log entries are written by:

1. **`exo task complete --log "..."`** — Appends a `kind = "completion"` entry (already implemented)
2. **`exo task log <id> --message "..."`** — Appends a `kind = "progress"` entry (RFC 00242, not yet implemented)
3. **Strike completion** — Auto-appends a summary entry to relevant tasks

### Read Path (Walkthrough Rendering)

The walkthrough is rendered by aggregating task logs across all goals:

- **`exo context`** — Includes a "Walkthrough" section derived from task logs (**implemented**)
- **`exo phase status`** — Includes walkthrough entries in output (**implemented**)
- **Sidebar** — Shows the latest log entry per goal in the Phase Details tree (RFC 00242)

The read path is implemented in `walkthrough.rs`, which extracts entries from both `[[plan.goals.tasks.log]]` and goal completion logs (joined from plan.toml per RFC 00177).

### Sidebar Visibility

The latest task log entry for each goal should be visible in the Phase Details sidebar tree, providing at-a-glance progress without reading the chat transcript. See RFC 00242 for the sidebar integration design.

## Implementation Status

| Item                                                      | Status              |
| --------------------------------------------------------- | ------------------- |
| `walkthrough.toml` deprecated and removed                 | Done (RFC 0064)     |
| Task log schema in implementation-plan.toml               | Done (RFC 0131)     |
| `walkthrough.rs` reads task logs and goal completion logs | Done                |
| `exo context` renders walkthrough section                 | Done                |
| `exo phase status` renders walkthrough entries            | Done                |
| Templates no longer create walkthrough.toml               | Done                |
| `exo task complete --log` writes completion entries       | Done                |
| `exo task log` writes intermediate progress entries       | Not yet (RFC 00242) |
| Sidebar shows latest log entry per goal                   | Not yet (RFC 00242) |
| Strike completion auto-appends summary log                | Not yet             |

## References

- RFC 0064: Deprecates walkthrough.toml
- RFC 0131: Defines task-embedded `log[]` and `verification[]` in implementation-plan.toml
- RFC 00177: Data Location Axiom (completion_log lives in plan.toml)
- RFC 00242: Progress Tool — provides the write command (`exo task log`) and sidebar integration
