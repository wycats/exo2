<!-- exo:10203 ulid:01kync6fa5cmyr7f4rmefj82hy -->

# RFC 10203: Local Lane Workbench Host and Agent Launch

**Feature**: lane-centered-workbench

## Summary

This RFC specifies a local web workbench for Exo's lane-centered model. The
workbench is hosted by the project-authority Exo daemon, opened through a
short-lived workspace-bound capability, and usable from an ordinary browser or
from an agent host that can present a link. Its first screen is the current
workbench lane: the durable intent, current Git state, associated phase, goals,
tasks, steering, and diagnostics for the workspace that launched it.

The design adds two pure, replayable Exo commands. `exo workbench launch`
starts or reuses the daemon's loopback HTTP host and returns a capability-scoped
URL. `exo workbench snapshot` returns the data model behind the screen. MCP
presents a successful launch as both text and a standard `resource_link`, so an
agent can give the user an openable workbench without opening a browser on the
user's behalf.

The first implementation is deliberately narrow. It lets a user inspect the
focused lane and focus a different existing lane. It does not create lanes,
advance tasks, change lifecycle state, manage pull requests, or turn the
browser into a second project model. Every read and write passes through Exo's
existing command dispatcher with one request-scoped workspace identity.

## Motivation

RFC 10202 established the adapter-neutral lane foundation: portable lane
identity, workspace-local focus, a public `exo lane` command surface, and a
small VS Code focus client. That proof makes lane continuity real, but it does
not yet provide the place where lane-centered work can become the primary
human experience.

Continuing only in the existing VS Code sidebar would inherit a UI organized
around earlier Exo concepts. It would also leave users working mainly in Codex
without a durable visual surface for the workspace an agent is advancing. A
fresh local workbench gives the project room to organize the experience around
lanes from the first pixel while remaining available beside any editor or agent
host.

A browser surface creates an integration question that the lane model alone
does not answer. An agent may know that a workbench exists, but it needs a safe
way to select the current workspace and hand the human something they can open.
A raw localhost URL is insufficient: linked worktrees share project state but
have distinct focus, and a loopback listener is not an authorization boundary.
Launch must carry workspace identity without publishing local paths, and the
browser must prove that it received the link from the matching daemon.

The local workbench is therefore a capability of Exo's existing runtime
authority, not a separate application with its own project resolver, database,
or lifecycle rules.

## Guide-Level Explanation

A user asks Exo for the workbench associated with the current workspace:

```text
exo workbench launch
```

The command returns a local link together with a project identifier, an opaque
workspace key and label, the daemon instance that issued the link, and the
five-minute ticket expiration. The command is safe to repeat. The first launch
starts the host; later launches reuse it and issue a fresh capability. Neither
case changes canonical project state or opens a browser.

An agent invokes the same command through `exo-run`. MCP clients receive a
normal text result and, under the protocol revision Exo already implements, a
`resource_link` named `exo-workbench`. A client that renders resource links can
offer a direct open action. A client that does not still receives the complete
textual URL. The user remains in control of opening it.

The URL selects one Exo project and one validated workspace. The browser opens
on that workspace's focused lane. If two linked worktrees focus different
lanes, launching from each worktree produces a workbench for the same project
but a different current stream. Neither link contains the raw workspace path,
and opening one cannot silently switch the focus of the other.

The first screen answers four connected questions: what lane this workspace is
advancing, which branch and commit it is advancing from, which phase and tasks
supply the planning context, and what Exo recommends next. A lane rail makes
other existing lanes visible. Selecting one invokes the existing `lane focus`
operation and refreshes the screen from the committed result.

An agent can obtain the same orientation without opening the browser:

```text
exo workbench snapshot
```

The browser consumes this command's result rather than reconstructing state on
its own. A snapshot is a read of one validated workspace, not an aggregate over
whatever checkout happens to be active in another process.

## Reference-Level Explanation

### Command surface and effects

The generated command namespace is:

```rust
#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "workbench", description = "Local lane workbench commands")]
pub enum WorkbenchCommands {
    #[exo(effect = "pure", description = "Launch the local lane workbench")]
    Launch,

    #[exo(effect = "pure", description = "Read the current lane workbench snapshot")]
    Snapshot,
}
```

