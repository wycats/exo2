<!-- exo:236 ulid:01kmzxefewq2910r38z351e05k -->

# RFC 236: Resource Projections: Exposing Reactive State as Agent-Discoverable Symbols


# RFC 00236: Resource Projections: Exposing Reactive State as Agent-Discoverable Symbols

## Summary

Introduce **resource projections** — a thin mapping layer that connects computed roots to VS Code's workspace symbol and virtual document APIs, making plan entities discoverable and readable by AI agents in chat.

A resource projection says: "this computed root produces these resources, and here's what the agent should see when it reads one."

The name deliberately echoes MCP's _resources_ (URI-addressable, read-only context for agents). Today the transport is VS Code's symbol and content-provider APIs; tomorrow it could be MCP's `resources/list` and `resources/read`. The registry's contract stays the same.

## Concepts

This RFC introduces one new concept and builds on two existing ones:

| Concept                               | Layer      | Role                                                         |
| ------------------------------------- | ---------- | ------------------------------------------------------------ |
| **Computed root** (exists, RFC 00188) | Data       | Reactive cached value with auto-tracked dependencies         |
| **Resource projection** (new)         | Mapping    | Shapes a computed root into discoverable, readable resources |
| **`exo://` URI** (new)                | Addressing | Stable identifier for a projected resource                   |

The VS Code providers (`WorkspaceSymbolProvider`, `TextDocumentContentProvider`) are implementation details — they're how resource projections are delivered, not concepts in our domain.

## Motivation

### The Problem

Today, agents have no structured way to discover or reference plan entities (phases, goals, RFCs, ideas). They rely on:

1. **Tool calls** (`exo-status`, `exo-phase`) — requires the agent to know the tool exists and decide to call it
2. **File reads** — requires knowing which TOML file to read and how to parse it
3. **Semantic search** — unreliable for structured data

Meanwhile, VS Code's workspace symbol system (`#` picker, `search_workspace_symbols` tool) gives agents a universal mechanism for discovering and referencing entities. But we don't participate in it.

### The Insight

We already have **computed roots** (RFC 00188) that maintain reactive, cached, trace-validated views of plan state:

- `computed:phase.details` — full phase hierarchy with goals, tasks, progress
- `computed:rfc.index` — all RFCs with metadata by stage
- `computed:inbox.summary` — active/pending counts
- `computed:ideas.summary` — idea list with metadata

These are exactly the entities we want agents to discover. What's missing is the mapping from "reactive cached value" to "agent-discoverable symbol with readable content."

### What "Discoverable" Means, Concretely

When we register a workspace symbol, two things happen:

1. **The `#` picker and `search_workspace_symbols` tool** can find it — the agent sees `Symbol: <name>, containing symbol: <container>` plus a `PromptReference` to the symbol's location
2. **The content at the symbol's location is read and sent to the agent** — VS Code calls `workspace.openTextDocument(uri)`, which triggers our content provider, and includes the text verbatim in the prompt

So a resource projection must produce both: **(a)** symbol metadata for discovery, and **(b)** rendered text content for reading.

## Design

### Resource Projection

A **resource projection** maps a single computed root to a **resource document** — a structured document whose sections naturally define both the symbols and their content. Symbols are a consequence of document structure, not a parallel data path.

```
computed root ──→ resource projection ──→ ResourceDocument
                                              │
                                     ┌────────┴────────┐
                                     │                 │
                               sections → symbols   content → TextDocumentContentProvider
                                     │
                              WorkspaceSymbolProvider
```

The projection author writes one function that produces a structured document. The `ResourceRegistry` derives both the symbol metadata (for discovery) and the rendered content (for reading) from that document.

### The `exo://` URI Scheme

Each projected resource needs a stable, addressable URI:

```
exo://phase/whiteboard-spike
exo://goal/symbol-spike
exo://rfc/00235
exo://idea/some-idea-id
```

The scheme is registered with a `TextDocumentContentProvider`. When VS Code opens `exo://rfc/00235`, the content provider:

1. Parses the URI to determine which resource projection and entity
2. Calls `computedRootRegistry.get()` — returns cached if valid, recomputes if stale
3. Calls the projection's `renderContent()` with the computed value and URI
4. Returns the rendered text

### ResourceRegistry

A single service that:

- Holds all resource projection registrations
- Implements `WorkspaceSymbolProvider` (discovery)
- Implements `TextDocumentContentProvider` (content serving)
- Delegates to computed roots for data and freshness

