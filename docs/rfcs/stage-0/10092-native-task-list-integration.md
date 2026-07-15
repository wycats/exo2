<!-- exo:10092 ulid:01kmzxefev8vn07vc9768j2xcb -->


# RFC 10092: Native Task List Integration

- **Supersedes**: RFC 0171


## Summary

Investigate integrating Exosuit's phase/task system with VS Code's native "Task List" UI (if available/applicable) or similar native UI surfaces to reduce custom UI maintenance and feel more "at home".

## Motivation

Currently, Exosuit renders its own task list in a custom TreeView. VS Code has been experimenting with native Task List APIs (e.g., for GitHub Issues). Using native surfaces could improve performance and consistency.

## Detailed Design

_Research required:_

- Does VS Code expose a generic "Task List" API for extensions?
- Can we map our `plan.toml` structure to it?
- How do we handle "verification" actions in a native list?

## Unresolved Questions

- Is the API stable?
- Does it support the rich metadata we need (status, verification)?