Both operations have `Effect::Pure` and
`RecoveryClass::ReplayableRead`. Launching a loopback listener and issuing an
ephemeral capability are runtime presentation effects; they do not mutate
canonical SQLite state, the filesystem, Git, or a remote system. Retrying a
launch can safely return a fresh ticket for the same validated workspace.

The two commands require daemon runtime services. Normal CLI execution already
uses daemon dispatch. The MCP adapter must route requests whose resolved command
path begins with `workbench` through
`daemon_client::send_request_with_project_recovery_report` rather than executing
them in the MCP worker. It preserves the original request envelope and
workspace root. `exo --direct workbench launch` and `exo --direct workbench
snapshot` fail with `precondition_failed` and stable detail kind
`workbench.daemon_required`; they do not start an independent host.

Help, CommandSpec artifacts, machine-channel coverage, and MCP annotations are
derived from the same registered commands. The commands do not acquire a new
shell or browser-opening effect.

### Daemon runtime services

The daemon extracts its existing request closure into a cloneable
`DaemonRequestDispatcher`. The Unix or named-pipe server and the HTTP adapter
share this dispatcher, request admission semaphore, outcome ledger, project
authority, write notification sender, and request-scoped workspace validator.
The HTTP server cannot bypass bounded admission or call command implementations
directly.

`CommandContext` and `MutableCommandContext` gain an optional borrowed
`DaemonRuntimeServices`. Existing direct and test contexts use `None`. Daemon
contexts provide an `Arc<WorkbenchHostManager>` and the daemon-wide workbench
revision counter. `WorkbenchLaunch` and `WorkbenchSnapshot` require this
service and produce `workbench.daemon_required` when it is absent.

The host manager begins unbound. The first launch serializes startup, binds
`127.0.0.1:0`, creates daemon-lifetime secret material, starts the HTTP task on
the existing Tokio runtime, and records the resulting origin. Concurrent first
launches converge on one host. Later launches reuse that host. A bind or server
startup failure does not stop the machine-channel daemon; launch returns
`workbench.host_unavailable` with a stable diagnostic.

The host shuts down with the daemon. It never publishes a replacement daemon
identity, changes `identity_matches_project`, or participates in daemon
spawn/restart authority. A connected authenticated event stream updates the
existing daemon activity clock at every keepalive, so an actively viewed
workbench prevents idle shutdown. Static asset requests by themselves do not.

### Runtime observability

The host atomically maintains the machine-local record
`runtime/workbench.host.json`:

```json
{
  "schema_version": 1,
  "instance_id": "01...",
  "pid": 1234,
  "process_start_id": "...",
  "origin": "http://127.0.0.1:49152",
  "assets_hash": "blake3:...",
  "server_task_alive": true,
  "started_at": "2026-07-28T20:00:00Z",
  "updated_at": "2026-07-28T20:00:00Z",
  "last_error": null
}
```

The record contains no secret and no workspace root. Readers accept it only
when instance ID, PID, and process-start identity match the exact current
daemon. Cleanup removes only a record owned by that exact runtime generation.
Stale or malformed records are ignored.

`daemon status` exposes the matching record as an optional `workbench_host`
object with `origin`, `assets_hash`, `server_task_alive`, `updated_at`, and
`last_error`. This is diagnostic truth, not launch authorization. Opening the
origin without a capability yields only the static session-required screen.

### Launch result

The Rust output type and the TypeScript mirror serialize this shape:

```typescript
interface WorkbenchLaunchResult {
  kind: "workbench.launch";
  ok: true;
  schema_version: 1;
  url: string;
  expires_at: string;
  expires_in_seconds: 300;
  reused_host: boolean;
  project: {
    id: string;
  };
  workspace: {
    key: string;
    label: string;
    branch: string | null;
    head: string | null;
  };
  daemon: {
    instance_id: string;
  };
}
```

`url` is a bearer capability and must be treated as secret until it expires.
The human result labels it as a five-minute link and does not print the raw
workspace path. `workspace.key` is an opaque, random daemon-lifetime identifier
mapped to the validated canonical workspace root in memory. `workspace.label`
is the branch name when attached and `detached@<short-head>` otherwise.

The URL has the form:

```text
http://127.0.0.1:<port>/#ticket=<ticket>
```

No ticket, URL, session identifier, or cookie value is written to daemon
identity, health, diagnostics, command events, SQL, or sidecar projections.

### Launch ticket

At first host startup, Exo reads 32 bytes from the operating system CSPRNG for
an in-memory HMAC key. A ticket is:

```text
v1.<base64url(payload-json)>.<base64url(hmac-sha256(payload-json))>
```

The encoded payload is signed exactly as emitted; verification does not
reserialize it. The payload is:

```typescript
interface WorkbenchTicketV1 {
  version: 1;
  capability_id: string;
  instance_id: string;
  project_id: string;
  workspace_key: string;
  capabilities: ["workbench.snapshot", "lane.focus"];
  issued_at: number;
  expires_at: number;
}
```

Times are Unix seconds. `expires_at` is exactly 300 seconds after `issued_at`.
`capability_id` is 256 bits of random data encoded without padding. The host
keeps a pending capability record that maps the ID and workspace key to the
validated root. A successful exchange consumes the pending record. A ticket is
therefore signed, short-lived, workspace-bound, daemon-bound, and one-time.

Signature comparison is constant-time. Invalid signature, unknown capability,
wrong instance, wrong project, expired ticket, and previously redeemed ticket
all return the same public `workbench.ticket_invalid` response. This prevents
the endpoint from becoming an oracle for local workspace existence.

### Browser session

The static application reads the fragment and immediately exchanges the ticket
through `POST /api/session`. On success it calls `history.replaceState` to
remove the fragment and stores no ticket in JavaScript persistence. The server
returns a public random session key and sets an independent random 256-bit
session identifier in a cookie named
`exo_workbench_session_<session-key>`. The cookie has `HttpOnly`,
`SameSite=Strict`, `Path=/`, no `Domain`, and `Max-Age=43200`. The session key
selects the matching cookie; it is not authorization without the independent
secret cookie value. Per-session names prevent two linked-worktree tabs on the
same loopback host from replacing each other's credentials. The initial
transport is HTTP loopback, so the first version does not claim a `Secure`
cookie that the server cannot portably enforce.

A session has a twelve-hour absolute lifetime and a thirty-minute idle
lifetime. An authenticated command, snapshot poll, or SSE keepalive refreshes
the idle deadline but never the absolute deadline. Sessions exist only in
memory and contain the daemon instance, project ID, workspace key, validated
workspace root, fixed capabilities, creation time, last activity, and absolute
expiration. Daemon replacement invalidates every session.

The server accepts at most 64 live sessions and evicts expired sessions before
rejecting a new exchange. It accepts at most 32 authenticated event streams.
Request bodies are limited to 64 KiB. Exceeding any bound returns
`workbench.busy` without entering Exo command dispatch.

The host binds the numeric loopback address and requires the exact expected
`Host` on every API request. Session exchange and command requests also require
the exact `Origin`. EventSource GET requests may omit `Origin`, as browsers
commonly do for same-origin streams, but a supplied origin must match exactly.
The server emits no permissive CORS headers. A missing required origin, foreign
origin, missing session, or unavailable capability fails before workspace
resolution or command dispatch.

### HTTP surface

The version-one HTTP surface is:

```text
POST /api/session   exchange a launch ticket for a session cookie
POST /api/command   invoke one capability-allowed Exo operation
GET  /api/events    receive invalidation events for the session workspace
```

`POST /api/session` accepts:

```typescript
interface WorkbenchSessionRequest {
  ticket: string;
}
```

and returns:

```typescript
interface WorkbenchSessionResult {
  kind: "workbench.session";
  ok: true;
  schema_version: 1;
  project_id: string;
  workspace_key: string;
  session_key: string;
  expires_at: string;
}
```

`POST /api/command` accepts only this discriminated union:

```typescript
interface WorkbenchCommandRequest {
  protocol_version: 1;
  id: string;
  session_key: string;
  operation:
    | { kind: "snapshot" }
    | { kind: "lane_focus"; lane_id: string };
}
```

The browser generates a ULID request ID. The server maps `snapshot` to the
registered `workbench snapshot` command and `lane_focus` to `lane focus
<lane_id>`. It constructs an ordinary `RequestEnvelope` with the session's
validated workspace root and the browser's request ID. The client cannot submit
a workspace root, arbitrary command text, Exo confirmation ticket, or workflow
confirmation. The event stream passes the same selector as
`GET /api/events?session_key=<session-key>`.