```typescript
interface ResourceProjection<T> {
  /** Which computed root this projection reads */
  computedRoot: string;

  /** Produce a structured document from the computed value */
  project(value: T): ResourceDocument;
}

interface ResourceDocument {
  /** Top-level sections, each of which becomes a symbol */
  sections: ResourceSection[];
}

interface ResourceSection {
  /** Symbol name (dot-qualified for discoverability) */
  name: string;
  /** Maps to VS Code SymbolKind for global/local filtering */
  kind: SymbolKind;
  /** Display-only context shown beneath the symbol */
  containerName: string;
  /** Stable URI for this resource: exo://{kind}/{id} */
  uri: Uri;
  /** The rendered content the agent will read */
  content: string;
  /** Nested sections (optional — future use for range-based references) */
  children?: ResourceSection[];
}
```

Each `ResourceSection` unifies what were previously separate concerns: `name`/`kind`/`containerName`/`uri` define the symbol, and `content` defines what the agent reads when it opens that URI. One data structure, one function, no sync risk.

### Registration Example

```typescript
resourceRegistry.register<PhaseDetails>({
  computedRoot: PHASE_DETAILS_ROOT_ID,

  project(details) {
    return {
      sections: [
        // The phase itself
        {
          name: `phase.${details.phaseId}`,
          kind: SymbolKind.Module,
          containerName: details.epochTitle,
          uri: Uri.parse(`exo://phase/${details.phaseId}`),
          content: [
            `# Phase: ${details.title}`,
            `Epoch: ${details.epochTitle}`,
            `Status: ${details.status}`,
            ``,
            `## Goals`,
            ...details.goals.map(
              (g) =>
                `- [${g.status}] ${g.label} (${g.completedTasks}/${g.totalTasks} tasks)`,
            ),
          ].join("\n"),
        },

        // Each goal in the current phase
        ...details.goals.map((goal) => ({
          name: `goal.${goal.id}`,
          kind: SymbolKind.Interface,
          containerName: details.title,
          uri: Uri.parse(`exo://goal/${goal.id}`),
          content: [
            `# Goal: ${goal.label}`,
            `Phase: ${details.title}`,
            `Status: ${goal.status}`,
            ``,
            `## Tasks`,
            ...goal.tasks.map((t) => `- [${t.status}] ${t.label}`),
          ].join("\n"),
          // Tasks for in-progress goals only
          children:
            goal.status === "in-progress"
              ? goal.tasks.map((task) => ({
                  name: `task.${task.id}`,
                  kind: SymbolKind.Property,
                  containerName: goal.label,
                  uri: Uri.parse(`exo://task/${task.id}`),
                  content: [
                    `# Task: ${task.label}`,
                    `Goal: ${goal.label}`,
                    `Status: ${task.status}`,
                  ].join("\n"),
                }))
              : undefined,
        })),
      ],
    };
  },
});
```

Note how task emission is controlled by goal status — tasks only appear as symbols when their parent goal is in-progress. This is a natural consequence of writing `project()` as a function of state, not a separate activation API.

### SymbolKind Mapping

Based on VS Code's "global symbol" filter (which always shows Class, Enum, File, Interface, Namespace, Package, Module regardless of settings):

| Entity     | SymbolKind | containerName | Global? | Rationale                   |
| ---------- | ---------- | ------------- | ------- | --------------------------- |
| Epoch      | Namespace  | —             | ✅      | Top-level grouping          |
| Phase      | Module     | Epoch title   | ✅      | Unit of work within epoch   |
| RFC        | File       | Stage label   | ✅      | Document-like entity        |
| Goal       | Property   | Phase title   | ❌      | Scoped to a phase           |
| Idea       | Variable   | "Ideas"       | ❌      | Lightweight backlog item    |
| Inbox item | Event      | Subject ref   | ❌      | Time-sensitive notification |

Goals and ideas use kinds outside the "global" set, meaning they're filtered when the user has `search.quickOpen.includeSymbols` in local-skip mode. This is arguably correct — goals and ideas are "local" to a phase/backlog, while phases, epochs, and RFCs are "global" project entities.

### Query Handling

`provideWorkspaceSymbols(query)` is called with whatever the user or agent typed. Our implementation:

1. Calls `computedRootRegistry.get()` for each registered resource projection (cheap — returns cached unless stale)
2. Calls `project()` on each projection's value, walks the resulting `ResourceDocument` sections (and children)
3. Returns `SymbolInformation` entries derived from sections — VS Code handles fuzzy matching, scoring, and deduplication

For empty queries (`""`), we return all symbols. The `search_workspace_symbols` tool caps at 20 results anyway, and VS Code's dedup sorts by name → kind → URI.

### Content Freshness

The content provider calls `computedRootRegistry.get()`, which recomputes if the trace is stale. Content is always consistent with current source files — no explicit invalidation logic needed in the resource projection layer.

The computed root layer handles:

- Dependency tracking (plan.toml change → computed:phase.details marked stale)
- Lazy recomputation (only when actually queried)
- Trace validation (no unnecessary recomputation)
- Error caching

## What Each Layer Provides

| Concern                      | Provided by                         |
| ---------------------------- | ----------------------------------- |
| Dependency tracking          | Computed roots (RFC 00188)          |
| Lazy recomputation           | Computed roots                      |
| Trace validation             | Computed roots                      |
| Agent discovery              | VS Code WorkspaceSymbolProvider     |
| Human discovery              | `chatContextProvider` (proposed)    |
| Human fallback discovery     | LM Tools (zero-arg reads)           |
| Fuzzy search & ranking       | VS Code symbol search               |
| Content serving              | VS Code TextDocumentContentProvider |
| Document lifecycle           | VS Code virtual document model      |
| Data → structured document   | **Resource projections** (this RFC) |
| Document → symbols + content | **ResourceRegistry** (this RFC)     |

The resource projection layer is deliberately thin — ~200 lines for the registry, ~30–80 lines per individual projection (just a `project()` function returning structured data). The heavy lifting is done by computed roots (reactivity) and VS Code (discovery + serving).

## Transport Strategy

Resource projections produce structured documents. But how those documents reach agents and humans depends on the **transport layer** — and different transports serve different consumers.

### The Three Transports

| Transport                 | API Status   | Consumer                 | Discovery Style                                      |
| ------------------------- | ------------ | ------------------------ | ---------------------------------------------------- |
| WorkspaceSymbolProvider   | **Stable**   | Agent tools              | `search_workspace_symbols` tool, command palette `#` |
| LM Tools (zero-arg reads) | **Stable**   | Human `#tool` attachment | `#exo-status`, `#exo-phase`, etc.                    |
| `chatContextProvider`     | **Proposed** | Human chat `#` picker    | Native chat context attachment                       |

