# Display Titles Design

> Collaborative design doc for command display rendering in the VS Code chat UI.

## Display Surfaces

There are **three** display surfaces for each command invocation:

| Surface             | When                        | Source                              | Example                     |
| ------------------- | --------------------------- | ----------------------------------- | --------------------------- |
| **Collapsed title** | Before execution            | `prepareInvocation` (TS, client)    | "Completing task 'fix-bug'" |
| **Summary**         | After execution (one-liner) | `generate_summary_from_data` (Rust) | "task.complete: OK"         |
| **Body**            | After execution (rich)      | `generate_body_from_data` (Rust)    | Markdown list of tasks      |

This document focuses on the **collapsed title** (invocation message).

---

## Identity Model: Slugs

Per [RFC 0130](../rfcs/stage-3/0130-ulid-like-identifiers-ordering-projections-and-human-slugs.md),
exo artifacts carry two kinds of identity:

- **Slug**: Human-readable, stable, and conversational. Unique within a scope.
  A human can say it, remember it, and use it to refer to the entity.
- **Opaque ID**: Machine-generated (ULID, UUID, numeric). Stable and unique,
  but not something a human would say aloud.

### Slug Taxonomy

| Namespace    | ID Type   | Example            | Scope          | Slug?                                            |
| ------------ | --------- | ------------------ | -------------- | ------------------------------------------------ |
| **task**     | slug      | `fix-bug`          | per-goal       | ✅                                               |
| **goal**     | slug      | `retire-tools`     | per-phase      | ✅                                               |
| **criteria** | slug      | `tests-pass`       | per-phase      | ✅                                               |
| **axiom**    | slug      | `context-is-king`  | per-scope-file | ✅                                               |
| **rfc**      | numeric   | `00224`            | global         | ✅ auto-generated but durable and conversational |
| **epoch**    | ULID      | `01kh50k1n9...`    | global         | ❌                                               |
| **phase**    | ULID      | `01kgjbs7rb...`    | per-epoch      | ❌                                               |
| **idea**     | UUID      | `c69552da-...`     | global         | ❌                                               |
| **inbox**    | ULID      | `intent-01ke84...` | global         | ❌                                               |
| **feedback** | UUID      | `fb-{uuid}`        | global         | ❌                                               |
| **strike**   | timestamp | `strike-177...`    | global         | ❌                                               |

### Slugs as Conversational Handles

A slug isn't just a display label — it's a **conversational handle**. When a slug
appears in a collapsed title or command output, it becomes a token that both the
human and the AI can use to refer to that entity in subsequent messages:

> **AI**: _(collapsed title)_ Completing task 'fix-bug'
> **Human**: "How's 'fix-bug' going?"
> **AI** _(internally)_: `task log fix-bug` ← the slug round-trips as a command arg

This works because:

1. The slug is **human-meaningful** — you can read it and know what it refers to
2. The slug is **unambiguous within its scope** — task slugs are unique per-goal
3. The slug **round-trips** — what appears in output can be pasted back into input

**Rule**: Human narrative shows conversational handles and titles, not opaque IDs.
Structured responses retain canonical IDs, and runnable commands or structured
actions retain the exact identifier required for execution.

### Surface Identity Contract

The same operation appears through prose, structured data, and executable
actions. Each surface carries identity differently:

| Surface | Identity contract |
| --- | --- |
| Human narrative, summaries, and UI labels | Title only for opaque-ID entities; handle plus title for slug-bearing entities |
| JSON and machine-channel results | Canonical IDs remain present and stable |
| Copyable commands and structured actions | Include the exact accepted identifier needed to execute the action |
| Internal UI item IDs and cache keys | May use canonical IDs; they are not rendered as user-facing prose |

Aliases are compatibility handles. When a task handle is renamed, the new handle
is canonical in structured output and the previous handle continues to resolve.

### Slug Resolution and Scoping