A well-formed authenticated command returns the normal Exo `ResponseEnvelope`
with HTTP status 200, including Exo errors such as an invalid lane, phase
precondition, policy failure, or `daemon.busy`. Transport, origin, session, body
shape, and capability failures use HTTP status and a small error object before
Exo dispatch. A browser retry after connection loss or `daemon.busy` preserves
the same request ID for `lane_focus`; the daemon outcome ledger remains the
recovery authority.

Each session command revalidates the retained workspace root against the
current project ID and state root before dispatch. A removed linked worktree or
a path reused by a foreign repository returns
`workbench.workspace_unavailable`. The server never falls back to the daemon's
startup workspace.

### Snapshot schema

`workbench snapshot` returns:

```typescript
interface WorkbenchSnapshot {
  kind: "workbench.snapshot";
  ok: true;
  schema_version: 1;
  observed_at: string;
  revision: number;
  project: {
    id: string;
  };
  workspace: {
    key: string;
    label: string;
    branch: string | null;
    head: string | null;
    detached: boolean;
    dirty: boolean;
  };
  lanes: WorkbenchLaneSummary[];
  focused_lane: WorkbenchLaneDetails | null;
  phase: WorkbenchPhase | null;
  steering: WorkbenchSteering;
  diagnostics: WorkbenchDiagnostic[];
}

interface WorkbenchLaneSummary {
  id: string;
  title: string;
  state: "prepared" | "executing";
  phase_id: string;
  phase_title: string;
  phase_status: string;
  focused_here: boolean;
}

interface WorkbenchLaneDetails extends WorkbenchLaneSummary {
  intent: string;
  created_at: string;
  updated_at: string;
}

interface WorkbenchPhase {
  id: string;
  title: string;
  status: string;
  goals: Array<{
    id: string;
    title: string;
    status: string;
    tasks: Array<{
      id: string;
      title: string;
      status: string;
    }>;
  }>;
}

interface WorkbenchSteering {
  situation: string;
  next_actions: Array<{
    label: string;
    command: string;
    rationale: string;
    intent: string;
    confidence: number | null;
  }>;
}

interface WorkbenchDiagnostic {
  code: string;
  severity: "info" | "warning" | "error";
  message: string;
}
```

The Rust structs are the normative serialization. The cockpit keeps matching
TypeScript types plus runtime decoders, and both suites consume checked JSON
contract fixtures emitted by Rust tests. A schema change increments
`schema_version` and updates both fixture consumers in the same change.

Snapshot composition validates the workspace once, opens one SQLite loader for
lane and plan state, and samples Git from that same root. It does not claim a
transaction across SQLite and Git. `observed_at` records when sampling
completed. Results expose branch and commit identity but omit workspace root,
Git directory, database path, state root, sidecar path, runtime path, and
process identity.

`phase` is the phase associated with the focused lane. When no lane is focused,
`focused_lane` and `phase` are null while the lane rail remains available. An
inconsistent legacy lane/phase focus is returned as a diagnostic using RFC
10202's read behavior; the snapshot does not repair it.

### Events and freshness

The daemon keeps an in-memory `u64` workbench revision initialized to zero. It
increments after every successful write notification and broadcasts the new
value. Existing machine clients still receive the current `write_happened`
notification; the event payload expansion is internal to the daemon.

An authenticated `GET /api/events` stream first sends:

```text
event: ready
id: <revision>
data: {"kind":"workbench.ready","revision":<revision>}
```

A later successful Exo write sends `event: invalidate` with
`{"kind":"workbench.invalidate","revision":<revision>}`. If the client's
`Last-Event-ID` differs from the current revision, reconnect immediately emits
one invalidation rather than attempting event-log replay. Events are hints; the
client always obtains truth by refetching `workbench snapshot`.

The stream sends an SSE comment every 15 seconds, refreshes the session idle
deadline, and updates daemon activity. A lagged broadcast collapses to one
invalidation at the newest revision. No event contains command results, paths,
or secrets.

Exo write notifications do not cover ordinary Git or filesystem changes. While
the document is visible, the cockpit refreshes a snapshot every five seconds in
addition to responding to SSE. It stops the timer while hidden and refreshes
immediately on `visibilitychange` or window focus. This gives branch, HEAD, and
dirty state a bounded freshness contract without introducing a new filesystem
watcher in the first slice.

### MCP presentation

