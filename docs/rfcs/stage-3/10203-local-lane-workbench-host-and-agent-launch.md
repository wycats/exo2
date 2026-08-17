<!-- exo:10203 ulid:01kync6fa5cmyr7f4rmefj82hy -->

# RFC 10203: Local Lane Workbench Host and Agent Launch

**Feature**: lane-centered-workbench

## Summary

This RFC specifies a local web workbench for Exo's lane-centered model. The
workbench is hosted by the project-authority Exo daemon, opened through a
short-lived workspace-bound capability, and usable from an ordinary browser or
from an agent host that can present a link. Its first screen is the current
workbench lane: the durable intent, current Git state, associated phase, goals,
and tasks for the workspace that launched it. Agent guidance and diagnostics,
when present, are secondary coordination context rather than the user's task
list.

The design adds two Exo commands with different recovery boundaries.
`exo workbench launch` is an external-at-most-once write: it starts or reuses
the daemon's host and returns a one-time capability URL without placing that
bearer in durable recovery state. `exo workbench snapshot` is a pure,
replayable read of the data model behind the screen. MCP presents a successful
launch as normal text containing the URL and as structured launch data, so an
agent can give the user an openable workbench without opening a browser on the
user's behalf. Rich link presentation is deferred until the agent host and Exo
negotiate a content shape that host accepts.

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
one-hour enrollment-ticket expiration. The first launch starts the host;
deliberate later launches use new request IDs, reuse the host, and issue fresh
capabilities. Neither case changes canonical project state or opens a browser.
The enrollment ticket is only the browser's one-time entry credential; after
exchange, the independent renewable session lifetime governs the open cockpit.

If delivery of a launch response is ambiguous, the caller retries with the same
request ID. The issuing daemon returns the exact original response while its
one-time capability and every workspace, entry, host, and publication binding
remain current. It never creates a second capability for that request. Once the
capability is consumed or expired, the daemon is replaced, or any binding is no
longer exact, Exo fails closed and asks the caller to use a new request ID for a
deliberate fresh launch.

An agent invokes the same command through `exo-run`. MCP clients receive a
normal text result containing the complete URL and expiration, plus structured
launch data containing the same URL. This baseline works in hosts that reject
unknown rich-content variants. A future adapter may present a richer open action
after it has established that the host accepts that content shape. The user
remains in control of opening the link.

The URL selects one Exo project and one validated workspace. The browser opens
on that workspace's focused lane. If two linked worktrees focus different
lanes, launching from each worktree produces a workbench for the same project
but a different current stream. Neither link contains the raw workspace path,
and opening one cannot silently switch the focus of the other.

The first screen answers three connected questions: what lane this workspace is
advancing, which branch and commit it is advancing from, and which phase and
tasks supply the planning context. A lane rail makes other existing lanes
visible. Selecting a lane whose phase is still active invokes the existing
`lane focus` operation and refreshes the screen from the committed result.
Completed-phase lanes remain visible as history but cannot be focused.