Slugs work as conversational handles because commands resolve them within an
**implicit scope** — typically the active phase. This is an intentional design
choice: slug-bearing commands operate on the current context, not globally.

| Namespace    | Resolution Scope              | Implicit Context |
| ------------ | ----------------------------- | ---------------- |
| **task**     | All goals in the active phase | Active phase     |
| **goal**     | Active phase only             | Active phase     |
| **criteria** | Current `implementation-plan` | Active phase     |
| **axiom**    | Single scope file             | `--scope` (req)  |
| **rfc**      | Global `docs/rfcs/` tree      | None needed      |

**Design invariant**: Commands that accept slugs should resolve them within a
well-defined implicit scope. Avoid adding cross-scope operations on slug-bearing
entities — this would undermine the conversational handle property by making bare
slugs ambiguous. If cross-scope access is needed, require explicit scoping (e.g.
`--phase`) rather than global search.

#### Ambiguity: Within-Scope Collisions

The only live ambiguity case today is **tasks**: two goals in the active phase can
both have a task called `fix-bug`. The resolution algorithm handles this:

1. **Exact match** on the fully-qualified form `goal::task` → resolves immediately
2. **Suffix match** on the bare slug → resolves if exactly one goal has it
3. **Ambiguity error** → lists the qualified forms: `goal-a::fix-bug, goal-b::fix-bug`

When an agent hits an ambiguity error, it should retry with the qualified form.
This is rare by design (most phases have few goals, and task names tend to be
specific to their goal).

#### Ambiguity: Wrong-Scope Errors

If a slug isn't found in the default scope, the error should **not** search other
scopes. Instead, it should report "not found in active phase" and let the user or
agent decide whether to target a different scope explicitly. This prevents silent
cross-scope resolution that would make slug behavior unpredictable.

If we later add cross-scope operations, the error messages should distinguish:

- **"Ambiguous within scope"** — multiple matches, use qualified form
- **"Not found in default scope"** — suggest `--phase` or similar explicit scoping

### Display Tiers

Entities vary in what identity information is available and useful in a title.
Every entity has both an ID and a human-readable title/label field:

| Entity       | Slug           | Title/Label field | Title example                 |
| ------------ | -------------- | ----------------- | ----------------------------- |
| **task**     | `fix-bug`      | `title`           | "Fix the parser edge case"    |
| **goal**     | `retire-tools` | `label`           | "Retire delegated LM tools"   |
| **criteria** | `tests-pass`   | `description`     | "All unit tests pass"         |
| **rfc**      | `00224`        | `title`           | "The SOAR Loop"               |
| **epoch**    | _(ULID)_       | `title`           | "Epoch 1: Foundation"         |
| **phase**    | _(ULID)_       | `title`           | "LM Tool Architecture v2"     |
| **idea**     | _(UUID)_       | `title`           | "Tool-shaped guidance for..." |
| **inbox**    | _(ULID)_       | `subject`         | "Hooks should use exohook..." |

This gives us four display tiers:

| Tier               | When                                          | Format               | Example                                              |
| ------------------ | --------------------------------------------- | -------------------- | ---------------------------------------------------- |
| **slug + title**   | Entity has a slug                             | `'{slug}' ({title})` | Completing task 'fix-bug' (Fix the parser edge case) |
| **title only**     | No slug, but title exists in data             | `"{title}"`          | Starting phase "LM Tool Architecture v2"             |
| **option preview** | No slug, but a key option carries the content | `"{option value}"`   | Creating RFC "Workspace-Portable Hook Definitions"   |
| **generic**        | Nothing available at call time                | _(verb + noun)_      | Acknowledging inbox item                             |

The slug is the **handle** (what you call it). The title is the **what** (what it
means). You want both — the slug for reference, the title for recognition.

Tier assignments:

