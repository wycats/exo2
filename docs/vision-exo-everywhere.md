---
status: draft
last_updated: 2026-04-24
scope: exo Everywhere epoch and its descendants
---

# Vision: exo Everywhere

## Origin

Written at session-close on 2026-04-24, after Goal 1 of the `exo Everywhere` epoch's Phase 1 delivered the two-mode project-identity model. The motivation is live and documented in [RFC 10184](rfcs/stage-0/10184-project-workspace-worktree-unbundling-the-conflated-root.md): *"I haven't been able to use exo in other projects in a few weeks and I really miss it."*

This document captures the **vision, principles, and anti-goals** that shape the epoch. It exists because the work has enough mechanical depth (gitdir detection, daemon discovery, schema changes, extension ergonomics) that the user-facing outcome can drift out of view. Read this before every working session.

## The win state

I open any git repo — mine, my team's, a contractor gig, a weekend experiment — and I can adopt it into exo in one command. Whether the state commits into that repo or lives quietly in my home directory is a *policy I choose*, not a *physics I'm stuck with*.

When I want to work on a PR, I create a worktree and one or more epochs and work on it in a dedicated VS Code window. The project stays centralized around the main window; the worktree is a first-class workflow unit — its own epoch set, its own active work — but all worktrees share the same project's state, daemon, and dashboard. The agent in one worktree sees what's happening in another. When I'm done with a PR's worktree, I remove it; its epochs close cleanly and what they accomplished is still visible from the main window.

When the agent reports on its work, it speaks to me about **outcomes and artifacts**, not about process and claim IDs. "Here are the six files from today's research, look right?" — not "Acknowledge pending inbox intents." The LM-tool surface is dependable; the CLI is a fallback for the cases where a CLI is what I want. I never have to think about where exo's state lives, which daemon is serving which worktree, or whether my tools are "currently disabled by the user" despite being enabled. I just work, and exo is there.

## Principles

Five load-bearing commitments. Each resolves a real tension we've encountered.

### 1. Any git repo can be an exo project.

Require-git is the only precondition. `exo adopt` is the only command to start. No walk-up search, no path conventions, no "you need this file present first" — a user who has a repo has everything they need to begin.

### 2. Projects span worktrees; worktrees are first-class workflow units.

One gitdir = one project. All worktrees of a repo share its state, daemon, and dashboard. Worktrees are how parallel work is organized — the PR worktree, the experiment worktree, the hotfix worktree — each with its own active epochs, all visible from the main window. The project is centralized; the worktrees are its working faces.

### 3. State location is a policy decision.

Commit-in-repo (for repos I own and want the state visible in) or shadow-in-home (for repos I contribute to but don't control, or where I want state private to my machine). The choice serves the user's relationship to the repo, not exo's internals. Identity is uniform across both modes; only storage location differs.

### 4. The human surface speaks plainly.

Completion claims, steering signals, prompts, and error messages describe **what happened** and **what's next**, not **how the system represents it**. When an execute subagent finishes six tasks, the human sees six artifacts with links and one-line outcomes — not six pending inbox intents to acknowledge. Agent vocabulary (ULIDs, claim intents, phase IDs, plan-mutation events) is an implementation detail the user never has to see unless they choose to.

### 5. The LM-tool path is the product.

The CLI is a fallback for disposable or unusual cases. When the LM-tool surface flakes — "Tool exo-run is currently disabled by the user" — that's a **show-stopper** we diagnose and fix. It's not a bug class we route around with "try the CLI." If we can't make the LM-tool path dependable, we don't have a product; we have a CLI with UI features.

## Anti-goals

What `exo Everywhere` is **not** trying to do. Each of these names a plausible interpretation we've deliberately ruled out.

- **Not: run exo in any directory.** Gitless is out of scope. `git init` is the on-ramp; it's cheap (~100ms) and every repo exo could serve has git anyway.
- **Not: support multiple VCSes.** Git-only. Other VCSes can be added later if a real user pulls; not speculatively.
- **Not: become a code host.** We're a thinking-and-planning tool that lives alongside the code. GitHub-like features (review, CI, issue tracking) are orthogonal.
- **Not: optimize for first-time adoption.** Built for sustained work by people who already trust exo. Onboarding polish is downstream; the depth of the workflow is upstream.
- **Not: parity between CLI and LM-tool surfaces.** They serve different users (LM-tool serves agents working with humans; CLI serves humans working directly). Parity isn't the goal; fitness-for-surface is.
- **Not: offline-first.** The daemon is load-bearing; offline mode is a separate concern if and when it surfaces.

## What this means for the epoch

Each phase and goal should be evaluated against the principles:

- **Goal 1 (done)** — principles 1, 2, 3. Locked project identity, uniform across two state modes.
- **Goal 2 (next)** — principle 2 (multi-worktree daemon + active-epoch pinning) and principle 5 (daemon reshape must not make the LM-tool surface flakier).
- **Goal 3** — principles 1 and 3 crystallize into the Stage 1 RFC. Principle 4 shapes the "Alternatives Considered" prose — why we rejected gitless.
- **Phase 2 (queued)** — principle 4 is the whole phase. The completion-claim surface today doesn't satisfy it; this is where we fix that.

## What could go wrong

The things to watch for as we build:

1. **Mechanical drift** — getting lost in daemon protocols, schema migrations, and gitdir edge cases while forgetting the user is trying to adopt their work repo. Re-read this document before each working session.
2. **Process-vocabulary creep** — if the agent starts saying "acknowledge pending claims" to the human, principle 4 is breaking. Catch it and fix the surface, not the human.
3. **CLI-as-the-answer** — if we find ourselves saying "the LM-tool flake is fine, use the CLI," principle 5 is breaking. Stop and fix the LM-tool path.
4. **Over-generalization** — if we start designing for "any repo, any VCS, any platform," the anti-goals are breaking. Narrow back.
5. **"It's just an implementation detail"** — sometimes true (daemon port numbers), sometimes a smell for something the user will feel (daemon startup latency, LM-tool registration errors). If it affects user experience, it's in scope.

## Related

- [RFC 10184: Project / Workspace / Worktree unbundling](rfcs/stage-0/10184-project-workspace-worktree-unbundling-the-conflated-root.md) — Stage 0 idea, backbone of this epoch
- [RFC 10177: Local XDG](rfcs/stage-2/10177-local-xdg-project-scoped-directory-conventions.md) — will be superseded in Goal 3
- [vision.md](vision.md) — the broader Exosuit philosophy (this doc is scoped narrower)