### Why Three Transports?

The critical discovery: **VS Code's chat `#` picker does NOT use `WorkspaceSymbolProvider`**. The chat input's `#` completion menu draws from a separate system — it shows open editor document symbols (from the `DocumentSymbol` cache, LRU capped at 15 editors), tools, and eventually chat context providers. Workspace symbols only appear in the **command palette** `#` mode and in the **`search_workspace_symbols`** agent tool.

This means:

- **Agents** can discover our resources via `search_workspace_symbols` — which calls `executeWorkspaceSymbolProvider`, takes `slice(0, 20)`, and does NO fuzzy matching (passes the raw query to providers, so we control matching entirely)
- **Humans** typing `#` in chat will NOT see our resources through WorkspaceSymbolProvider alone

The three transports cover three scenarios:

1. **Agent discovery** (WorkspaceSymbolProvider): Agent asks `search_workspace_symbols("phase")`, gets our symbols, reads content via `exo://` URI. This is the primary machine-to-machine path. Stable API, works today.

2. **Human attachment fallback** (LM Tools): Until `chatContextProvider` stabilizes, humans can still attach plan context via `#exo-status`, `#exo-phase`, etc. These are the existing zero-arg tools. They appear in the chat `#tool` category. Not ideal — they're unstructured and don't compose — but they work on stable APIs.

3. **Human-native discovery** (`chatContextProvider`): The ideal human path. Resources appear directly in the chat `#` picker as first-class context items. The human types `#phase` and sees it alongside files and tools, attaches it, and the rendered content flows into the prompt.

### `chatContextProvider` (Proposed API)