| Namespace      | Tier           | Rationale                                             |
| -------------- | -------------- | ----------------------------------------------------- |
| **task**       | slug + title   | Slug is the handle; title reminds what it means       |
| **goal**       | slug + title   | Slug is the handle; label adds useful context         |
| **criteria**   | slug + title   | Slug is the handle; description clarifies             |
| **rfc**        | slug + title   | `00224 (The SOAR Loop)` is compact and informative    |
| **axiom**      | slug + title   | Slug is in `--id` option; parse it                    |
| **epoch**      | title only     | No slug; title is the only human-readable part        |
| **phase**      | title only     | No slug; title is the only human-readable part        |
| **rfc create** | option preview | Parse `--title` — creative action, user must verify   |
| **idea add**   | option preview | Parse `--title` — creative action, user must verify   |
| **inbox add**  | option preview | Parse `--subject` — creative action, user must verify |
| **tdd new**    | option preview | Parse `--name` — user needs to confirm target task    |
| **feedback**   | generic        | Opaque ID, no useful option to preview                |
| **strike**     | generic        | Opaque timestamp                                      |

> **Note**: The "title only" tier for epoch/phase and the "slug + title" tier
> both require the server-side `display.invocation_message` to look up the
> entity's title from the result data. The client-side `prepareInvocation`
> can only show the slug (from the positional arg) — it falls back to
> slug-only or generic when the title isn't available.
>
> The "option preview" tier requires client-side parsing of `--title`,
> `--subject`, or `--name` from the command string. This is straightforward
> since the command string is available in `prepareInvocation`.
>
> **TDD red/green enrichment**: These are zero-arg commands, so the client
> shows generic titles (`Confirming test fails (RED)`). But the server knows
> which task has an active TDD cycle, so it enriches to:
> `Confirming test fails (RED) for 'fix-bug' (Fix the parser edge case)`.

### Gap: Epochs and Phases Need Slugs

Epochs and phases currently use ULIDs as their primary `id`. They can't fully
participate in the conversational handle pattern — a user can't say "start epoch
'foundation'".

RFC 0130 anticipated this (slugs alongside stable IDs), but it hasn't been
implemented for epochs/phases yet. This is a gap worth closing (separate work).
Until then, epoch/phase commands use the "title only" tier where possible.

---

## Design Principles

1. **Read like a status update**, not a CLI echo
2. **Include the slug** when the entity has one — it's the conversational handle
3. **Always pair slug with title** — the slug is the handle, the title is the reminder
4. **Use the title alone** when no slug exists but a title is available (epoch, phase)
5. **Preview creative actions** — when the AI creates content on the user's behalf
   (`rfc create`, `inbox add`, `idea add`), show what's being created
6. **Never show opaque IDs in narrative text** (ULIDs, UUIDs) — preserve them in
   structured data and executable actions instead
7. **Omit identity entirely** when the action is self-evident (list, status, finish)
8. **Natural phrasing** — "Completing task 'fix-bug'", not "Running task complete fix-bug"
9. **Singular for actions, plural for discovery** — "Completing task" (one entity),
   "Listing tasks" (many entities). This prevents drift like "Listing task" or
   "Completing tasks".
10. **Show filters on list commands** — when a list command includes filters, append
    them in parentheses: `Listing RFCs (stage 2)`, `Listing tasks (completed)`.
    The client can parse these from the command args.
11. **Invocation message = what was attempted** — on failure, the display title stays
    the same. It describes the _attempt_, not the _outcome_. Errors are communicated
    through the result body and summary fields.

## Implementation

The collapsed title is generated in `prepareInvocation` (TypeScript, client-side) by
parsing the command string to extract `(namespace, operation, first_positional_arg)`,
then applying a curated template.

The server-side `display.invocation_message` (Rust) uses the same templates but has
access to the full parsed args and can look up entity titles from the result data.
Used for the result header, not the collapsed title.

**Client-side** (prepareInvocation — before execution):

- Slug-bearing commands: include the slug from the positional arg
- Option-preview commands: parse `--title`/`--subject`/`--name` from command string
- No access to entity titles — can't look up data before execution
- Falls back to slug-only or generic when title isn't available

