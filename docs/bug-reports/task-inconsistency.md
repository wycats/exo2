Why the Exosuit tool can’t list tasks (and why it’s “not blocking”)

You effectively have two “task systems” in play:
The exo CLI (installed: exo 0.1.0) treats this as an Exosuit-managed repo via the presence of the agent-context artifacts (especially plan.toml and the “current” TOMLs). It reads tasks for the active phase from task-list.toml.
The editor-integrated Exosuit tool wrapper (functions.exosuit) looks for a separate task configuration file (exosuit.toml or .config/exo/exosuit.toml). In this repo, those files do not exist (the tool’s own “artifacts” list confirms exists: false), so list kind=tasks returns an empty list.
That’s why it’s “not blocking” if you standardize on exo CLI + agent-context TOML: all phase/task truth and mutation can still be done with exo commands, and the wrapper is just missing a configuration bridge.
Bug report draft for exo maintainers (copy/paste)
Title:

Exosuit tool wrapper can’t list tasks in agent-context-based projects (no exosuit.toml)
Environment:

OS: Linux
Repo: wycats/dotlocal (branch main)
exo --version: exo 0.1.0
Steps to reproduce:

In an Exosuit project that uses agent-context TOMLs (has plan.toml and task-list.toml):
Run exo phase status --format json and observe an active phase with tasks (e.g. phase 109 shows tasks 109.1–109.8).
Run exo task list and observe tasks listed correctly.
Call the editor-integrated Exosuit wrapper list(kind="tasks").
Actual result:

Wrapper returns no tasks and says you “may need task config (exosuit.toml …) with a [tasks] table”.
Expected result (one of):

Wrapper detects agent-context-based Exosuit projects and lists tasks from task-list.toml, matching exo task list, OR
Wrapper clearly documents it only supports exosuit.toml-configured tasks and provides a direct remediation (e.g., “create exosuit.toml” plus an example), OR
Wrapper shells out to exo task list --format json when available.
Notes:

The repo clearly is an Exosuit project (exo phase status works and recognizes active phase, tasks, and snapshot invariants).
The wrapper already detects agent-context artifacts (it can list plan.toml as an artifact), but doesn’t bridge that into task listing.
Suggested fix:

If exosuit.toml is absent but plan.toml exists, treat it as an agent-context project and implement task listing by parsing task-list.toml (or by invoking exo task list --format json).