VS Code issue [#271104](https://github.com/microsoft/vscode/issues/271104) ("Chat: Support contributable chat context resources") introduces `chatContextProvider` — a proposed API that lets extensions contribute context items to the chat input.

**Status**: Milestone **February 2026** (this month). Assigned to alexr00, filed by joaomoreno. PR [#289349](https://github.com/microsoft/vscode/pull/289349) just landed, extending support from webviews to text editors. alexr00 noted: "There will be one more big change to the API this week as I break it into 3 separate providers."

**API shape** (current, subject to change):

```typescript
vscode.chat.registerChatContextProvider(
  selector: DocumentSelector,  // e.g. { scheme: 'exo' }
  options: { providesList: boolean },
  provider: ChatContextProvider
);

interface ChatContextProvider {
  provideChatContextForResource(
    resource: Uri,
    language: string | undefined,
    token: CancellationToken
  ): ProviderResult<ChatContext>;
}

interface ChatContext {
  items: ChatContextItem[];
}

interface ChatContextItem {
  icon: ThemeIcon;
  label: string;
  description: string;
  value: string | Thenable<string>;  // The content sent to the agent
}
```

**Key architectural fit**: The API uses `DocumentSelector`, which matches URIs by scheme. We register for `{ scheme: 'exo' }`, and every `exo://` resource becomes a chat context item. The `ResourceDocument` content we already produce for `TextDocumentContentProvider` is exactly the `value` string the chat context item needs. Same data, different transport.

**The three-provider split** (alexr00's planned refactor):

1. **Workspace context** — auto-attaches to every chat request (no user action needed). Could inject `#status` automatically.
2. **Explicit context** — shown in the explicit context UI / `#` picker. This is the primary human discovery path.
3. **Resource context** — context for a specific resource (e.g., when an `exo://` document is the active editor).

### Transport Selection Strategy

```
                         ┌───────────────────────────────┐
                         │     ResourceProjection         │
                         │     project() → ResourceDoc    │
                         └──────────┬────────────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    │               │               │
                    ▼               ▼               ▼
         ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
         │ Workspace     │  │ LM Tools     │  │ chatContext   │
         │ SymbolProvider│  │ (fallback)   │  │ Provider      │
         │               │  │              │  │               │
         │ Agent tools,  │  │ Human #tool  │  │ Human # picker│
         │ cmd palette # │  │ attachment   │  │ (proposed)    │
         │               │  │              │  │               │
         │ STABLE        │  │ STABLE       │  │ PROPOSED      │
         └──────────────┘  └──────────────┘  └──────────────┘
```

All three transports read from the same `ResourceDocument`. The projection is written once; the transports are wiring.

### Migration Path

1. **Steel thread spike**: Implement WorkspaceSymbolProvider + TextDocumentContentProvider. Validate agent discovery. Keep existing LM tools as human fallback.
2. **Insiders build**: Implement `chatContextProvider` behind `vscode.proposed.chatContextProvider.d.ts`. Validate human discovery on VS Code Insiders.
3. **chatContextProvider stabilizes**: Once the API moves from proposed to stable, remove the zero-arg LM tools. The human path and agent path are both covered by resource projections.
4. **Upstream engagement**: Participate in [#271104](https://github.com/microsoft/vscode/issues/271104) to represent our use case — extension-defined virtual resources as chat context. Our `exo://` scheme is a concrete example of what the API enables beyond webviews and text editors.

### What This Means for Tool Deprecation

The original plan was: implement resource projections → immediately remove zero-arg tools. The transport discovery refines this:

- **Agent-facing tools** (`exo-status`, `exo-phase`, etc. called programmatically): Can be removed once WorkspaceSymbolProvider is validated. Agents use `search_workspace_symbols` instead.
- **Human-facing tools** (`#exo-status`, `#exo-phase` attached in chat): Keep as fallback until `chatContextProvider` stabilizes. Removing them before the human path is covered would be a regression.

This is a nuance, not a reversal. The destination is the same: zero read-only tools. The transport strategy just sequences the migration to avoid a gap in human discoverability.

## Resource Catalog

This section works through what the resource surface should look like: what entities become resources, how they're named, when they're active, and what the naming conventions reveal about organizational gaps upstream.

### Design Principle: Constraint as Lens

If an entity can't be found via natural typing in symbol search, that's a signal the entity itself is poorly organized — not just poorly presented. The symbol system is a forcing function for conceptual clarity.

Concretely: if we have 80 RFCs and no one can find anything by typing 3-4 characters, the problem isn't the symbol count — it's that RFCs lack meaningful grouping. The `feature` frontmatter field already exists but isn't used as a primary organizing axis. The symbol constraint surfaces this.

### Naming Convention

Since `containerName` is **display-only** (shown beneath the symbol in the picker as `containerName • path`, rendered in tool output as `containing symbol: <containerName>`, but **never searched**), all discoverability lives in the `name` field.

We use **dot-qualified names** that mirror how code symbols work:

```
status                                ← singleton
steering                              ← singleton
phase.whiteboard-spike                ← current phase
goal.symbol-spike                     ← goal in current phase
task.rename-derived-to-computed       ← in-progress task

rfc.symbols.00236.resource-projections  ← RFC (by feature category)
rfc.workflow.00224.soar-loop            ← RFC (by feature category)

idea.reactivity.file-watcher          ← idea (by category)
idea.workflow.soar-metrics            ← idea (by category)

inbox.bug.phase-transition            ← inbox item (by type)
inbox.feature.new-tool-request        ← inbox item (by type)
```

**Why this works with fuzzy search:**

- `#status` → exact match on singleton
- `#goal` → matches all goals (all start with `goal.`)
- `#rfc.236` → high-score match (consecutive characters through the dot)
- `#rfc symb` → finds RFCs in the `symbols` feature category
- `#idea react` → finds reactivity-related ideas
- `#inbox bug` → finds bug inbox items
- `#task` → finds in-progress tasks

The dots aren't special to VS Code's fuzzy matcher — they're just characters. But they create visual structure for humans scanning results, and the segments naturally act as search terms because fuzzy matching scores adjacent character matches higher than gapped ones.

### Entity Catalog

#### Singletons (Always Active)

These replace the current zero-arg read-only tools:

| Resource     | Replaces      | SymbolKind | containerName | Content                  |
| ------------ | ------------- | ---------- | ------------- | ------------------------ |
| `status`     | `exo-status`  | Namespace  | Epoch title   | Phase, tasks, git state  |
| `phase.{id}` | `exo-phase`   | Module     | Epoch title   | Goals, tasks, progress   |
| `context`    | `exo-context` | Namespace  | —             | Full context dump        |
| `plan`       | `exo-plan`    | Namespace  | —             | Roadmap, epoch structure |

All use "global" SymbolKinds (Namespace, Module) so they're always visible in the picker.

**`steering` remains a tool.** Unlike the other Status/Orient reads, steering synthesizes confidence-scored action options — an inherently expensive computation. It's not something that should be kept reactively up-to-date or casually queried. The tool-call boundary signals its cost and intentionality. (If computed root optimization eventually makes steering cheap to maintain, this decision can be revisited.)

**Implication**: This reduces the LM tool surface to **mutations, actions, and steering only**. Most Status and Orient tools become resources. The SOAR model simplifies: Status and Orient are mostly "read a resource," Act is "call a tool," steering is the one Orient operation that remains a tool.

#### Active Phase Entities (Scoped by Phase State)

| Resource    | When Active            | SymbolKind | containerName |
| ----------- | ---------------------- | ---------- | ------------- |
| `goal.{id}` | Always (current phase) | Interface  | Phase title   |
| `task.{id}` | Goal is in-progress    | Property   | Goal label    |

Goals in the current phase use a "global" SymbolKind (Interface) so they're prominently discoverable. Tasks use Property (local) since they're more granular — you'd search `#task` explicitly rather than browse for them.

**Activation rule**: Tasks are emitted only for goals whose status is `in-progress`. This keeps the symbol count proportional to active work, not total work. The computed root `computed:phase.details` already contains goal status, so the projection just filters.

Goals in _other_ phases and phases in _other_ epochs are not emitted. They're historical — use tool calls or file reads if needed.

#### RFCs (Collection, Always Active)

```
rfc.{feature}.{number}.{slug}
```

Examples:

```
rfc.symbols.00236.resource-projections
rfc.workflow.00224.soar-loop
rfc.reactivity.00188.derived-roots
```

| Aspect        | Value                         |
| ------------- | ----------------------------- |
| SymbolKind    | File (global)                 |
| containerName | `Stage {n}`                   |
| When active   | Always — all RFCs registered  |
| Content       | Full RFC rendered as markdown |

**Organizational insight**: The `feature` field in RFC frontmatter becomes a first-class organizing axis. Typing `#rfc workflow` chunks the RFC list by domain, which is how humans actually think about RFCs ("that workflow RFC") rather than by stage or number. This means we should enforce `feature` as a required field and audit existing RFCs for consistent feature values.

#### Ideas (Collection, Always Active)

```
idea.{category}.{slug}
```

Examples:

```
idea.reactivity.file-watcher
idea.workflow.soar-metrics
idea.ux.goal-progress-bar
```

| Aspect        | Value                                   |
| ------------- | --------------------------------------- |
| SymbolKind    | Variable (local)                        |
| containerName | Category name                           |
| When active   | Always                                  |
| Content       | Idea description, priority, linked RFCs |

**Organizational insight**: Ideas need consistent categorization. Currently `ideas.toml` has categories but they're loosely applied. The symbol constraint says: if `#idea react` should work, every idea needs a meaningful category. This is an upstream improvement, not a presentation hack.

#### Inbox Items (Collection, When Non-Empty)

```
inbox.{type}.{slug}
```

Examples:

```
inbox.bug.phase-transition
inbox.feature.new-tool-request
inbox.question.rfc-naming
```

| Aspect        | Value                                  |
| ------------- | -------------------------------------- |
| SymbolKind    | Event (local)                          |
| containerName | Subject reference                      |
| When active   | Only when inbox is non-empty           |
| Content       | Item details, source, related entities |

**Organizational insight**: Inbox items need a type/category field (bug, feature, question, etc.) to be discoverable. This enables the user to say "review the `#inbox.bug`" to find all bug reports.

### Symbol Count Analysis

With the naming convention above, a typical project at any moment:

| Category                  | Count      | Searchable via                      |
| ------------------------- | ---------- | ----------------------------------- |
| Singletons                | 5          | Direct name (`#status`, `#plan`)    |
| Goals (current phase)     | 3-5        | `#goal` prefix                      |
| Tasks (in-progress goals) | 2-8        | `#task` prefix                      |
| RFCs                      | 20-80+     | `#rfc {feature}` or `#rfc {number}` |
| Ideas                     | 10-40      | `#idea {category}`                  |
| Inbox                     | 0-10       | `#inbox {type}`                     |
| **Total**                 | **40-148** |                                     |

This looks large, but it's fine because:

1. **No one browses the full list** — humans type to filter, just like with code symbols in a large codebase
2. **Dot-qualified names prune naturally** — typing any segment narrows results dramatically
3. **The 20-symbol tool cap only applies to `search_workspace_symbols`** — and the agent always provides a query, so it gets relevant results
4. **Global/local SymbolKind filtering** hides tasks, ideas, and inbox items from casual browsing

### Activation and Reactivity

Which resources are emitted is determined by `project()`, which reads from computed roots. Since computed roots are reactive:

- **Phase change** → `computed:phase.details` invalidated → `project()` returns different goals/tasks
- **Goal started** → same root invalidated → tasks for that goal now emitted as children
- **RFC created** → `computed:rfc.index` invalidated → new RFC symbol appears
- **Inbox cleared** → `computed:inbox.summary` invalidated → inbox symbols disappear

No separate activation API is needed. `provideWorkspaceSymbols()` is called fresh on every query, and it calls `computedRootRegistry.get()`, which recomputes if stale. The symbol set is always current.

### Upstream Organizational Implications

The resource catalog reveals three upstream improvements. These aren't symbol-system concerns — they're organizational concerns that the symbol constraint makes visible.

#### RFC Feature Categories (Proposed Controlled Vocabulary)

The existing `feature` field is inconsistent: 100+ RFCs use ~40 distinct values including "Unknown" (13 times), one-off strings like "vscode-surface-inventory", and casing variants ("workflow" vs "Workflow"). Proposed consolidation into two tiers:

**Universal categories** (hardcoded, always available):

| Category       | Covers                                                    | Example RFCs                    |
| -------------- | --------------------------------------------------------- | ------------------------------- |
| `workflow`     | SOAR loop, phases, goals, tasks, implementation plans     | 00224, 00230, 00231, 0131, 0105 |
| `architecture` | Reactive system, CLI patterns, structured IO, file system | 0119, 0118, 0133, 0135, 00234   |
| `agent`        | LM tools, agent interop, hybrid tools, prompts, guidance  | 0083, 0125, 0128, 0136, 00178   |

**Project-specific categories** (configurable in `exosuit.toml`):

| Category     | Covers                                                     | Example RFCs                  |
| ------------ | ---------------------------------------------------------- | ----------------------------- |
| `governance` | RFCs, axioms, modes, lifecycle management                  | 0076, 0120, 00226, 0149, 0150 |
| `symbols`    | Resource projections, workspace symbols, discovery         | 00236, 00227                  |
| `tooling`    | CLI commands, exospec, build tools, testing                | 0132, 0134, 00233, 0201, 0153 |
| `ui`         | Sidebar, status bar, webview, optimistic UI, Problems pane | 0094, 0157, 00225, 00232      |
| `schema`     | Identity, ordering, terminology, data model                | 0130, 00228, 00229            |

The universal categories represent concerns present in virtually any exosuit-managed project. Project-specific categories are declared in `exosuit.toml` and vary by project. Together, the vocabulary is controlled — new values require explicit justification. The goal is that `#rfc.workflow` gives you all workflow RFCs and `#rfc.agent` gives you all agent RFCs, with no ambiguity about which bucket an RFC belongs in.

#### Idea Categories

Ideas currently have a `tags` field (always empty). The symbol constraint says: replace `tags` with a required `category` field using the same vocabulary as RFC features (universal + project-specific). Ideas default to `triage` if uncategorized — this matches the natural workflow where ideas are captured quickly and categorized later. Every idea needs a meaningful category for `#idea.{category}` to work, and `triage` gives uncategorized ideas a home that's both searchable (`#idea.triage`) and signals "needs attention."

#### Inbox Item Types

Inbox items need a `type` field: `bug`, `feature`, `question`, `cleanup`. This enables `#inbox.bug` to find all bug reports.

### Zero-Arg Tool Replacement

The singleton resources (`status`, `steering`, `phase.{id}`, `context`, `plan`) directly replace the current zero-arg read-only tools. This has implications:

| Current Tool     | Becomes Resource            | What Changes                                      | Deprecation Gate                |
| ---------------- | --------------------------- | ------------------------------------------------- | ------------------------------- |
| `exo-status`     | `#status`                   | Attachable, composable, cached                    | Agent: Phase 2 / Human: Phase 5 |
| `exo-phase`      | `#phase.{id}`               | Same                                              | Agent: Phase 2 / Human: Phase 5 |
| `exo-context`    | `#context`                  | Same                                              | Agent: Phase 2 / Human: Phase 5 |
| `exo-plan`       | `#plan`                     | Same                                              | Agent: Phase 2 / Human: Phase 5 |
| `exo-list-tasks` | (merged into `#phase.{id}`) | Tasks visible in phase content                    | Agent: Phase 2 / Human: Phase 5 |
| `exo-goal-list`  | (merged into `#phase.{id}`) | Goals visible in phase content                    | Agent: Phase 2 / Human: Phase 5 |
| `exo-steering`   | (stays as tool)             | Expensive computation; tool boundary signals cost | N/A                             |

**Remaining tools after migration** (mutations, actions, and steering):

- `exo-steering` (expensive Orient operation)
- `exo-task-*` (add, complete, remove, update, reorder, start)
- `exo-tdd-*` (start, red, green)
- `exo-rfc-create`, `exo-rfc-promote`
- `exo-epoch-start`, `exo-epoch-finish`
- `exo-phase-finish`
- `exo-add-goal`, `exo-add-task`
- `exo-idea`

This cuts the read-only tool count to zero (except steering) and aligns cleanly with SOAR: resources are for Status/Orient, tools are for Act, steering is the one Orient operation expensive enough to warrant a tool.

## Alternatives Considered

### Direct symbol registration (no resource projections)

Register symbols imperatively, bypassing computed roots. Rejected: duplicates the caching and invalidation that computed roots already provide.

### File materialization

Generate `.exo/` files on disk containing rendered content. Rejected: adds file lifecycle management, risks stale files, doesn't leverage virtual documents.

### Custom chat participant variables

Use the proposed `ChatParticipantVariableProvider` API to add items to the chat `#` picker. Rejected: the API is **participant-scoped** — variables only appear when the user is chatting with a specific `@participant`, not in the default Copilot chat. Since exosuit resources should be discoverable in any chat context (not just `@exo`), this is non-viable. The `chatContextProvider` API (see Transport Strategy) is the correct proposed API for this use case — it's workspace-scoped and works across all chat participants.

### Extend computed roots directly

Add `project()` to the computed root definition. Rejected: conflates computation with presentation. Some computed roots are internal (e.g., `computed:phase.active` for the status bar) and shouldn't be exposed as resources.

## Implementation Plan

### Phase 0: Steel Thread Spike

Validate the end-to-end pipeline before building the full system:

- [ ] Hardcoded `ResourceRegistry` with one projection (phase details)
- [ ] `exo://` scheme registered with `TextDocumentContentProvider`
- [ ] `WorkspaceSymbolProvider` returning symbols from the projection
- [ ] Verify: `search_workspace_symbols("phase")` finds the symbol and returns content
- [ ] Verify: content updates when phase state changes (computed root invalidation)
- [ ] Verify: command palette `#` search finds the symbol

Success criteria: agent can reference `#phase.whiteboard-spike` via `search_workspace_symbols` and receive current phase details. This validates the agent discovery path.

### Phase 0.5: chatContextProvider Spike (Insiders)

Validate the human discovery path on VS Code Insiders:

- [ ] Enable `vscode.proposed.chatContextProvider` in extension manifest
- [ ] Register `chatContextProvider` for `{ scheme: 'exo' }` selector
- [ ] Wire up the same `ResourceDocument` content used by `TextDocumentContentProvider`
- [ ] Verify: typing `#` in chat shows exo resources
- [ ] Verify: attaching a resource sends rendered content to the agent

Success criteria: human can type `#phase` in chat `#` picker and attach phase details. This validates that our `exo://` scheme works with `DocumentSelector` and that the proposed API meets our needs.

**Note**: This phase runs on Insiders only and depends on the `chatContextProvider` API stabilization timeline. If the API changes significantly, this spike adapts.

### Phase 1: Core Infrastructure

- [ ] `ResourceRegistry` service (production version)
- [ ] `ResourceDocument` / `ResourceSection` types
- [ ] `exo://` URI scheme parsing and routing
- [ ] `WorkspaceSymbolProvider` walking `ResourceDocument` sections
- [ ] `TextDocumentContentProvider` serving section content
- [ ] `chatContextProvider` registration (behind proposed API flag)

### Phase 2: Singleton Resources + Agent Tool Removal

- [ ] `status` singleton projection
- [ ] `phase.{id}` projection (with goals and in-progress tasks)
- [ ] `context` singleton projection
- [ ] `plan` singleton projection
- [ ] Remove agent-facing zero-arg tools (`exo-status`, `exo-phase`, `exo-list-tasks`, `exo-goal-list`, `exo-context`, `exo-plan`)
- [ ] Keep human-facing tool equivalents until `chatContextProvider` stabilizes

### Phase 3: Collection Resources

- [ ] RFC projection (from `computed:rfc.index`), using feature categories
- [ ] Ideas projection (from `computed:ideas.summary`), using categories (default: `triage`)
- [ ] Inbox projection (from `computed:inbox.summary`), using types

### Phase 4: Upstream Organization

- [ ] Audit and normalize RFC `feature` field to controlled vocabulary (universal + project-specific)
- [ ] Add `category` field to ideas (replace empty `tags`, default `triage`)
- [ ] Add `type` field to inbox items
- [ ] Configure project-specific categories in `exosuit.toml`

### Phase 5: Human Tool Migration

- [ ] Once `chatContextProvider` reaches stable API, remove remaining zero-arg LM tools
- [ ] Human and agent paths both fully served by resource projections

### Phase 6: Refinement

- [ ] Content rendering polish (markdown formatting for agent readability)
- [ ] Observability (logging when symbols are queried, content is served)
- [ ] Tests (mock computed roots, verify symbol output, verify content rendering)
- [ ] Explore workspace-context auto-injection for `#status`

## Resolved Decisions

1. **Content format** → Markdown. Coherent with tool output format, optimized for AI reading (scannable headers, dense bullets, self-describing). TOML/YAML adds syntactic noise without comprehension benefit.
2. **Unified document model** → Yes. `project()` returns a `ResourceDocument` whose sections define both symbols and content. No separate `toResources()` and `renderContent()`. Symbols are a consequence of document structure.
3. **Range granularity** → One `exo://` document per entity. Simpler to implement, no range tracking needed. `ResourceSection.children` supports nesting for future range-based references if needed.
4. **RFC feature categories** → Two-tier controlled vocabulary: universal categories hardcoded (`workflow`, `architecture`, `agent`), project-specific categories configurable in `exosuit.toml`. `feature` becomes required.
5. **Idea categories** → Same vocabulary as RFC features. Default to `triage` if uncategorized (searchable via `#idea.triage`, signals "needs attention").
6. **Steering** → Stays as a tool. Expensive to compute; the tool boundary signals cost and intentionality. Revisit if computed root optimization makes it cheap.
7. **Tool deprecation** → Sequenced by transport. Agent-facing tools removed once WorkspaceSymbolProvider validated. Human-facing tools kept until `chatContextProvider` stabilizes. No gap in either path.
8. **Chat `#` picker transport** → `chatContextProvider` (proposed API, issue [#271104](https://github.com/microsoft/vscode/issues/271104)). `ChatParticipantVariableProvider` rejected as participant-scoped. WorkspaceSymbolProvider does NOT appear in chat `#` picker — only in command palette `#` and `search_workspace_symbols` tool.
9. **Chat `#` picker limitation** → The chat input `#` picker uses `DocumentSymbol` cache from open editors (LRU capped at 15), NOT `WorkspaceSymbolProvider`. This is a VS Code architectural fact, not a bug. Our transport strategy accounts for it.

## Open Questions

1. **`chatContextProvider` API stability**: The API is on a February 2026 milestone and alexr00 says "one more big change this week" (breaking into 3 separate providers). How aggressively should we build on the proposed API? Current plan: implement on Insiders, keep LM tools as stable fallback.
2. **Workspace context auto-injection**: The planned three-provider split includes a "workspace context" variant that auto-attaches to every chat request. Should `#status` be auto-injected? This is powerful but could waste tokens if the agent doesn't need plan context for a given query.
3. **Feature category boundaries**: The proposed universal categories (`workflow`, `architecture`, `agent`) cover broad swaths. Some project-specific categories may be too narrow (`symbols`) or too broad (`architecture`). The vocabulary should be refined during the Phase 4 audit.
4. **`exo://` scheme and DocumentSelector**: The `chatContextProvider` API uses `DocumentSelector` to match resources. Does VS Code's `DocumentSelector` support custom schemes like `exo`? This needs empirical validation — if not, we may need the "explicit context" provider variant instead of "resource context."