**Server-side** (display.invocation_message — after execution):

- Can include slug + title for all slug-bearing entities (looked up from result data)
- Can include title alone for epoch/phase
- This is the authoritative display; client-side is a best-effort preview

---

## Command Display Table

Legend:

- **Slug** — which arg (if any) appears in the title
- **Collapsed Title** — what the user sees before execution

### Root Commands

| Command        | Slug | Collapsed Title           | Notes                           |
| -------------- | ---- | ------------------------- | ------------------------------- |
| `status`       | —    | Checking project status   |                                 |
| `map`          | —    | Mapping project           |                                 |
| `update`       | —    | Applying project upgrades |                                 |
| `write {path}` | —    | Writing context file      | path is a file path, not a slug |

### task (slug + title tier)

| Command                | Slug | Collapsed Title                        | Notes                                |
| ---------------------- | ---- | -------------------------------------- | ------------------------------------ |
| `task list`            | —    | Listing tasks                          |                                      |
| `task add {slug}`      | slug | Adding task '{slug}'                   | title not yet known at call time     |
| `task start {slug}`    | slug | Starting task '{slug}' ({title})       | server-side includes title from data |
| `task complete {slug}` | slug | Completing task '{slug}' ({title})     | server-side includes title from data |
| `task remove {slug}`   | slug | Removing task '{slug}'                 |                                      |
| `task update {slug}`   | slug | Updating task '{slug}'                 |                                      |
| `task log {slug}`      | slug | Logging progress on '{slug}' ({title}) | server-side includes title from data |
| `task reorder {slug}`  | slug | Reordering task '{slug}'               | position omitted                     |
| `task rename {slug}`   | slug | Renaming task '{slug}' ({title})       | result reports the new canonical handle |

### goal (slug + title tier)

| Command                | Slug | Collapsed Title                    | Notes                                |
| ---------------------- | ---- | ---------------------------------- | ------------------------------------ |
| `goal list`            | —    | Listing goals                      |                                      |
| `goal add {slug}`      | slug | Adding goal '{slug}'               | title not yet known at call time     |
| `goal complete {slug}` | slug | Completing goal '{slug}' ({label}) | server-side includes label from data |
| `goal abandon {slug}`  | slug | Abandoning goal '{slug}' ({label}) | server-side includes label from data |
| `goal remove {slug}`   | slug | Removing goal '{slug}'             |                                      |
| `goal update {slug}`   | slug | Updating goal '{slug}'             | new label omitted                    |
| `goal reorder {slug}`  | slug | Reordering goal '{slug}'           | position omitted                     |

### criteria (slug + title tier)

| Command                     | Slug | Collapsed Title                          | Notes                               |
| --------------------------- | ---- | ---------------------------------------- | ----------------------------------- |
| `criteria list`             | —    | Listing acceptance criteria              |                                     |
| `criteria add {slug}`       | slug | Adding criterion '{slug}'                | description not yet known           |
| `criteria remove {slug}`    | slug | Removing criterion '{slug}'              |                                     |
| `criteria satisfy {slug}`   | slug | Satisfying criterion '{slug}' ({desc})   | server-side includes desc from data |
| `criteria unsatisfy {slug}` | slug | Unsatisfying criterion '{slug}' ({desc}) | server-side includes desc from data |

### axiom (slug + title tier, slug from --id option)

| Command        | Slug | Collapsed Title         | Notes                          |
| -------------- | ---- | ----------------------- | ------------------------------ |
| `axiom list`   | —    | Listing axioms          |                                |
| `axiom add`    | slug | Adding axiom '{slug}'   | parse --id from command string |
| `axiom remove` | slug | Removing axiom '{slug}' | parse --id from command string |

### rfc (slug + title tier)

