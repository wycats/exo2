<!-- exo:10062 ulid:01kmzxefebf2fwdrzqxrbhsjae -->


# RFC 10062: The Exosuit Release Lifecycle

- **Superseded by**: RFC 0037


- **Status**: Stage 0 (Draft)
- **Feature**: Process / Governance
- **Related**: RFC 0035 (EDK)

## Summary

Establish a rigorous, automated "Train Model" for releasing Exosuit libraries (EDK, Core, etc.), inspired by Ember, Rust, and Chrome. This model prioritizes stability, predictable updates, and automated migration (codemods).

## Motivation

The Exosuit Development Kit (EDK) aims to be the foundation for many extensions. If the foundation is unstable or hard to update, the ecosystem will crumble. We need a release process that:
1.  **Guarantees Stability**: Users can trust that `minor` updates won't break them.
2.  **Enables Evolution**: We can ship new features rapidly without waiting for "The Big Rewrite".
3.  **Automates Maintenance**: Updating should be a command, not a project.

## The "Train Model"

We will adopt a rhythmic release cycle (e.g., every 4-6 weeks).

### Channels

1.  **Nightly (Canary)**:
    -   Contains the latest commits.
    -   **Experimental Features**: Enabled here.
    -   **Stability**: Low. For early adopters and internal testing.
2.  **Beta**:
    -   Stabilization branch.
    -   **Experimental Features**: Disabled (unless explicitly stabilized).
    -   **Stability**: Medium. For testing upcoming releases.
3.  **Stable (Release)**:
    -   The official version.
    -   **Stability**: High. Semver guarantees apply.
4.  **LTS (Long Term Support)**:
    -   A designated Stable release (e.g., every 6th cycle) that receives critical bugfixes for a longer period (e.g., 12 months).
    -   Provides a "Stable Anchor" for the ecosystem.

## The Cybernetic Lifecycle

We augment the standard Train Model with **AI-Driven Automation** to reduce the friction of stability. We treat the lifecycle as a data problem, not a paperwork problem.

### 1. Reified Lifecycle Events (Policy as Code)
Deprecations, Features, and Changes are defined in structured data (TOML), not just comments.
-   **The Fact**: `[[deprecation]] id="DEP-0123" type="rename" from="foo" to="bar" since="1.3.0"`
-   **The Benefit**: Tooling can enforce policy (e.g., "Cannot remove X in 2.0 because it hasn't been deprecated for 2 cycles").

### 2. The Deprecation Agent
When a developer marks a feature as deprecated:
1.  **ID Assignment**: The system assigns a unique ID (e.g., `DEP-0123`).
2.  **Codemod Generation**: An AI agent analyzes the change (AST diff) and drafts a codemod to automate the migration.
3.  **Docs Draft**: The agent drafts the migration guide entry based on the PR context.

### 3. The Update Agent (`exo upgrade`)
Updating is an agentic process, not just a package bump.
-   **Analysis**: Detects the current version and target version.
-   **Execution**: Applies relevant codemods for all intermediate deprecations.
-   **Verification**: Runs tests and attempts to fix minor breakages automatically.

### 4. Synthetic Canaries
We do not rely solely on user reports.
-   **Concept**: AI agents generate "Synthetic User Apps" based on our documentation.
-   **Loop**: These apps run continuously against Nightly.
-   **Signal**: If a Nightly build breaks a Synthetic App, we know our docs or code have drifted before a human user ever sees it.

## The Stability Policy

### 1. Stability vs. Recommended
We distinguish between:
-   **Stable API**: The set of APIs that are semver-guaranteed.
-   **Recommended Set**: The "Feature Suite" that is currently considered best practice.
    -   *Example*: A new Router might be "Stable" (safe to use) but not yet "Recommended" (default in new projects).

### 2. Deprecation Workflow
We do not break things. We deprecate them.
-   **Constraint**: A feature cannot be removed in Major `N` unless it was deprecated in a Minor of `N-1`.
-   **Requirements for Deprecation**:
    -   **Replacement**: A clear alternative exists.
    -   **Guide**: Documentation on how to migrate.
    -   **ID**: A unique code (e.g., `dep-001`) linking to the guide.
    -   **Codemod**: An automated script to perform the migration (whenever possible).
-   **Staged Deprecation**: Deprecations can be "staged" via flags (e.g., `--enable-future-deprecations`) before becoming default warnings.

### 3. Majors as "Cleanup"
-   Major releases (e.g., 2.0, 3.0) are primarily for **removing deprecated code**.
-   They should not introduce massive new paradigms that weren't already available in the previous minor.
-   *Goal*: "The update to 2.0 should be boring."

### 4. Editions
We use **Editions** (e.g., "Exosuit 2025") to bundle a set of defaults, features, and recommendations for marketing and documentation purposes.
-   An Edition is a "snapshot" of the Recommended Set.
-   It helps users reason about "Modern Exosuit" without needing to track every minor version.

## Automation & Tooling

This system relies on automation, not willpower.
-   **Source Annotations**: We use code annotations (e.g., `@deprecated(since="1.4", id="dep-001")`) to drive the release process.
-   **Automated Changelogs**: Generated from these annotations and PR labels.
-   **Codemod Registry**: A central place to find and run migration scripts (`exo upgrade`).

## References
-   **Ember.js**: The "Stability without Stagnation" model.
    -   [Ember Release Cycle](https://emberjs.com/releases/)
    -   [Ember Deprecation Workflow](https://deprecations.emberjs.com/)
-   **Rust**: Editions and release trains.
    -   [Rust Release Channels](https://doc.rust-lang.org/book/appendix-07-nightly-rust.html)
    -   [Rust Editions](https://doc.rust-lang.org/edition-guide/editions/index.html)
-   **Chrome**: The evergreen browser model.
    -   [Chrome Release Cycle](https://chromium.googlesource.com/chromium/src/+/master/docs/process/release_cycle.md)