Exo already negotiates MCP protocol revision `2025-06-18`, whose tool result
content includes
[`ResourceLink`](https://modelcontextprotocol.io/specification/2025-06-18/schema).
`McpContent` gains this variant:

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpContent {
    Text { text: String },
    ResourceLink {
        name: String,
        title: Option<String>,
        uri: String,
        description: Option<String>,
        #[serde(rename = "mimeType")]
        mime_type: Option<String>,
        annotations: Option<McpAnnotations>,
    },
}
```

A successful `workbench launch` result always includes structured content and
adds a second content block with:

```json
{
  "type": "resource_link",
  "name": "exo-workbench",
  "title": "Open Exo workbench",
  "uri": "<launch url>",
  "description": "Lane workspace for the selected Exo project and worktree",
  "mimeType": "text/html",
  "annotations": {
    "audience": ["user"],
    "priority": 1.0
  }
}
```

The text block remains first and contains the URL and expiration. The adapter
does not add the link on errors, previews, expired cached results, or other
commands. The launch URL is not added to `resources/list`, and the server does
not require `resources/read` support.

### Focus-only browser experience

The first browser screen is the current lane workspace, not a landing page. A
compact lane rail shows all existing lanes, their state and phase, and which one
is focused here. The main surface gives the focused lane's intent first visual
priority, then shows branch, HEAD, dirty state, phase, goals, tasks, steering,
and diagnostics in a dense work-oriented layout.

Only lane focus is mutable. The UI has no lane creation, start, removal,
parking, closure, task completion, phase lifecycle, pull-request management,
RFC management, validation history, daemon recovery, or settings controls. A
focus action remains pending until the `lane.focus` response commits. The UI
preserves the request ID across transport retries, renders Exo failures as
failures, and then refreshes the complete snapshot.

The cockpit has explicit loading, no-lane, no-focus, session-expired,
workspace-unavailable, transport-error, and diagnostic states. Missing state is
not filled from local storage. The session cookie is the only browser-retained
identity in the first slice.

### Frontend development and embedded distribution

`packages/exosuit-cockpit` becomes a client-rendered Svelte application. Its
SvelteKit configuration replaces `adapter-node` with `adapter-static` and uses
`fallback: "index.html"`. Production output is written to the package's
`build/` directory.

The Exo crate gains a default `ui` feature. With that feature enabled,
`tools/exo/build.rs` runs the pinned pnpm cockpit build when its inputs are newer
than the output, copies the build into `OUT_DIR`, and emits rerun directives for
the cockpit sources, package manifests, workspace manifest, and lockfile. Rust
uses `include_dir` or an equivalent compile-time embed over the copied output.
`--no-default-features` omits the assets and causes launch to return
`workbench.ui_unavailable`; official binaries use default features.

The embedded server serves hashed assets with immutable caching and serves
`index.html` with `no-store`. Unknown non-API GET routes fall back to
`index.html`; `/api/*` never falls back. Responses include `nosniff`,
`Referrer-Policy: no-referrer`, `frame-ancestors 'none'`, `base-uri 'none'`, and
a restrictive same-origin content security policy. Assets are addressed only
from the embedded manifest.

Development uses Vite on `127.0.0.1` with hot module replacement and proxies
`/api` to a diagnostic Exo daemon host. The capability and session flow remains
active in development; Vite is not an unauthenticated alternate API.

Because UI is a default Exo feature, every CI job that compiles the Exo binary
must have the pinned Node and pnpm toolchain and a frozen workspace install.
This includes the Exo Rust job, Windows Rust, and the binary matrix. Binary and
Windows classifiers include `packages/exosuit-cockpit/`, `pnpm-lock.yaml`, and
`pnpm-workspace.yaml`. Official artifact acceptance runs the installed binary
from a directory without frontend sources and verifies the embedded asset hash.

## Error and Recovery Contract

The adapter uses stable diagnostic kinds:

| Kind | Boundary |
| --- | --- |
| `workbench.daemon_required` | A workbench command ran without daemon runtime services |
| `workbench.host_unavailable` | The loopback host could not start or its task stopped |
| `workbench.ui_unavailable` | The binary was built without embedded UI assets |
| `workbench.ticket_invalid` | Ticket validation or one-time redemption failed |
| `workbench.session_invalid` | The session is missing, expired, or from another runtime |
| `workbench.origin_mismatch` | Host or Origin validation failed |
| `workbench.capability_denied` | The session attempted an operation outside its fixed set |
| `workbench.workspace_unavailable` | The retained workspace no longer resolves to this project |
| `workbench.busy` | Session, stream, body, or HTTP admission is saturated |

Authentication errors do not distinguish expired, redeemed, foreign, or
malformed credentials. They do not include steering that reveals local
workspaces. A stopped host does not trigger daemon replacement. Existing daemon
status and recovery remain the operator path.

Snapshot is replayable. `lane_focus` uses the existing atomic-project-state
recovery contract. The browser treats a connection loss as an unknown delivery
state and retries the same request ID after reconnecting; it never substitutes a
new request ID until the terminal response is known. The HTTP adapter neither
persists its own outcomes nor translates a retryable Exo response into success.

## Security Considerations

The loopback bind prevents remote network access but does not protect against
other local processes, browser cross-origin requests, leaked URLs, or DNS
rebinding. The exact numeric loopback origin, Host and Origin checks,
short-lived signed ticket, one-time redemption, fixed capabilities, HttpOnly
SameSite cookie, in-memory session state, body and concurrency bounds, and raw
path redaction act together. No single one is treated as sufficient.

The first version intentionally avoids a general HTTP command endpoint. A
session cannot smuggle arbitrary Exo syntax, workspace roots, confirmation
tickets, or workflow outcomes through a nominal lane action. Every allowed
operation is a closed enum translated by server code.

The launch URL is a bearer secret. Exo may display it to the invoking human or
agent host, but it must not include it in logs, diagnostics, telemetry, events,
SQL, sidecar data, crash messages, or resource catalogs. Browser code removes
the fragment immediately after exchange and sets `Referrer-Policy: no-referrer`
before rendering links.

This design does not provide remote access, multi-user identity, TLS, or an
organizational authorization model. Exposing the listener beyond loopback is a
new security design, not a configuration flag.

## Compatibility and Migration

Projects with no lanes can launch the workbench and see the explicit no-lane
state. No project-state migration is required. The host record, ticket map,
sessions, workspace keys, and revision counter are machine-local runtime data
and never enter portable or reactive SQL projection.

Existing CLI, MCP, machine-channel, VS Code, lane, daemon authority, admission,
outcome recovery, and workspace-validation behavior remains valid. The MCP
adapter's daemon routing change is limited to the `workbench` namespace. Adding
`ResourceLink` is backward compatible because every launch still includes a
text content block.

The workbench schema starts at version 1. Rust serialization is normative;
TypeScript runtime decoding and shared fixtures prevent silent drift. Future
additive fields do not change the version. Removing a field, changing its
meaning, or adding an operation requires a schema or capability version change.

## Relationship to Existing RFCs

RFC 10202 remains the authority for lane identity, lifecycle, phase
association, workspace focus, and the `exo lane` command surface. This RFC does
not revise those semantics; it adds a new client and launch boundary over them.

RFC 10193 remains the Codex integration and plugin-packaging record. It explains
how Codex selects an Exo workspace and presents MCP tools. This RFC owns the
host-neutral local workbench server and capability that a Codex adapter may
present.

RFC 10200 and RFC 10190 continue to own the CLI-shaped MCP transport and durable
MCP proxy. This RFC adds one standard content variant and requires the two
workbench commands to reach project daemon runtime services. It does not create
a second MCP server or a workbench-specific tool surface.

## Drawbacks

Serving HTTP from the daemon increases the runtime and security surface of a
process that owns important local state. Capability validation, cookies, static
asset serving, SSE, browser-origin behavior, and HTTP resource bounds all need
adversarial tests. The narrow route and operation sets limit the expansion but
do not erase it.

Embedding a Svelte application increases binary size and makes a reproducible
Node toolchain part of ordinary Exo compilation. It also creates Vite and
embedded development modes that can drift. Installed-binary acceptance must
cover the embedded path rather than treating the Vite experience as sufficient.

A local browser is more portable than a VS Code sidebar, but it is not a remote
or collaborative web application. The design deliberately favors local
workspace authority and a capability link over access from another machine.

Five-second visible polling is intentionally modest engineering rather than a
perfect filesystem event model. It bounds Git-state staleness without adding a
watcher, but it performs recurring local reads while the workbench is visible.

## Alternatives

The workbench could remain a VS Code-only surface. That would reuse existing
extension infrastructure, but it would keep the new direction inside a UI
organized around older concepts and would not give Codex-centered work a
natural visual companion.

A separate `exo-workbench` process could host the frontend. That keeps HTTP out
of the daemon, but it would need to rediscover project authority, coordinate
lifecycle, authenticate against the daemon, and avoid becoming a second
workspace selector. A host manager inside the project daemon is the smaller
product model.

An embedded MCP App could provide a richer in-host experience. Host support is
not universal, and it would make the foundational UI depend on one presentation
protocol. A normal local web application plus a standard resource link works in
an ordinary browser and can later be wrapped by embedded hosts.

The browser could call a broad CLI-shaped HTTP endpoint. That would make future
features easy to expose, but possession of a workbench session would become
general Exo command authority. The fixed operation union is safer and keeps UI
scope explicit.

The daemon could start the HTTP listener eagerly. Lazy startup avoids another
open port and frontend task for users who never launch the workbench while
retaining one host for the daemon lifetime.

## Implementation Plan

The host and launch task proceeds in four internal steps. First, add the
Workbench CommandSpec, runtime-service injection, shared daemon dispatcher, host
manager, capability/session model, HTTP routes, and runtime host record. Second,
route MCP workbench requests through the daemon and add resource-link output.
Third, convert and embed the cockpit build and update all relevant CI
classifiers and toolchain setup. Fourth, add installed-binary, security,
linked-worktree, and contract-fixture coverage before exposing the UI task.

The focus-only UI task then implements the snapshot decoder, session bootstrap,
lane rail, current-lane workspace, SSE invalidation, visible polling, focus
mutation, and all empty/error states. It does not widen the HTTP capability.

## Validation Strategy

Rust unit tests cover token signing and constant-time verification, expiration,
one-time redemption, session idle and absolute expiry, origin and host checks,
capability denial, resource bounds, path redaction, runtime-record generation
matching, and cleanup. Command and MCP tests cover pure/replayable metadata,
daemon-only execution, request workspace forwarding, textual fallback,
structured content, and the exact resource-link shape.

Daemon integration starts one project daemon, launches from two linked
worktrees, and proves one origin with distinct opaque workspace keys and
snapshots. It focuses a lane in one workspace with a retained request ID and
shows that the other focus is unchanged. Removing a worktree and reusing its
path for another repository invalidates its session. Daemon replacement
invalidates both sessions without changing project authority or silently
restarting again.

Event tests cover ready and invalidate revisions, broadcast lag, `Last-Event-ID`,
reconnect, session expiry, stream bounds, idle keepalive, and graceful host
shutdown. Snapshot tests cover no lanes, no focus, focus mismatch diagnostics,
detached HEAD, dirty worktree, Git changes within five seconds, and complete
path redaction.

Frontend tests use Vitest for protocol decoding, bootstrap, focus retries,
loading and error states. Playwright runs the embedded or production-equivalent
build at desktop and mobile sizes and checks nonblank rendering, text fit,
control overlap, lane switching, reconnect, expired session, and workspace
removal. Canvas is not part of the first UI.

Artifact acceptance builds and installs the default UI-enabled Exo binary,
runs it outside the source tree, launches a workbench, verifies the embedded
asset hash, and confirms that no Node process is required at runtime. Linux,
macOS, and Windows compilation all exercise the cockpit build prerequisites.

The end-to-end agent proof invokes `exo-run workbench launch` from a known Codex
workspace, verifies the text and resource-link results, opens the link, and
compares the rendered project, workspace, branch, head and focused lane with
`exo-run workbench snapshot`. The same proof repeats from a linked worktree with
a different focus.

Existing Exo library, daemon lifecycle, request outcome, context/RFC,
CommandSpec, machine-channel artifact, MCP stdio, TypeScript, Svelte, binary,
Windows, formatting, lint, and diff gates remain required.

## Stage and Future Work

This Stage 2 RFC is implementation-ready for the local host, launch and
focus-only workbench tasks described above. Reaching Stage 3 requires landed
code, installed-binary acceptance, linked-worktree isolation evidence, exact
MCP resource-link proof, frontend visual validation, terminal CI, and clean
exact-head review.

Lane creation, start and closure; task and phase mutation; attachments,
observations, validation freshness, review state, pull requests, RFCs, daemon
recovery, remote access, multi-user identity, and cloud hosting remain future
work. They must extend the lane model and capability surface explicitly rather
than arriving as incidental controls in the first browser client.