| Command             | Slug | Collapsed Title              | Notes                                    |
| ------------------- | ---- | ---------------------------- | ---------------------------------------- |
| `rfc list`          | —    | Listing RFCs                 | with `--stage`: "Listing RFCs (stage 2)" |
| `rfc create`        | —    | Creating RFC "{title}"       | option preview: parse --title            |
| `rfc show {id}`     | id   | Showing RFC {id} ({title})   | server-side includes title from data     |
| `rfc promote {id}`  | id   | Promoting RFC {id} ({title}) | server-side includes title from data     |
| `rfc edit`          | —    | Editing RFC                  |                                          |
| `rfc edit {id}`     | id   | Editing RFC {id}             |                                          |
| `rfc withdraw {id}` | id   | Withdrawing RFC {id}         |                                          |
| `rfc archive {id}`  | id   | Archiving RFC {id}           |                                          |
| `rfc rename {id}`   | id   | Renaming RFC {id}            |                                          |
| `rfc supersede`     | —    | Superseding RFC              | complex args                             |
| `rfc status`        | —    | Checking RFC status          |                                          |

### phase (title only tier — no slug)

| Command                 | Slug | Collapsed Title               | Notes                             |
| ----------------------- | ---- | ----------------------------- | --------------------------------- |
| `phase start`           | —    | Starting next phase           | server: Starting phase "{title}"  |
| `phase finish`          | —    | Finishing phase               | server: Finishing phase "{title}" |
| `phase status`          | —    | Checking phase status         |                                   |
| `phase add`             | —    | Adding phase                  |                                   |
| `phase remove`          | —    | Removing phase                | ID is ULID — omit                 |
| `phase update`          | —    | Updating phase                | ID is ULID — omit                 |
| `phase history`         | —    | Showing phase history         |                                   |
| `phase execution.tasks` | —    | Listing phase execution tasks |                                   |

### epoch (title only tier — no slug)

| Command          | Slug | Collapsed Title       | Notes                             |
| ---------------- | ---- | --------------------- | --------------------------------- |
| `epoch list`     | —    | Listing epochs        |                                   |
| `epoch start`    | —    | Starting epoch        | server: Starting epoch "{title}"  |
| `epoch finish`   | —    | Finishing epoch       | server: Finishing epoch "{title}" |
| `epoch add`      | —    | Adding epoch          |                                   |
| `epoch remove`   | —    | Removing epoch        | ID is ULID — omit                 |
| `epoch status`   | —    | Checking epoch status |                                   |
| `epoch bankrupt` | —    | Bankrupting epoch     | ID is ULID — omit                 |
| `epoch review`   | —    | Reviewing epoch       | ID is ULID — omit                 |

### tdd

| Command     | Slug | Collapsed Title                 | Notes                                   |
| ----------- | ---- | ------------------------------- | --------------------------------------- |
| `tdd new`   | —    | Starting TDD cycle for '{name}' | option preview: parse --name            |
| `tdd red`   | —    | Confirming test fails (RED)     | server: includes active task slug+title |
| `tdd green` | —    | Confirming test passes (GREEN)  | server: includes active task slug+title |

### idea (opaque UUID — no slug)

| Command        | Slug | Collapsed Title        | Notes                                       |
| -------------- | ---- | ---------------------- | ------------------------------------------- |
| `idea list`    | —    | Listing ideas          | with `--status`: "Listing ideas (archived)" |
| `idea add`     | —    | Adding idea "{title}"  | option preview: parse --title               |
| `idea archive` | —    | Archiving idea         | ID is UUID — omit                           |
| `idea to-rfc`  | —    | Converting idea to RFC | ID is UUID — omit                           |

### inbox (opaque ULID — no slug)

| Command         | Slug | Collapsed Title               | Notes                           |
| --------------- | ---- | ----------------------------- | ------------------------------- |
| `inbox list`    | —    | Listing inbox                 |                                 |
| `inbox add`     | —    | Adding inbox item "{subject}" | option preview: parse --subject |
| `inbox ack`     | —    | Acknowledging inbox item      | ID is ULID — omit               |
| `inbox archive` | —    | Archiving inbox item          | ID is ULID — omit               |
| `inbox resolve` | —    | Resolving inbox item          | ID is ULID — omit               |