When Exo supplies steering, the workbench may show the first suggested action as
a quiet agent-next-step summary. The steering situation and raw command stay
behind an agent-details disclosure, and diagnostics appear only when Exo reports
them. Neither surface is presented as a human attention queue. Exo does not yet
have enough explicit state to tell the user reliably what needs their attention,
so the first workbench makes no all-clear claim when those fields are empty.

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
    #[exo(effect = "write", description = "Launch the local lane workbench")]
    Launch,

    #[exo(effect = "pure", description = "Read the current lane workbench snapshot")]
    Snapshot,
}
```

`workbench launch` has `Effect::Write` and
`RecoveryClass::ExternalAtMostOnce`. Starting or reusing a host and issuing a
one-time capability changes machine-local authority even though it does not
mutate canonical project SQLite state, Git, or a remote system. A same-request
retry must therefore recover the original live result or fail closed; it must
not issue a fresh ticket. `workbench snapshot` remains `Effect::Pure` and
`RecoveryClass::ReplayableRead`.

Launch is excluded from canonical post-write persistence: it does not write a
project SQL dump, preflight or checkpoint a sidecar, advance the workbench
revision, or broadcast a project write. It does emit the ordinary secret-free
command event so operators can observe that a launch occurred without recording
its URL, ticket, display body, signing material, or workspace path.

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

The host manager normally begins unbound. The first launch serializes startup,
binds `127.0.0.1:0`, creates daemon-lifetime secret material, starts the HTTP
task on the existing Tokio runtime, and records the resulting origin.
Concurrent first launches converge on one host. Later launches reuse that host.

When a compatible replacement daemon loads live resumable grants, it also reads
the prior compatible host record. After installing its command dispatcher, it
attempts to bind the same loopback port before accepting browser traffic. This
keeps installed tabs and stable development proxies on one origin across an
ordinary daemon replacement. The retained origin is only a routing hint: the
new host creates fresh daemon-lifetime secret material and authenticates every
request through the restored session grant. Compatibility follows the persisted
session and host-record schema rather than the embedded asset hash, so a normal
cockpit upgrade can retain its origin. A malformed origin is ignored. If the
preferred port is unavailable, the daemon falls back to a fresh ephemeral
loopback port so a new launch can still succeed.

A bind or server startup failure does not stop the machine-channel daemon;
launch returns `workbench.host_unavailable` with a stable diagnostic.

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

The record contains no secret and no workspace root. Readers accept it as live
status only when instance ID, PID, and process-start identity match the exact
current daemon. Clean shutdown marks the owned record inactive rather than
removing it. A compatible replacement may use its numeric loopback port as a
recovery hint only when the exact project also has live durable session grants
with compatible schemas. Other stale or malformed records are ignored.

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
  schema_version: 2;
  launch_mode: "direct_loopback" | "published";
  url: string;
  expires_at: string;
  expires_in_seconds: 3600;
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
The human result labels it as a one-time browser enrollment link with a
one-hour lifetime and does not print the raw workspace path. `workspace.key` is
an opaque, random daemon-lifetime identifier mapped to the validated canonical
workspace root in memory. `workspace.label` is the branch name when attached
and `detached@<short-head>` otherwise. `launch_mode` distinguishes the original
numeric-loopback entry from RFC 10206's published canonical entry; it does not
change the response schema or the capability's one-time semantics.

The URL has the form:

```text
http://127.0.0.1:<port>/#ticket=<ticket>
```

No ticket, URL, display body, signing material, session identifier, or cookie
value is written to daemon identity, health, diagnostics, command events,
project SQL, sidecar projections, host records, workspace registrations, or
authorization stores.

### Launch request recovery

The daemon outcome ledger reserves `workbench launch` like any other
external-at-most-once operation, but successful completion uses a
launch-specific adapter. The ledger stores only a typed, secret-free completion
marker. The exact `ResponseEnvelope`, including text and structured URL
presentation, remains in daemon-lifetime memory associated with the original
request ID and pending capability. At issuance, that capability captures the
exact workspace root, workspace-registration generation, host generation, and
entry binding; replay retention cannot replace those facts with a later
runtime snapshot.

A terminal pre-dispatch retry and a concurrent waiter both resolve the marker
through that live adapter after the ledger has verified the request hash. The
adapter returns the original envelope, changing only transport-level response
ID normalization, when all of these facts are still current:

- the capability exists, is unconsumed, and has not expired;
- the exact workspace registration and canonical root still match;
- the exact entry mode and canonical origin still match;
- the issuing host generation is alive; and
- for a published entry, current publication authority is ready for that exact
  project instance, workspace, origin, and listener generation.

If the memory entry is missing, the daemon instance changed, or any validation
fails, the marker produces `workbench.launch_replay_unavailable` with
`retry_with_new_request_id: true`. The same request ID is terminal and never
re-executes. The secret-free marker is retained beyond the generic completed
outcome retention window so that boundary remains durable. A non-successful
launch response is returned once but is not retained as live capability
authority, so retrying that request ID reaches the same fail-closed boundary.
Ordinary external-at-most-once commands continue to persist and replay their
response envelopes unchanged.

Host shutdown fences both replay retention and replay reads, clears all cached
launch responses when shutdown begins, and rechecks the fence after external
publication validation. Published replay additionally requires a provider that
is not shutting down. A replacement daemon may restore a durable pairing and
its exact publication, but it cannot reconstruct the prior daemon's launch
response; the old marker fails closed while a launch issued by the replacement
can establish its own same-daemon replay.

The outcome database and any live WAL or shared-memory companions are
owner-only. On startup, a legacy successful launch row that contains a raw URL
is replaced by the marker with secure deletion and WAL truncation; it is never
replayed as bearer authority after daemon replacement.

### Launch ticket

At first host startup, Exo reads 32 bytes from the operating system CSPRNG for
an in-memory HMAC key. A ticket is:

```text
v1.<base64url(payload-json)>.<base64url(hmac-sha256(payload-json))>
```

This version-1 form is the direct-loopback ticket. RFC 10206 extends the same
one-time boundary with a version-2 published ticket that additionally binds the
locald project instance and canonical origin.

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

Times are Unix seconds. `expires_at` is exactly 3,600 seconds after `issued_at`.
`capability_id` is 256 bits of random data encoded without padding. The host
keeps a pending capability record that maps the ID and workspace key to the
validated root. A successful exchange consumes the pending record. A ticket is
therefore signed, short-lived, workspace-bound, daemon-bound, and one-time.
The one-hour pre-redemption window accommodates asynchronous handoff between an
agent and human without changing the credential's authority or the lifetime of
the browser session created by a successful exchange.

While at least one enrollment ticket remains unexpired and unredeemed, its
pending capability keeps the issuing daemon and loopback host alive. Redemption
or expiration releases that hold; without authenticated event-stream activity,
the daemon then returns to its ordinary idle-shutdown policy.

Signature comparison is constant-time. Invalid signature, unknown capability,
wrong instance, wrong project, expired ticket, and previously redeemed ticket
all return the same public `workbench.ticket_invalid` response. This prevents
the endpoint from becoming an oracle for local workspace existence.

### Browser session

The static application reads the fragment, clears the retained session selector,
and calls `history.replaceState` to remove the fragment before exchanging the
ticket through `POST /api/session`. It never submits the same one-time ticket
again after an ambiguous transport outcome. If the server explicitly returns
`workbench.busy`, it has not consumed the ticket, so the application may retry
that ticket while it remains valid. If delivery of the exchange response is
ambiguous, the application asks for a fresh launch link. A later ticket fragment
in the same browser tab starts a fresh exchange through `hashchange`.
History traversal starts a fresh client bootstrap through `popstate` whenever
the restored session selector differs from the current client, so Back and
Forward cannot leave the screen attached to the session from another entry.

On success the application retains the returned public random session key in
same-entry history state. The server sets an independent random 256-bit session
secret in a cookie named
`exo_workbench_session_<session-key>`. The cookie has `HttpOnly`,
`SameSite=Strict`, `Path=/`, no `Domain`, and `Max-Age=43200`. The session key
selects the matching cookie; it is not authorization without the independent
secret cookie value. Per-session names prevent two linked-worktree tabs on the
same loopback host from replacing each other's credentials. The initial
transport is HTTP loopback, so the first version does not claim a `Secure`
cookie that the server cannot portably enforce.

A session has a renewable twelve-hour grant and a thirty-minute idle lifetime.
Authenticated commands, snapshot polls, and SSE keepalives refresh activity.
The browser renews an active grant at a bounded interval, which extends both the
server grant and the cookie lifetime. An inactive session still expires after
thirty minutes.

The host stores resumable grants in the project runtime directory as
`workbench.sessions.json`. This machine-local file is atomically replaced with
owner-only permissions. It contains a digest of the cookie secret, the public
selector, exact project and workspace identity, validated canonical workspace
root, fixed capabilities, creation time, last activity, and grant expiration.
The raw cookie secret is never written to disk.

A compatible replacement host loads only the store for its exact project. When
the browser presents both credentials, the host re-resolves the retained root
and requires the same project ID, state root, workspace key, and canonical
worktree before hydrating the session into memory. It never substitutes the
host-launching worktree or another linked worktree. Expired, malformed,
foreign-project, and no-longer-resolvable grants fail closed. Completion-review
records remain process-local, so a review interrupted by replacement must be
prepared again from the current snapshot.

While reconnection is underway, the application keeps its last successful
snapshot visible and disables focus and planning mutations. A compatible
replacement reclaims the prior loopback origin when possible, restores the
session, and requires a fresh authoritative snapshot before mutations resume.
The replacement is therefore invisible once renewal and refresh succeed. If
the origin cannot be reclaimed, a fresh launch still exposes the replacement
host; a development proxy or old direct-origin tab may require explicit route
recovery. If the exact grant or workspace can no longer be recovered, the
cockpit remains visible in read-only form with a compact fresh-launch boundary
instead of replacing the workspace with a terminal error screen.

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
POST /api/session/renew
                    renew the current workspace-bound session grant
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

`POST /api/session/renew` accepts the existing public `session_key`. The
independent cookie secret remains browser-managed and is not present in the
body. A successful renewal returns the same result shape, refreshes
`expires_at`, and repeats the cookie with a fresh `Max-Age`.

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

A well-formed authenticated command returns a browser-safe projection of the
Exo `ResponseEnvelope` with HTTP status 200. Successful results retain the
snapshot or lane result but omit post-write persistence reports and
operator-facing trace or steering fields. Exo errors retain their status and
error code while replacing messages and details with stable path-free browser
text. Transport, origin, session, body shape, capability, and HTTP-admission
failures use HTTP status and a small error object before Exo dispatch.

When delivery of a `lane_focus` request is ambiguous, the browser retries with
the same request ID so the daemon outcome ledger remains the recovery authority.
A `workbench.busy` response from the HTTP adapter is known not to have entered
Exo dispatch; it is retryable without claiming a ledger reservation, and a
session exchange may reuse its unconsumed ticket. Once the browser receives an
Exo response, that response is terminal. A deliberate later re-execution uses a
new request ID.

Each session command revalidates the retained workspace root against the
current project ID and state root before dispatch and requires the resolved
worktree root to equal the root retained by the session. A removed linked
worktree, a path reused by a foreign repository, or a retained path that now
resolves to another linked worktree returns
`workbench.workspace_unavailable`. The server never falls back to the daemon's
startup workspace.

### Snapshot schema

`workbench snapshot` returns:

```typescript
interface WorkbenchSnapshot {
  kind: "workbench.snapshot";
  ok: true;
  schema_version: 2;
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
  between_phases_context: WorkbenchBetweenPhasesContext | null;
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

interface WorkbenchBetweenPhasesContext {
  epoch_id: string;
  epoch_title: string;
  completed_phase: {
    id: string;
    title: string;
    completed_at: string;
    goal_count: number;
    completed_goals: number;
  } | null;
  next_phase: {
    id: string;
    title: string;
    goal_count: number;
    rfc_count: number;
  } | null;
  pending_phases: number;
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

Snapshot composition validates the workspace once, reads all lane, focus,
phase, goal, and task state through one SQLite read transaction, and then
samples Git from that same root. It does not claim a transaction across SQLite
and Git. `observed_at` records when sampling completed. Results expose branch
and commit identity but omit workspace root, Git directory, database path,
state root, sidecar path, runtime path, and process identity.

`phase` is the phase associated with the focused lane. When no lane is focused,
`focused_lane` and `phase` are null while the lane rail remains available. An
inconsistent legacy lane/phase focus is returned as a diagnostic using RFC
10202's read behavior; the snapshot does not repair it.

When a workspace remains anchored to a phase that has completed while its epoch
still has pending work, `between_phases_context` presents the evidence-backed
most recent completion, the next pending phase in roadmap order, and the number
of pending phases. The completion summary is selected by its persisted
completion timestamp rather than by roadmap position. Historical rows without
completion evidence do not become the most-recent claim. Completion-log prose
is intentionally omitted from this browser-safe projection. An in-progress
phase without a focused lane is not a between-phases state.

### Events and freshness

The daemon keeps an in-memory `u64` workbench revision initialized to zero. It
increments after every response whose effect records a committed canonical
project write, including a response that reports post-commit persistence or
outcome-finalizing failure, and broadcasts the new value. Machine-local
`workbench launch` authority is deliberately excluded even though the command
is classified as a write for recovery. Existing machine clients still receive
the current `write_happened` notification for canonical writes; the event
payload expansion is internal to the daemon.

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

A successful `workbench launch` MCP result has one ordinary text content block.
That text contains the complete launch URL and its expiration. The result also
includes `structuredContent`, whose `result` is the same versioned
`workbench.launch` value returned by the command, including the exact URL. An
agent can therefore show or link the textual URL, while an adapter can use the
structured value without scraping prose.

The baseline adapter does not add an unconditional `resource_link` content
block. Although the negotiated MCP protocol revision defines that content type,
real hosts may reject a tool result containing a variant they do not implement.
Rich-link presentation requires a later host-capability contract or another
negotiated extension. Adding it in a capable host must preserve the text and
structured URL contract defined here.

The launch URL is not added to `resources/list`, and the server does not require
`resources/read` support. Errors, previews, expired cached results, and other
commands never receive workbench launch presentation.

### Focus-only browser experience

The first browser screen is the current lane workspace, not a landing page. A
compact lane rail shows all existing lanes, their state and phase, and which one
is focused here. The main surface gives the focused lane's intent first visual
priority, then shows branch, HEAD, dirty state, phase, goals, and tasks in a
dense work-oriented layout. Only lanes whose phase is `in-progress` can be
focused. Pending and completed-phase lanes remain available as context but
their focus controls are disabled.

When the workspace remains anchored to a completed phase and the epoch still
has pending work, the main surface becomes a project-trajectory view rather
than a generic no-focus prompt. It presents the evidence-backed most recent
phase as **Just finished**, gives the next pending phase stronger **Up next**
priority, and lists prepared lanes belonging to that next phase without making
them focusable before the phase starts. A workspace whose phase is still
`in-progress` but has no focused lane retains the ordinary no-focus state.

An optional Coordination rail contains secondary machine context. If steering
contains a suggested action, the rail shows only the first action's label and
rationale as **Agent next step**. Its situation and raw command are collapsed
under **Agent details**. Diagnostics render only when non-empty, and the whole
rail is absent when neither signal exists. The workbench does not derive or
display a human-attention queue in this version.

Only lane focus is mutable. The UI has no lane creation, start, removal,
parking, closure, task completion, phase lifecycle, pull-request management,
RFC management, validation history, daemon recovery, or settings controls. A
focus action remains pending until the `lane.focus` response commits. The UI
preserves the request ID across ambiguous transport retries. Once Exo returns a
terminal failure, an explicit user retry receives a new request ID. The UI
renders failures as failures and then refreshes the complete snapshot.

The cockpit has explicit loading, no-lane, no-focus, session-expired,
workspace-unavailable, transport-error, and diagnostic states. Missing state is
not filled from local storage. The browser retains only the public session
selector in same-entry history state and the independent HttpOnly session
cookie; the selector is not authorization on its own.

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
`/api` to the Exo workbench origin selected by
`EXO_WORKBENCH_DEV_ORIGIN`. That origin may belong to any compatible
project-authority daemon; diagnostics are not a prerequisite. The capability
and session flow remains active in development, so Vite is not an
unauthenticated alternate API.

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
| `workbench.launch_replay_unavailable` | The original launch response no longer has matching live capability and routing authority; retry with a new request ID |
| `workbench.ticket_invalid` | Ticket validation or one-time redemption failed |
| `workbench.session_invalid` | The session is missing, expired, or cannot be revalidated for the exact project and workspace |
| `workbench.origin_mismatch` | Host or Origin validation failed |
| `workbench.capability_denied` | The session attempted an operation outside its fixed set |
| `workbench.workspace_unavailable` | The retained workspace no longer resolves to this project |
| `workbench.busy` | Session, stream, body, or HTTP admission is saturated |

Authentication errors do not distinguish expired, redeemed, foreign, or
malformed credentials. They do not include steering that reveals local
workspaces. A stopped host does not trigger daemon replacement. Existing daemon
status and recovery remain the operator path.

Snapshot is replayable. Launch uses the live external-at-most-once adapter above.
`lane_focus` uses the existing atomic-project-state recovery contract. The
browser treats a connection loss as an unknown delivery state and retries the
same request ID after reconnecting; it never substitutes a new request ID until
the terminal response is known. A deliberate retry after a known terminal
response is a new request. The HTTP adapter neither persists its own outcomes
nor translates a retryable Exo response into success.

## Security Considerations

The loopback bind prevents remote network access but does not protect against
other local processes, browser cross-origin requests, leaked URLs, or DNS
rebinding. The exact numeric loopback origin, Host and Origin checks,
short-lived signed ticket, one-time redemption, fixed capabilities, HttpOnly
SameSite cookie, owner-only digest-based grant persistence, exact workspace
revalidation, body and concurrency bounds, and raw path redaction act together.
No single one is treated as sufficient.

The first version intentionally avoids a general HTTP command endpoint. A
session cannot smuggle arbitrary Exo syntax, workspace roots, confirmation
tickets, or workflow outcomes through a nominal lane action. Every allowed
operation is a closed enum translated by server code.

The launch URL is a bearer secret. Exo may display it to the invoking human or
agent host and retain that exact response in issuing-daemon memory for bounded
same-request recovery, but it must not include it in logs, diagnostics,
telemetry, events, SQL, outcome DB/WAL/shared-memory files, sidecar data, host or
authorization stores, crash messages, or resource catalogs. Browser code
removes the fragment immediately after exchange and sets
`Referrer-Policy: no-referrer` before rendering links.

This design does not provide remote access, multi-user identity, TLS, or an
organizational authorization model. Exposing the listener beyond loopback is a
new security design, not a configuration flag.

## Compatibility and Migration

Projects with no lanes can launch the workbench and see the explicit no-lane
state. No project-state migration is required. The host record, ticket map,
sessions, workspace keys, and revision counter are machine-local runtime data
and never enter portable or reactive SQL projection.

Existing CLI, MCP, machine-channel, VS Code, lane, daemon authority, admission,
and workspace-validation behavior remains valid. The launch-specific marker is
an internal exception inside the existing external-at-most-once ledger; generic
external mutations retain their durable response replay behavior. The MCP
adapter's daemon routing change is limited to the `workbench` namespace. Text
content and structured launch data use existing tool-result fields and avoid
requiring a client to accept a new content variant.

The launch schema is version 2 after RFC 10206 added `launch_mode`; the original
snapshot and HTTP protocol schemas retain their independently versioned
contracts. Rust serialization is normative; TypeScript runtime decoding and
shared fixtures prevent silent drift. Future additive fields do not change the
version. Removing a field, changing its meaning, or adding an operation requires
a schema or capability version change.

## Relationship to Existing RFCs

RFC 10202 remains the authority for lane identity, lifecycle, phase
association, workspace focus, and the `exo lane` command surface. This RFC does
not revise those semantics; it adds a new client and launch boundary over them.

RFC 10193 remains the Codex integration and plugin-packaging record. It explains
how Codex selects an Exo workspace and presents MCP tools. This RFC owns the
host-neutral local workbench server and capability that a Codex adapter may
present.

RFC 10200 and RFC 10190 continue to own the CLI-shaped MCP transport and durable
MCP proxy. This RFC uses their existing text and structured-result fields and
requires the two workbench commands to reach project daemon runtime services.
It adds no MCP content variant, second MCP server, or workbench-specific tool
surface.

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
protocol. A normal local web application plus a textual and structured launch
URL works in an ordinary browser and can later be wrapped by embedded hosts or
presented as a negotiated rich link.

The browser could call a broad CLI-shaped HTTP endpoint. That would make future
features easy to expose, but possession of a workbench session would become
general Exo command authority. The fixed operation union is safer and keeps UI
scope explicit.

The daemon could start the HTTP listener eagerly. Lazy startup avoids another
open port and frontend task for users who never launch the workbench while
retaining one host for the daemon lifetime.

## Implementation

The implementation landed in two bounded slices. PR #57 established the
`workbench` commands, shared daemon dispatcher, lazy loopback host, capability
and session model, closed HTTP API, runtime observability record, embedded
adapter-static cockpit assets, MCP daemon routing, and the text-plus-structured
launch result. It also added the security, artifact, and linked-worktree
contract coverage needed to make the host usable outside the source tree.

PR #58 completed the focus-only workspace. It added the version-one snapshot
decoder, workspace session bootstrap, lane rail, current-lane workspace, SSE
invalidation, visible-state polling, request-ID-preserving focus mutation, and
explicit empty and error states. The same slice made agent guidance secondary
Coordination context and removed the unsupported MCP `resource_link` content
block while preserving the launch URL in text and structured output.

Both slices retain Exo as the only project, session, and command authority. The
browser does not own a parallel state model, and the implemented HTTP
capability remains limited to snapshot reads and lane focus.

The launch-recovery hardening keeps that authority one-time across ambiguous
machine-channel delivery. It adds a typed durable completion marker, an exact
daemon-memory response cache bound to live capability and routing facts, and a
launch-only replay adapter at both terminal recovery points. It also separates
launch from canonical post-write persistence and revision signals while
retaining a secret-free command event.

## Validation Evidence

Rust unit and integration coverage exercises ticket signing and redemption,
session expiry, origin and host checks, capability denial, resource bounds,
path redaction, runtime-record generation matching, daemon-only command
execution, MCP presentation, snapshot coherence, event invalidation, and host
cleanup. The linked-worktree daemon suite proves that two worktrees share one
project host while retaining distinct workspace keys, snapshots, and focus.

Launch-recovery coverage additionally proves exact same-daemon replay without a
second capability, concurrent-waiter replay, and fail-closed behavior after
consumption, expiry, daemon replacement, workspace re-registration, entry or
origin mismatch, host-generation change, and publication-authority loss. It
also preserves ordinary external-mutation replay and checks that raw legacy
launch rows are scrubbed, outcome files are owner-only, and bearer material is
absent from durable outcome, event, host, workspace, authorization, SQL, and
sidecar surfaces.

The cockpit is covered by Vitest protocol and interaction tests, Svelte
validation, and a production build. Post-merge dogfooding then ran the Vite
application through machine-local locald against the real project-authority
host. A real browser rendered authoritative merged-main state at desktop and
mobile widths, and an approved Exo write advanced the visible revision through
SSE without a page reload.

PR #57 artifact acceptance copied the release binary outside the source tree,
launched its embedded UI, verified the embedded asset hash, and exercised the
session and snapshot flow without a runtime Node process. PR #58 post-merge
acceptance installed the merged binary, rebuilt the selected workspace
development binary, established matching daemon authority, and invoked
`exo-run workbench launch` through Codex. The result contained an openable URL
in ordinary text and the same versioned value in structured output.

A disposable linked-worktree project supplied the isolation proof. Both
worktrees used one daemon and host while preserving separate focus. Replaying
the retained request ID did not repeat the mutation, SSE delivered the committed
invalidation, polling observed a later Git change, and the sibling worktree's
focus remained unchanged. The two merged slices also passed their required
platform, artifact, formatting, and exact-head review gates.

## Stage and Future Work

This RFC is a Stage 3 Candidate. PR #57 landed the local host and launch
substrate as `f058ff6d8e21c8f07c759e1bf5f5da205b5439fe`; PR #58 landed the
focus-only workspace and Codex-compatible launch result as
`b99e568ec6dbdbda118d0f6f46dc63a07d960866`. The implementation and evidence
above cover the candidate contract across installed binaries, linked
worktrees, the real browser, locald-supervised development, required CI, and
exact-head review.

Lane creation, start and closure; task and phase mutation; attachments,
observations, validation freshness, review state, pull requests, RFCs, daemon
recovery, remote access, multi-user identity, and cloud hosting remain future
work. A trustworthy human-attention queue and negotiated rich-link presentation
also remain future work. They must extend the lane model and capability surface
explicitly rather than arriving as incidental controls in the first browser
client.
