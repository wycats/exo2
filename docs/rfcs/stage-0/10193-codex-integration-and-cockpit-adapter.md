<!-- exo:10193 ulid:01ktf5tp81ydvsat1vep97aejs -->

# RFC 10193: Codex Integration and Cockpit Adapter

**Status**: Idea
**Feature**: codex

## Summary

Define Exo's Codex integration strategy: how the Exo Codex plugin is packaged, how agents are instructed to use it, how the MCP surface appears in Codex hosts, how workspace binding works for plugin-launched MCP calls, and how future cockpit surfaces can reach Codex users.

This RFC is the Codex host-integration layer. It delegates the `exo-run` MCP tool contract to **RFC 10200: CLI-Shaped exo-run MCP Transport** and durable MCP proxy architecture to **RFC 10190: Durable MCP Proxy and Hot-Swappable Exo Worker**. It delegates sidebar concepts and implemented VS Code UI architecture to RFC 00184 and RFC 10162.

## Motivation

Exo now has a Codex plugin that registers the durable `exo-mcp` proxy and teaches agents the CLI-shaped `exo-run` workflow. That gives Codex a practical Exo entrypoint, but the product contract needs to be explicit.

Codex is a family of surfaces with different support for local tools, plugins, skills, app-backed capabilities, browser views, approval controls, and long-running conversations. Exo needs a design that says how the plugin binds to the active project workspace, how the agent discovers and uses Exo commands, how safety boundaries appear, and how richer UI can be added without changing command semantics.

The goal is to make Exo feel native in Codex while preserving the central Exo theory from RFC 10163: agents use a small CLI-shaped tool surface, discover commands through help, and avoid paying per-command schema cost every turn.

## Current Package Shape

The current Codex plugin lives under `plugins/exo` and contains:

- `.codex-plugin/plugin.json` as the Codex package entrypoint;
- `.mcp.json` as the MCP server registration;
- `skills/exo/SKILL.md` as the workflow instruction layer;
- `README.md` with local setup guidance.

The plugin currently launches:

```json
{
  "mcpServers": {
    "exo": {
      "command": "exo-mcp",
      "args": []
    }
  }
}
```

The source plugin package keeps the portable `exo-mcp` command. Local dogfood can rewrite an installed plugin cache to an absolute `exo-mcp` path so the running Codex host uses the freshly installed binary.

The MCP server exposes the CLI-shaped `exo-run` tool described in **RFC 10200: CLI-Shaped exo-run MCP Transport**.

## Codex Workspace Metadata Contract

Codex plugin-packaged stdio MCP servers may start with process cwd set to the plugin package root. That cwd is not the active project workspace. The supported Codex integration path is per-tool-call MCP metadata.

`exo-mcp` advertises this capability during MCP initialize:

```json
{
  "capabilities": {
    "experimental": {
      "codex/sandbox-state-meta": {}
    }
  }
}
```

For servers that advertise this capability, Codex injects `_meta["codex/sandbox-state-meta"]` on `tools/call`. That object can include:

- `sandboxCwd`: the current turn cwd and workspace execution cwd;
- `sandboxPolicy`;
- `permissionProfile`;
- `codexLinuxSandboxExe`;
- `useLegacyLandlock`.

Exo uses `sandboxCwd` as the Codex plugin workspace source when it is present and absolute. The `exo-mcp` proxy then runs Exo's normal project resolver from that cwd. The condition is successful Exo project resolution from the selected cwd.

This metadata is injected for `tools/call`. It is not available for `resources/list` or `resources/read`, so the baseline plugin design keeps `tools/list` static and workspace-free. Broader turn metadata such as `x-codex-turn-metadata` can be useful diagnostics, but it is not Exo's primary workspace-root binding.

Process cwd support remains the local/manual MCP launch path when the process cwd resolves as an Exo project. Session/global-state fallbacks are compatibility and diagnostic inputs; the product contract for Codex plugin launches is tool-call `sandboxCwd`.

## Design

### Layer Ownership

Codex integration has four layers:

1. **Plugin package**: how Codex discovers Exo, shows it in plugin UI, installs it, and wires MCP servers and skills.
2. **Skill guidance**: how the agent learns Exo's workflow discipline, confirmation handling, and CLI-shaped command conventions.
3. **MCP action surface**: how the agent reads and mutates Exo state through tool calls.
4. **Cockpit surface**: how the human sees Exo state through a richer UI when the host supports it.

This RFC owns the first, second, and fourth layers, plus the Codex-specific interpretation of the third. **RFC 10200: CLI-Shaped exo-run MCP Transport** owns the actual MCP tool contract. **RFC 10190** owns `exo-mcp`, static `tools/list`, Codex workspace metadata handling, and worker lifecycle.