### commit

| Command         | Slug | Collapsed Title     | Notes                     |
| --------------- | ---- | ------------------- | ------------------------- |
| `commit create` | —    | Creating commit     | --message has the content |
| `commit status` | —    | Checking git status |                           |

### strike (opaque timestamp — no slug)

| Command         | Slug | Collapsed Title           | Notes               |
| --------------- | ---- | ------------------------- | ------------------- |
| `strike start`  | —    | Starting surgical strike  | --name has the name |
| `strike finish` | —    | Finishing surgical strike |                     |
| `strike abort`  | —    | Aborting surgical strike  |                     |

### plan

| Command              | Slug | Collapsed Title           | Notes                       |
| -------------------- | ---- | ------------------------- | --------------------------- |
| `plan review`        | —    | Reviewing plan            |                             |
| `plan health`        | —    | Checking plan health      |                             |
| `plan linearize`     | —    | Linearizing phase numbers |                             |
| `plan migrate-ids`   | —    | Migrating to ULID IDs     |                             |
| `plan update-status` | —    | Updating plan item status | ID could be anything — omit |

### verify

| Command      | Slug | Collapsed Title      | Notes |
| ------------ | ---- | -------------------- | ----- |
| `verify run` | —    | Running verification |       |

### feedback (opaque UUID — no slug)

| Command                  | Slug | Collapsed Title                 | Notes |
| ------------------------ | ---- | ------------------------------- | ----- |
| `feedback threads`       | —    | Listing feedback threads        |       |
| `feedback thread.create` | —    | Creating feedback thread        |       |
| `feedback thread.reply`  | —    | Replying to feedback thread     |       |
| `feedback thread.status` | —    | Updating feedback thread status |       |

### context

| Command           | Slug | Collapsed Title       | Notes |
| ----------------- | ---- | --------------------- | ----- |
| `context paths`   | —    | Showing context paths |       |
| `context restore` | —    | Restoring context     |       |

### ai

| Command            | Slug | Collapsed Title         | Notes                          |
| ------------------ | ---- | ----------------------- | ------------------------------ |
| `ai context`       | —    | Dumping AI context      |                                |
| `ai prompt {name}` | name | Getting prompt '{name}' | name is a slug-like identifier |
| `ai chat-history`  | —    | Reading chat history    |                                |

### gc

| Command    | Slug | Collapsed Title   | Notes |
| ---------- | ---- | ----------------- | ----- |
| `gc inbox` | —    | Cleaning up inbox |       |

### run (task runner)

| Command           | Slug | Collapsed Title        | Notes                           |
| ----------------- | ---- | ---------------------- | ------------------------------- |
| `run task {slug}` | slug | Running task '{slug}'  | task runner slug, not task slug |
| `run tasks`       | —    | Listing runnable tasks |                                 |

### json (internal)

| Command              | Slug | Collapsed Title          | Notes |
| -------------------- | ---- | ------------------------ | ----- |
| `json spec`          | —    | Generating command spec  |       |
| `json schema`        | —    | Generating JSON schema   |       |
| `json lm-tools`      | —    | Generating LM tools      |       |
| `json package-tools` | —    | Generating package tools |       |
| `json artifact`      | —    | Generating artifact      |       |
| `json read`          | —    | Reading JSON             |       |
| `json write`         | —    | Writing JSON             |       |

### toml (internal)

| Command      | Slug | Collapsed Title | Notes |
| ------------ | ---- | --------------- | ----- |
| `toml read`  | —    | Reading TOML    |       |
| `toml write` | —    | Writing TOML    |       |

### docs

