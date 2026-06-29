This user should not be clearing out existing phases/tasks when adding new ones. The fact that they had to do this to avoid ID collisions suggests a problem with the way `exo` handles phase/task identifiers.

---

<note>Full transcript follows</note>

I’m going to start Phase 110 via `exo`, then confirm the new active phase + task list so the context is aligned for the next chunk of work.

Ran terminal command: git status --porcelain=v1

Ran terminal command: exo phase start 110

Ran terminal command: exo phase status --format json | jq '{phase_id: .phase_id, phase_title: .phase_title, epoch_title: .epoch_title, git_dirty: .git_dirty}'

Ran terminal command: exo task list

Ran terminal command: ls -la docs/agent-context/current | sed -n '1,120p'

Ran terminal command: git status --porcelain=v1 && git diff --stat

Ran terminal command: git diff -- docs/agent-context/plan.toml | sed -n '1,120p'

Ran terminal command: sed -n '1,200p' docs/agent-context/current/implementation-plan.toml

Ran terminal command: exo impl --help | sed -n '1,220p'

Ran terminal command: git add docs/agent-context/current/implementation-plan.toml docs/agent-context/current/task-list.toml docs/agent-context/current/walkthrough.toml docs/agent-context/plan.toml && git commit -m "Context: start phase 110"

Ran terminal command: git push origin HEAD

Phase 110 is now active.

- Started via `exo phase start 110`
- Verified via `exo phase status`: `phase_id=110`, title “Surface Contract v1 (Docs + Vocabulary Lock)”, epoch “User-Facing Surface Coherence”
- Tasks now tracked for this phase: `110.1` / `110.2` / `110.3` (all pending)
- Committed + pushed the context initialization (“Context: start phase 110”) so the branch is clean again

What’s next in Phase 110 is to replace the stub “First Step” in the current implementation plan with real steps:

- `110.1` define the surface contract (verbs/nouns)
- `110.2` define stability labels + governance
- `110.3` update docs to match

If you want, I can draft Phase 110’s implementation steps using `exo impl clear-steps` + `exo impl add-step` so we have an actionable checklist before touching docs/code.