### Plugin Contract

The Codex plugin manifest remains the package entrypoint. It keeps the Exo plugin small and legible:

- declare Exo as a project workflow plugin;
- point to the Exo skill directory;
- register the Exo MCP server through `.mcp.json`;
- describe the plugin as local-project state and workflow tooling;
- keep UI promises aligned with host support.

The plugin treats the Exo binaries as external local dependencies. Installation guidance explains that both `exo` and `exo-mcp` must be available to the Codex process, and that active threads may need a reload or fresh thread after plugin or binary changes.

The current plugin launch target is `exo-mcp`. Dogfood verification should prove the host is connected to the expected proxy and that the proxy routes tool calls through a worker for the active Codex workspace.

### Skill Contract

The Exo skill remains the workflow instruction layer. It teaches the agent to:

- start with `exo-run` command `status` and then read task state when relevant;
- send Exo CLI syntax without the leading `exo`;
- use placeholder args for multi-line or quote-heavy content;
- treat workflow and execution confirmations as human approval gates;
- keep Exo state current through Exo commands;
- use `exo-run "help ..."` for discovery in the current baseline;
- use future wrapper tools only after RFC 10200 defines and implements them.

The skill is behavioral guidance and recovery cues. The CLI manual and command help remain the source for exhaustive command documentation.

### MCP Tool Presentation In Codex

Codex-visible tool presentation is layered:

- use standardized MCP fields such as tool titles, annotations, icons, output schemas, and `_meta` where supported;
- include OpenAI-specific invocation metadata when available;
- keep textual tool descriptions clear when hosts ignore richer metadata;
- keep annotations as presentation and routing hints, not security facts;
- keep ordinary successful reads and help text-first by default, matching RFC 10200;
- reserve structured content for JSON/detail requests, errors, confirmations, replay/control data, and effect-budget rejections.

**RFC 10200: CLI-Shaped exo-run MCP Transport** owns the specific MCP tool names, schemas, effect metadata, and output contract. This RFC requires those choices to fit Codex presentation and permission surfaces.

Codex-visible output should avoid duplicating the same answer as prose and JSON. When a host surfaces only text, Exo should still be usable. When a host exposes structured results, Exo should use them for control data instead of treating them as a second agent-facing transcript.

### Safety And Approval Model

Codex integration exposes safety at the same boundaries Codex and ChatGPT-style app controls understand:

- read-only actions are visibly separate from write and exec actions when practical;
- write actions flow through Exo's normal policy and confirmation model;
- exec or destructive actions require explicit confirmation;
- hidden confirmation tickets and workflow confirmation payloads remain machine-only;
- admin or host-level read/write controls may disable some actions while leaving Exo help/status behavior available.

RFC 10200's wrapper-tool direction keeps the CLI-shaped command language while adding effect-budgeted entrypoints. `exo-help` is the help-only affordance. `exo-read` accepts commands classified as `pure`. `exo-write` accepts `pure` and `write` commands. Stronger effects require the stronger tool before execution.

If the Codex host blocks an action before Exo can return its own confirmation prompt, the agent reports the host permission boundary, continues with available read/help/status behavior, and asks the user to enable the needed plugin or app action permission. Hidden Exo confirmation data stays inside Exo's confirmation model.

### Install, Reload, And Stale Runtime Ergonomics

Codex users need clear recovery paths when plugin state, binary state, thread state, and worker state diverge.

The plugin documentation distinguishes:

- installed plugin package version;
- Exo binary version on `PATH`;
- active `exo-mcp` proxy process;
- worker identity and workspace root;
- current Codex thread's loaded tools and skills.

When users update Exo, the expected recovery path is:

1. Install or update the local Exo binaries.
2. Confirm the plugin package is installed and enabled.
3. Confirm the Codex process can resolve the expected `exo-mcp` and `exo` binaries.
4. Start a fresh Codex thread or reload the host when the active thread cannot see newly installed tools.
5. Run a low-risk `status` or `help` call through `exo-run`.
6. If MCP reads disagree with direct CLI reads, compare the active proxy, worker identity, workspace root, database path, and sidecar root before investigating command semantics.

When the MCP tool is absent, agents fail closed for Exo state claims and mutations: explain that the Exo MCP tool is not available in the current Codex thread or host, and use the user's direction before switching to a direct shell CLI workflow.

### Cockpit Strategy

The long-term Exo cockpit should be a host-neutral web surface with adapters.

The cockpit surface should provide glanceable Exo state: current mode, active phase, goals, task plan, steering, sidecar health, and pending confirmations. It should use structured Exo data and commands.

Supported adapters may include:

- VS Code sidebar or webview adapter, grounded in RFC 00184 and RFC 10162;
- local SvelteKit or equivalent web app served from the workspace;
- Codex integrated browser pointing at the local cockpit app;
- MCP App/resource UI where the host supports embedded app UI;
- plain textual fallback through `exo-run` when no cockpit UI is available.

The cockpit is optional for the baseline Codex plugin. Exo remains usable through skills and MCP tools alone.

Iframe-style MCP App UI remains experimental for Exo until host support and user experience are proven. Native VS Code surfaces, local browser surfaces, and host-integrated browser surfaces are the first cockpit adapter targets.

### Codex Host Capability Matrix

| Host surface | Skill guidance | MCP tools | Local cockpit browser | Embedded MCP/App UI | Baseline behavior |
| --- | --- | --- | --- | --- | --- |
| Codex desktop app | Yes | Yes, when plugin is loaded | Expected through integrated browser or localhost | Opportunistic | Full agent workflow plus optional cockpit |
| Codex CLI | Yes, when installed | Yes, when configured | Not assumed | Not assumed | Text and structured tool workflow |
| Codex IDE extension | Yes, when plugin support is available | Yes, when configured | Host-dependent | Host-dependent | Prefer native IDE/VS Code Exo UI when present |
| Codex web/cloud | Host-dependent | Host-dependent, local MCP may be unavailable | Not assumed | Host-dependent | Degrade to docs, skills, or remote-capable integrations |
| ChatGPT app host | Maybe | MCP-backed app tools | Not assumed | Possible through Apps SDK/MCP App patterns | App-style experience when supported |

Implementations should probe host behavior and record evidence before claiming support for a cockpit adapter in a host.

## Relationships

- **RFC 10163** defines the CLI-shaped tool reduction theory that this integration preserves.
- **RFC 10200: CLI-Shaped exo-run MCP Transport** defines the MCP transport and tool contract that Codex consumes.
- **RFC 10190** defines the durable MCP proxy, Codex `sandboxCwd` workspace binding, static `tools/list`, and worker lifecycle.
- **RFC 00184** defines the mode-aware sidebar cockpit behavior reused by cockpit adapters. It should be reconciled before a cockpit adapter depends on its `ProgressMode` details.
- **RFC 10162** defines the stable VS Code extension frame and view-provider architecture. Where cockpit behavior differs, RFC 00184 owns the newer cockpit behavior.

## Implementation Direction

1. Keep the current plugin package baseline: manifest, skill, `.mcp.json`, and README.
2. Implement the RFC 10190 workspace-binding contract in `exo-mcp`: initialize capability, static `tools/list`, tool-call `sandboxCwd` extraction, process-cwd local support, diagnostic reporting, and workspace-scoped worker selection.
3. Keep RFC 10200 as the tool contract for `exo-run` and future effect-budget wrappers.
4. Update the Exo skill and plugin README when wrapper tools are implemented and dogfooded.
5. Prototype a host-neutral cockpit web surface behind structured Exo data.
6. Add Codex cockpit adapter guidance after proving which Codex hosts support browser or app UI well enough.

## Acceptance Criteria

- A future implementer knows where to change plugin manifest metadata, skill guidance, MCP tool metadata, safety wrappers, workspace binding, and cockpit adapters.
- The Codex plugin remains useful when only skills and `exo-run` are available.
- The RFC explains the per-tool-call Codex metadata contract Exo uses for plugin workspace binding.
- The RFC explains why Exo keeps the CLI-shaped tool theory while adding effect-budgeted wrappers through RFC 10200.
- The RFC clearly says which UI paths are opportunistic because Codex and ChatGPT host support varies.
- The RFC avoids duplicating the MCP tool contract from **RFC 10200: CLI-Shaped exo-run MCP Transport** and the sidebar architecture from RFC 00184/10162.
- The plugin README documents install, reload, stale-runtime, and first-call troubleshooting.
- The Exo skill documents current baseline discovery through `exo-run "help ..."` and mentions wrapper tools only after they exist.
- The plugin manifest, `.mcp.json`, README, and skill stay coherent with the `exo-mcp` launch target.

## Stage Readiness

Keep this RFC at Stage 0 until the RFC 10190 Codex metadata binding is implemented and dogfooded. After that evidence exists, this RFC can move to Stage 1 as the Codex integration proposal that connects plugin packaging, skill guidance, MCP presentation, and cockpit strategy.

## Open Questions

- Which Codex surfaces reliably support MCP resource or app UI for local MCP servers?
- Should the local cockpit be served by `exo` itself, by the VS Code extension, or by a separate web package?
- Should plugin health expose a dedicated diagnostic command for binary freshness, active proxy identity, worker identity, and workspace source?
- How should enterprise app action controls map onto Exo's effect classification beyond read/write/exec?