| Command            | Slug | Collapsed Title    | Notes |
| ------------------ | ---- | ------------------ | ----- |
| `docs links.check` | —    | Checking doc links |       |
| `docs links.fix`   | —    | Fixing doc links   |       |

---

## Resolved Questions

1. **`axiom add/remove`**: ✅ Parse `--id` from command string. Show slug in title.

2. **`tdd new --name`**: ✅ Parse `--name`. Show "Starting TDD cycle for '{name}'".

3. **`rfc create --title`**: ✅ Parse `--title`. Show "Creating RFC \"{title}\"".
   Creative actions need content preview — user must verify AI captured intent.

4. **`inbox add --subject`**: ✅ Parse `--subject`. Same reasoning as rfc create.

5. **`idea add --title`**: ✅ Parse `--title`. Same reasoning.

6. **Fallback**: ✅ For any unrecognized command, fall back to
   "Running {namespace} {operation}".

## Open Questions

1. **Epoch/phase slug gap**: These namespaces can't participate in the conversational
   handle pattern until they get user-facing slugs. Track as a separate RFC or task?

2. **Title truncation**: Cap display titles at ~60 characters. Truncation rules:
   - **Slug + title**: Slug is always shown in full (short by convention).
     Truncate the parenthetical title with "…":
     `Completing task 'fix-bug' (Fix the parser edge…)`
   - **Title only / option preview**: Truncate the quoted string with "…":
     `Creating RFC "Display Title Truncation Str…"`
   - Truncation happens at the display generation layer (both client and server).
   - Needs validation with real VS Code chat UI widths to confirm the ~60 char target.

---

## Appendix A: VS Code API Surface Findings

Investigation of the VS Code `LanguageModelTool` API revealed several capabilities
beyond the basic `invocationMessage` string that significantly affect this design.

### A.1: `MarkdownString` in Collapsed Titles

`invocationMessage` accepts `string | MarkdownString`. When a `MarkdownString` is
provided (with `supportThemeIcons: true`), the collapsed title supports:

- **Inline code**: `` Completing task `fix-bug` ``
- **Bold/italic**: `**Completing** task 'fix-bug'`
- **Theme icons**: `$(check) Completed task 'fix-bug'`
- **Command links** (if `isTrusted`): clickable links to VS Code commands

This is used in production by MCP tools and VS Code's built-in `UsagesTool`:

```typescript
invocationMessage: localize(
  "tool.usages.invocationMessage",
  "Analyzing usages of `{0}`",
  input.symbol,
);
```

**Design implication**: We should use `MarkdownString` with theme icons for all
display titles. Inline code for slugs (`` `fix-bug` ``) gives them visual
distinction. Icons can reflect the operation type or result status.

### A.2: `pastTenseMessage` — Post-Completion Title Update

The internal `IPreparedToolInvocation` interface includes:

```typescript
interface IPreparedToolInvocation {
  invocationMessage?: string | IMarkdownString; // shown while running
  pastTenseMessage?: string | IMarkdownString; // shown after completion
  originMessage?: string | IMarkdownString; // subtitle (e.g. source)
  // ...
}
```

The collapsed title transitions from `invocationMessage` to `pastTenseMessage` when
the tool completes. This enables:

- **Verb tense shift**: "Completing task..." → "Completed task..."
- **Result icons**: `$(loading~spin)` while running → `$(check)` on success, `$(error)` on failure
- **Result reflection**: The title can change to reflect what actually happened

**Status**: Behind the `chatParticipantPrivate` proposed API. Not available in
stable VS Code — only in Insiders with the proposed API flag enabled.

**Plan**: Build a progressive enhancement mechanism that enables `pastTenseMessage`
when running in VS Code Insiders, with a build-time flag to strip proposed API
usage from marketplace builds targeting stable VS Code. The experience degrades
gracefully — stable users see the `invocationMessage` permanently, Insiders users
get the tense shift and result icons.

Example of the enhanced experience:

| Phase   | Collapsed Title                                                          |
| ------- | ------------------------------------------------------------------------ |
| Running | `$(loading~spin) Completing task \`fix-bug\` (Fix the parser edge case)` |
| Success | `$(check) Completed task \`fix-bug\` (Fix the parser edge case)`         |
| Error   | `$(error) Failed to complete task \`fix-bug\``                           |

### A.3: `confirmationMessages` — User Confirmation for Destructive Operations

`prepareInvocation` can return `confirmationMessages` to show a confirmation dialog
before `invoke()` runs. If the user clicks "Cancel", `invoke()` is never called.

```typescript
return {
  invocationMessage: "Removing task 'fix-bug'",
  confirmationMessages: {
    title: "Remove task 'fix-bug'?",
    message: new MarkdownString(
      "This will permanently remove the task and its log history.",
    ),
  },
};
```

**Key properties**:

- **Fully dynamic**: The decision to show confirmation is per-invocation, based on
  the parsed input. No static declaration required. `task remove` shows confirmation;
  `task list` doesn't — same tool, different behavior.
- **Supports MarkdownString**: Both `title` and `message` can use markdown formatting.
- **Interacts with approval system**: VS Code's tool approval settings layer on top.
  If the user has auto-approved a tool (per-session, per-workspace, or via settings),
  the confirmation dialog is skipped even if `confirmationMessages` is set.

**Candidate commands for confirmation**:

| Command               | Rationale                         |
| --------------------- | --------------------------------- |
| `task remove {slug}`  | Permanent deletion                |
| `goal remove {slug}`  | Permanent deletion                |
| `goal abandon {slug}` | Irreversible state change         |
| `rfc withdraw {id}`   | Irreversible state change         |
| `epoch bankrupt`      | Drastic recovery action           |
| `phase finish`        | Commits and closes — hard to undo |
| `strike start`        | Enters a special mode             |

**Implementation**: Since `prepareInvocation` will already call `Op::Preview` to get
the display title, the server can also return a `confirmation` field in the preview
response, indicating whether this invocation should show a confirmation dialog and
what the title/message should be. This keeps the confirmation logic in Rust alongside
the display title logic.

### A.4: Architecture — `Op::Preview` for Server-Side Display Generation

The core architectural decision: **all display title logic lives in Rust**. The
TypeScript client is just a pipe.

`prepareInvocation` is async (`ProviderResult<T>` = `T | Thenable<T>`), so it can
call the machine channel server before returning. We add a new `Op::Preview` to the
protocol:

```
Op::Preview(CallParams)  →  { display: Display, confirmation?: Confirmation }
```

`Preview` takes the same `CallParams` as `Call` but does not execute the command.
It parses the args, applies the tier-based template, and optionally does a cheap
state read (e.g., task title from implementation-plan TOML) to enrich the title.

**Pre-execution state reads**: Since `Preview` runs before execution, it can read
current state to look up entity titles. Task titles live in the implementation-plan
TOML, which is already loaded. This is a read-only file access with no side effects,
consistent with `prepareInvocation`'s "must be free of side-effects" contract.

This means the collapsed title can show the full slug+title even before execution:
`Completing task 'fix-bug' (Fix the parser edge case)` — not just the slug.

The title-only tier (epoch/phase) also benefits: `Starting phase "LM Tool
Architecture v2"` instead of the vague `Starting next phase`.

**Flow**:

```
prepareInvocation (async)
  → Op::Preview to server (JSON/stdio, fast)
  → server: parse args, apply template, read state if needed
  ← { display, confirmation? }
  → return PreparedToolInvocation {
      invocationMessage: display.invocation_message,
      pastTenseMessage: display.past_tense_message,  // Insiders only
      confirmationMessages: confirmation,              // if destructive
    }

invoke
  → Op::Call to server (executes command)
  ← ResponseEnvelope { result, display, steering }
  → return LanguageModelToolResult with display.body ?? display.summary
```
