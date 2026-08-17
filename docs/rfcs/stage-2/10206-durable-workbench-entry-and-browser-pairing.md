<!-- exo:10206 ulid:01kzf8x4ndna7bzsqz0k24a7f7 -->

# RFC 10206: Durable Workbench Entry and Browser Pairing

## Summary

The Exo workbench should be a place a person can return to, not a link they must
race to open again.

RFC 10203 established the implemented local host, one-time launch ticket,
renewable browser session, and exact workspace binding. This RFC adds two
longer-lived concepts around that session model:

1. a stable, non-secret entry origin for a declared workbench service; and
2. a durable, revocable pairing between one browser profile and one exact Exo
   project workspace.

The launch ticket becomes an enrollment and recovery credential. Redeeming it
pairs the browser and creates the first active session. Later visits to the bare
stable origin use the pairing to create a fresh bounded session without placing
another bearer credential in the URL.

Exo remains the workspace, browser-capability, session, pairing, and command
authority. A route provider such as locald owns the declared service identity,
canonical HTTPS origin, TLS termination, route health, and atomic route-binding
commit. Exo owns its listener and host lifecycle, application authorization,
per-worktree publication orchestration, and every response containing project
state.

## Motivation

The current launch flow protects the right things. A ticket is signed,
daemon-bound, project-bound, workspace-bound, capability-limited, one-time, and
time-limited. The browser exchanges it for an independent HttpOnly credential,
and the host revalidates the exact workspace before restoring a session after a
daemon replacement.

The product experience nevertheless exposes the bootstrap mechanism too often.
A person can return to a tab after the active session or host has expired, find
that a numeric loopback port has changed, and need the agent to produce another
secret URL merely to resume observing the same workspace. That makes the
cockpit feel temporary even though the project, physical worktree, lane, and
browser are all durable enough to support continuity.

Lengthening every credential does not solve the underlying problem. An unused
bearer link and an already paired browser are different kinds of authority. The
link should remain useful only for bounded enrollment. The paired browser should
be able to return over a much longer period, while active sessions remain short
enough to expire naturally when the cockpit is not in use.

The same distinction clarifies routing. A stable workbench origin identifies
where one declared service for one locald project instance can be reached. It is
not authorization. Discovering or bookmarking that origin reveals no project
state unless the browser presents a valid Exo pairing or enrollment ticket.

## Guide-Level Experience

### First entry pairs the browser

The user or agent asks Exo for the current worktree's cockpit:

```text
exo workbench launch
```

Exo ensures its daemon, validates the selected workspace, resolves the entry
mode, prepares the route-facing listener, and returns an enrollment URL. In
published mode the URL uses the canonical locald origin. Its fragment carries a
short-lived, one-time Exo ticket.

Opening the enrollment URL is the user's enrollment action. The browser removes
the fragment, exchanges the ticket, stores a host-only HttpOnly pairing cookie,
and receives the first active session. The cockpit opens on the selected
workspace. An adapter that can navigate automatically must ask before opening a
ticket that creates durable pairing authority.

The cockpit quietly confirms the paired workspace and offers a way to forget
the pairing. It does not ask the user to manage ticket, pairing, or session
terminology during the normal flow.

### Returning uses the stable origin

After pairing, the user can bookmark or reopen the bare worktree origin:

```text
https://workbench.<instance>.<project>.localhost
```

The exact hostname remains locald-defined. It contains no ticket, Exo workspace
key, filesystem path, lane identity, or secret. The browser presents its
pairing cookie. Exo revalidates the project, locald project instance, physical
worktree, canonical origin, and pairing grant, then creates a fresh active
session for that tab.

If an active session is still valid, ordinary renewal continues under RFC
10203. If only the pairing remains valid, the browser resumes without asking for
another launch URL. Different tabs receive distinct active sessions under the
same pairing.

A lane change does not change the origin. The physical worktree is the stable
routing and workspace boundary; its focused lane remains mutable Exo state.

### Provider downtime does not erase pairing

The stable origin can outlive one Exo host. When no publisher is active, locald
reports only endpoint availability. It does not assert or evaluate whether the
browser is paired, and it never clears pairing cookies. Once an authorized Exo
workflow republishes the endpoint, Exo validates the retained pairing and the
same tab can resume.

Locald does not start, stop, signal, restart, or select Exo. An agent, CLI
invocation, or editor adapter invokes `workbench launch` or an equivalent
Exo-owned ensure operation. When a replacement Exo daemon restores a live
published pairing, daemon startup is that ensure operation: Exo revalidates the
exact retained workspace, locald project instance, and canonical origin before
reacquiring publication. A retained pairing is the durable publication intent;
Locald does not infer it and provider-triggered activation remains outside this
RFC.

### Recovery and revocation stay understandable

A fresh enrollment link remains available when the browser has never been
paired, the browser forgot its local pairing state, durable authority was
revoked or expired, the worktree was removed and recreated, or Exo can no
longer prove the exact project-instance/workspace/origin binding.

The cockpit distinguishes temporary route unavailability from an invalid
pairing. It retains the last valid view when safe, disables mutations while
authority is unavailable, and asks for fresh enrollment only after Exo returns
a terminal pairing result.

Users can list, name, and revoke pairings through both the Exo CLI and the
cockpit. The CLI can also forget a retained pairing record after invalidating
its authority. The CLI is the recovery baseline; the cockpit is the ordinary
surface. Revocation affects browser access, not project state, lane focus,
tasks, RFCs, Git, or another workspace's pairing.

## Detailed Design

### Three separate identities

#### Stable entry identity

The stable entry identity answers:

> Which declared workbench service belongs to this locald project instance?

In published mode it is the canonical primary origin for the declared
`workbench` service identified by locald's `(ProjectInstanceId, ServiceName)`.
It is non-secret and stable across lane changes, branch renames, rebases,
detached HEAD, daemon replacement, private port changes, and worktree moves that
preserve the locald project instance. Removing and recreating the worktree
creates a new project instance and cannot inherit the old origin authority by
path coincidence.

A worktree move can preserve that identity, but a browser request cannot claim
the move. After the move, an authenticated Exo launch or ensure operation must
resolve the moved workspace, prove the same locald `ProjectInstanceId`, and
atomically replace the retained workspace root and key. That replacement
invalidates sessions and successful resume outcomes derived from the former
workspace binding. A terminal request outcome already returned to a browser
remains replay-stable for its short retention window. A bare request to the old
or new origin never relocates authority.

Locald aliases are redirect-only. Exo authorizes, pairs, and sets cookies only
for the canonical primary origin returned by the publisher protocol.

#### Browser pairing identity

The browser pairing answers:

> May this browser profile create active sessions for this exact workspace?

A pairing has a 256-bit random public selector and an independent 256-bit random
secret. The browser stores both in one host-only HttpOnly cookie. Exo persists
only a digest of the secret and the binding metadata described below. A pairing
is machine-local authorization; it is not portable project state and never
travels through Git, the sidecar, or SQL projection.

#### Active session identity

The active session answers:

> What may this browser tab do right now?

Active sessions retain RFC 10203's 12-hour renewable lifetime, 30-minute idle
expiry, per-tab selector and secret, capability checks, workspace revalidation,
request limits, and mutation-recovery rules. A valid pairing can mint a fresh
session; it does not make that session permanent or expand its authority.

### Entry modes

Every ticket, pairing, session, persisted grant, and authenticated request binds
an explicit entry mode:

- `published` binds an exact Exo workspace, locald `ProjectInstanceId`, and
  canonical HTTPS origin;
- `direct_loopback` binds an exact Exo workspace and numeric loopback origin and
  records no locald instance.

Durable pairings are created only in `published` mode. Direct loopback preserves
RFC 10203's ticket-and-session behavior without a long-lived pairing cookie.
This avoids granting a browser profile months of authority to a numeric HTTP
port that an unrelated same-user process might later bind.

The UI labels direct mode honestly. It is useful when the project has not opted
into a published service or locald is authoritatively absent, but it does not
claim durable entry.

Ticket versions make this boundary portable. Version-1 tickets remain
direct-loopback-only and contain no published-origin authority. A published
launch emits an explicit version-2 ticket that binds `entry_mode`, the exact
locald `ProjectInstanceId`, and the canonical origin in addition to RFC 10203's
daemon, project, workspace, capability, and lifetime fields. A version-1 ticket
is never upgraded into published authority by the route that receives it.

### Authorization store

The machine-local version-2 authorization store is
`runtime/workbench.authorizations.json`. It is separate from the legacy
`runtime/workbench.sessions.json` path so an older binary cannot overwrite or
discard pairings it does not understand. Version 2 contains three bounded
collections:

```json
{
  "schema_version": 2,
  "project_id": "...",
  "sessions": [],
  "pairings": [],
  "resume_outcomes": []
}
```

Each pairing record contains:

```text
selector
credential_digest
project_id
workspace_key
workspace_root
launch_mode = "published"
project_instance_id
canonical_origin
capabilities
created_at
last_used_at
idle_expires_at
absolute_expires_at
nickname?
revoked_at?
```

`workspace_root` is retained only in the owner-protected machine-local record so
Exo can re-resolve the exact physical worktree. It is never projected to the
browser, locald status, logs, sidecar, or portable documents. The
`credential_digest` is lowercase BLAKE3 hex over the raw pairing secret. Raw
pairing and session secrets are never persisted.

Version 2 is written through a mode-0600 temporary file, `sync_all`, and atomic
rename. Pairing creation, last-used advancement, session-grant creation, and the
corresponding resume outcome commit in one store replacement before success is
reported. Failure before that replacement leaves no committed authority.

At most 64 active pairings exist per project and at most 8 per workspace.
Expired active records are removed before enforcing those limits. A full active
set returns `workbench.pairing_limit` without eviction or partial mutation.

Revoked records use a separate retention bound of 64 per project and 8 per
workspace. When that bound is exceeded, Exo removes the oldest revoked records;
active authority is never evicted to make room for history. A nickname is
optional, is at most 80 Unicode scalar values, and carries no authority.

### Enrollment protocol

Published enrollment uses `POST /api/pairing/enroll` with an exact Origin and
Host match and a body no larger than 4 KiB:

```json
{
  "schema_version": 1,
  "ticket": "v2...."
}
```

The host verifies and consumes the one-time version-2 ticket, revalidates its
project, workspace, locald project instance, canonical origin, and fixed
capabilities, then creates the pairing and first session in one
authorization-store commit. It returns the ordinary session result and sets
both cookies. The published active-session cookie and pairing cookie are both
`Secure`, `HttpOnly`, host-only, `SameSite=Strict`, and scoped to `Path=/`.

If the browser already presents a valid pairing for the same binding and fixed
capability set, enrollment reuses that pairing and creates a fresh active
session. If the capability set differs, successful enrollment atomically
revokes the old cookie-selected pairing and creates a replacement. Pairing
authority is never widened by an ordinary resume.

An ambiguous enrollment response follows the ticket's existing one-time
boundary. The server never repeats a committed pairing mutation. If it cannot
re-establish the committed cookie after a process loss, it returns a terminal
fresh-enrollment result; it does not guess that no authority escaped.

The legacy `POST /api/session` route remains available in direct mode and for
older clients. It creates only an RFC 10203 session.

### Pairing resume and exact replay

Published resume uses `POST /api/pairing/resume`:

```json
{
  "schema_version": 1,
  "request_id": "<43-character base64url value>"
}
```

The browser generates the request ID from 32 random bytes before the first
attempt and retains it until a terminal response. The body contains no workspace
selector or requested capabilities. Exo derives authority from the pairing
cookie and canonical origin.

A successful resume:

1. validates exact Host and Origin;
2. parses the pairing selector and secret from the cookie;
3. verifies the stored secret digest;
4. re-resolves the retained project and physical worktree;
5. confirms the same locald project instance and canonical origin;
6. checks revocation and both expiry bounds;
7. advances last-used evidence without exceeding absolute expiry; and
8. creates a session with no capabilities beyond the pairing's fixed set.

The session selector and secret are deterministic pseudorandom values derived
from the authenticated pairing secret, the request ID, and distinct domain
separation labels. Exo persists only their selectors and digests. A retry with
the same pairing and request ID can therefore re-establish the same session
cookie and byte-equivalent successful result without storing raw secrets or
minting a second session.

Resume outcomes are keyed by `(pairing_selector, request_id)`. A successful
outcome is retained until the resulting session expires and never longer than
24 hours. Terminal resume failures are retained for five minutes and
replay the same failure even if an authenticated launch subsequently reconciles
the worktree binding. There are at most 32 retained outcomes per pairing and 256
per project. Exo prunes expired outcomes before enforcing the bounds; if no safe
slot is available, it returns `workbench.pairing_busy` before mutation.

The Exo resume request ID is application authority. It is never a locald
acquisition attempt, lease handle, binding revision, or publisher replay key.
Locald reacquisition can occur underneath a retained browser retry without
replaying the user-visible `workbench launch` action.

### Pairing lifetime and management

A pairing expires after 30 days without successful use and no later than 180
days after enrollment. Successful use advances the idle deadline but never the
absolute deadline. Wall-clock regression cannot extend either bound; if Exo
cannot establish conservative time ordering, it treats the pairing as expired.

The published pairing cookie is named `exo_workbench_pairing` and contains
`v1.<selector>.<secret>`. It is `HttpOnly`, `Secure`, `SameSite=Strict`,
host-only, and scoped to `Path=/`. Its `Max-Age` never exceeds the smaller
remaining idle and absolute lifetime. Pairing responses use `Cache-Control:
no-store`.

The management surfaces are:

```text
exo workbench pairing list
exo workbench pairing revoke <selector>
exo workbench pairing rename <selector> <nickname>
exo workbench pairing forget <selector>
```

The cockpit uses an authenticated `GET /api/pairings` projection plus
`POST /api/pairing/rename` and `POST /api/pairing/revoke` commands under the
`workbench.pairing.manage` capability. `POST /api/pairing/forget` remains a
current-browser action: it discards that browser's pairing cookie and every
active session cookie derived from the pairing without revoking durable
authority or deleting the retained record. Browser projections include only a
path-free workspace label, abbreviated selector, creation time, last-used time,
expiry, optional nickname, active or revoked status, optional revocation time,
and whether the row is the current browser. They omit filesystem paths,
credential digests, locald handles, and secrets.

Revocation marks the pairing record with `revoked_at` and invalidates its
successful resume outcomes and active sessions in one authorization-store
commit. Short-lived terminal request outcomes remain replay-stable but can
never mint authority. The retained record remains available for management and
audit identity. CLI `pairing forget` removes the selected record and every
remaining outcome; when the record is still active, the same atomic commit first
invalidates its sessions and successful outcomes. An expired active record is
pruned.

### Origin and HTTP rules

All browser API requests first validate the canonical Host and Origin before
workspace lookup or cookie parsing. No permissive CORS mode is added. Mutation
routes remain POST operations protected by `SameSite=Strict`, exact Origin, and
the ordinary Exo request envelope where applicable.

Published mode adds `GET /api/health`. It requires the exact canonical Host,
accepts no redirect, reads no browser credential, returns `204 No Content`, and
contains no project state. Locald uses it only to determine endpoint readiness;
health never authenticates a browser or authorizes a command.

Locald terminates TLS and proxies HTTP and WebSocket traffic to the private
loopback listener. It therefore transports opaque Cookie and body bytes that can
contain Exo credentials. Locald is not an authorization recipient: it must not
parse, persist, log, project, inspect, or reuse those credentials beyond opaque
proxy transit. Forwarded public-scheme and authority metadata is advisory; Exo
authenticates with its own canonical-origin mapping.

### Published-service integration

The exact workspace opts in through locald configuration:

```toml
[services.workbench]
type = "published"

[services.workbench.health_check]
type = "http"
path = "/api/health"
```

Exo parses this configuration without registering or mutating the project. A
published launch follows this order:

1. validate the exact Exo workspace and focused lane;
2. bind or retain a non-shareable `127.0.0.1` listener before expensive host
   startup;
3. use locald's authenticated resolution to obtain the exact
   `ProjectInstanceId`;
4. prepare the declared `workbench` publication using that expected identity,
   an absolute project locator as a non-authoritative routing hint, and the
   service name;
5. receive locald's canonical primary origin;
6. atomically install `{workspace_key, ProjectInstanceId, canonical_origin}` in
   Exo's application-authorization map;
7. acknowledge that exact origin and acquire with a duplicate of the retained
   listener capability;
8. start or reuse the private Exo host;
9. wait for the exact lease and binding revision to become healthy and
   routable; and
10. mint the workspace-and-origin-bound enrollment ticket and return the
    canonical URL.

The provider receives no Exo workspace key, state root, lane identity, ticket,
pairing selector, pairing secret, session selector, session secret, or browser
capability set. The authenticated local publication request can include the
absolute project locator solely as a locald resolution hint. Locald resolves it
independently and requires the result to equal the expected project instance.

The supported `locald-publisher-client` owns protocol framing, descriptor
transfer, peer authentication, epoch handling, publication-request replay,
renewal, wake handling, readiness waits, and typed errors. Exo does not
reimplement those concerns.

Exo keeps a publication supervisor per exact workspace and locald project
instance. The supervisor owns only that worktree's lease. Lease renewal and
epoch-driven reacquisition do not count as user activity. A live published
pairing is durable publication intent and keeps the Exo daemon resident until
the pairing is revoked or expires; otherwise the daemon's idle shutdown would
silently withdraw a bookmarkable origin. Each supervisor is released when its
workspace has no live enrollment or pairing authority, independently of other
workspaces. A build without workbench assets does not treat retained pairings
as daemon-residency intent. Releasing one supervisor never releases another
worktree's route or closes a shared listener still in use. Exo reconciles this
authority on a bounded maintenance interval even while unrelated daemon traffic
continues, and serializes release against a fresh launch for the same workspace.

A replacement daemon reconstructs supervisors only for live retained pairings
whose workspace registration still resolves to the same physical worktree. It
requires locald to return the pairing's exact project instance and canonical
origin before it republishes the private listener. Missing, revoked, expired,
relocated, or contradictory pairing state never creates publication authority.
The core daemon socket starts independently of this work. Restoration runs in
the background, isolates failures by workspace, and retries failed acquisition
with bounded backoff while the pairing remains live. One slow or unavailable
route therefore cannot hold the authorization store, delay Exo commands, or
suppress another worktree's route. Shutdown prevents an in-flight restoration
attempt from publishing after provider teardown begins.

When the private listener changes, Exo starts the candidate and rebinds each
worktree supervisor independently. Locald owns the authenticated atomic commit
of each route binding. Exo keeps the old listener alive until every affected
supervisor has either switched or explicitly released its old lease.

### Fallback and failure classification

Direct loopback is an explicit outcome, not a recovery from a broken published
route.

| Condition | Launch behavior |
|---|---|
| No published `workbench` declaration | Visibly labeled direct loopback |
| Authoritative probe proves locald wholly absent | Visibly labeled direct loopback |
| Explicit compatible locald sandbox context | Publish through that sandbox |
| Installation evidence but unsafe record, inactive transport, or unreachable daemon | Actionable setup or start failure |
| Incompatible publisher protocol | Actionable upgrade failure |
| Project instance or declaration mismatch | Actionable configuration failure |
| Origin install, acquire, pause, health, readiness, or routing failure | Stable-origin failure; no fallback |
| Lease loss or locald daemon restart | Fresh acquisition for the same declaration; no fallback |
| Pairing invalid, revoked, or expired | Terminal Exo authentication result and fresh enrollment |

A missing socket alone never proves that locald is absent. Once Exo observes a
valid declaration and installation evidence, it does not silently downgrade to
direct loopback. A paused route remains paused; Exo reports Resume or `locald
up` guidance and never clears the pause.

### Linked worktrees and shared hosts

Each physical worktree has a separate locald project instance, canonical origin,
pairings, and publication supervisor. Two worktrees in the same project can be
open in one browser without replacing each other's cookies or focus.

One private Exo listener may serve several worktree origins. The host therefore
stores an authorization map from canonical origin to exact workspace and locald
project instance rather than one global public origin. Tickets, pairings,
sessions, API requests, and SSE streams validate through that map. A credential
for one origin fails through every sibling origin even when both routes reach
the same listener.

The origin never encodes the focused lane. A lane focus change updates Exo state
served at the origin and leaves publication and pairing unchanged. RFC 10205's
project dashboard can summarize sibling workspaces, but a pairing for one
workspace never grants mutation authority in another.

## Security and Privacy

The stable origin is discoverable and grants no authority. The design protects
against accidental cross-workspace access, stale daemon or worktree identity,
leaked enrollment URLs after their bounded lifetime, foreign-origin browser
requests, DNS rebinding, indefinite reuse after revocation or expiry, and
duplicate sessions after ambiguous resume responses.

Pairing increases the consequence of browser-profile compromise relative to a
12-hour session. Rolling and absolute expiry, exact workspace-instance-origin
binding, fixed capabilities, digest-only persistence, host-only secure cookies,
bounded replay state, and explicit revocation constrain that exposure.

The initial local product does not defend a user from arbitrary code already
executing as the same operating-system user with access to browser profiles, Exo
runtime files, process memory, debugging interfaces, or locald proxy memory.
Likewise, the first publisher protocol may not distinguish cooperating Exo from
another live same-user process that satisfies locald's publication rules. These
are explicit boundaries, not properties inferred from Host, Origin, TLS, or
health checks.

## Compatibility and Migration

RFC 10203 remains the implemented Stage 3 contract until this RFC reaches
Candidate. Existing tickets and renewable sessions continue to work.

On first version-2 load, Exo reads live version-1 session grants and writes them
as direct-loopback-only grants into
`runtime/workbench.authorizations.json`. It infers no pairing,
`ProjectInstanceId`, or canonical published origin. After the new store is
durable, Exo renames the legacy file to `workbench.sessions.v1.json`, choosing a
non-conflicting timestamped archive name when necessary. If archiving fails,
the new store remains authoritative and the legacy file is preserved for
inspection. New binaries ignore that legacy path once the authorization store
exists, while older binaries can continue writing only the isolated legacy
path and therefore cannot destroy version-2 pairings.

New direct-loopback launches continue to use version-1 tickets and the legacy
session exchange. New published launches use version-2 tickets and pairing
enrollment. Deleting the authorization store signs browsers out but changes no
canonical project data. Removing a worktree or observing a project-instance
mismatch invalidates its pairings and sessions. Moving a worktree preserves a
pairing only after an Exo-owned ensure proves the same project instance,
atomically installs the moved workspace root and key, and invalidates derived
sessions and resume outcomes.

The real published path depends on locald delivering both the production
publisher transport and health-gated proxy routing. Until then, Exo can build
and test the pairing store, browser protocol, origin-aware host model, provider
adapter, and rebind orchestration against a fake provider without claiming a
usable stable origin.

## Validation

The Candidate gate requires all of the following:

- enrollment creates one pairing and one session only after durable commit;
- a valid pairing resumes after active-session expiry without a new ticket;
- same-request retries return the same session and cookie;
- different request IDs create distinct sessions;
- revocation, idle expiry, absolute expiry, time regression, and store deletion
  fail closed;
- pairing and outcome limits reject before mutation and do not evict live
  authority silently;
- exact Host, Origin, workspace, project instance, and canonical-origin mismatch
  cases fail before project data is read;
- direct mode never creates a durable pairing;
- version-1 tickets cannot acquire published-origin authority, while version-2
  tickets reject entry-mode, project-instance, and canonical-origin mismatch;
- legacy sessions migrate to the rollback-safe authorization path as
  direct-only grants and older-binary writes cannot replace version-2 pairing
  state;
- the complete opt-in, installation, protocol, pause, health, and fallback table
  is covered;
- locald health probes receive a state-free `204` and never follow redirects;
- one shared listener can safely serve two worktree origins with separate
  cookies, focus, publication supervisors, and request replay;
- an authenticated same-instance worktree move preserves only the pairing,
  updates the retained root and key atomically, and invalidates all derived
  sessions and outcomes; browser traffic alone cannot perform that rebind;
- daemon and locald replacement preserve valid pairings but do not preserve
  stale lease handles;
- partial per-worktree rebind keeps unaffected old bindings available;
- browser projections and diagnostics contain no path, secret, digest, private
  port, or locald capability handle; and
- real-browser desktop and mobile acceptance proves bookmark return, session
  expiry/resume, provider unavailability, and fresh enrollment recovery.

## Drawbacks

- Pairing adds another credential lifecycle beside tickets and sessions.
- Long-lived browser authority increases the value of a compromised profile.
- A stable origin introduces a route-provider dependency for the strongest
  continuity guarantee.
- Locald TLS termination means routing infrastructure transiently transports
  opaque Exo credentials.
- Provider downtime and Exo authentication failure need distinct UI states.
- Published-mode integration depends on locald transport and routing delivery.
- Direct loopback remains intentionally less durable.

## Alternatives

### Keep issuing longer launch tickets

A longer bearer URL gives the user more time to open it but remains awkward to
bookmark or revisit. It lengthens the window in which a leaked unused URL can
enroll a browser and does not provide stable routing.

### Make the active session permanent

A permanent session conflates current tab activity with durable browser trust.
It weakens idle cleanup, complicates per-tab behavior, and makes session
credentials carry more authority than necessary.

### Put workspace identity in a URL path

A path such as `/workspace/<key>` can select a workspace but does not establish
authorization or stable service ownership. It also puts several worktrees under
shared cookie scope. Separate worktree origins provide a clearer browser and
routing boundary.

### Let locald own browser authorization

Locald owns stable service and route authority, not Exo's workspace, lane, or
command semantics. Moving pairing into locald would duplicate Exo authority and
make routing infrastructure an application authorization service.

### Persist pairings for direct numeric loopback origins

A numeric HTTP origin can be rebound by another same-user process after Exo
exits and cannot honestly require a `Secure` cookie. Giving it a 180-day pairing
would make a temporary port look durable while increasing credential exposure.
Direct mode therefore remains session-only.

### Keep one stable origin for the whole project

A project-wide origin would require every request and cookie to select among
worktrees. Worktree-scoped origins match Exo focus isolation, locald physical
project-instance identity, and the expectation that linked worktrees can advance
different lanes.

## Delivery Boundary

Exo work can proceed independently on the version-2 authorization store,
pairing HTTP protocol, management surfaces, exact replay, origin-aware host
model, fake provider adapter, and listener ownership tests.

A real stable-origin launch waits for locald to deliver:

1. the active authenticated Unix publisher transport, strict listener
   capability transfer, production wake monitor, and installation record;
2. health-gated proxy routing, route authorization, truthful publication state,
   traffic cancellation, and effective exact-binding readiness; and
3. the joint Exo adapter and two-worktree acceptance proof.

Exo uses `locald-publisher-client` for publication protocol mechanics. Neither
RFC fixes a source dependency version before the production transport is ready.

## Stage

This RFC is a Stage 2 Draft. It specifies the user experience, authority model,
versioned records, HTTP and cookie protocol, replay and resource bounds,
provider handshake, strict fallback matrix, migration, security boundary, and
Candidate validation bar.

RFC 10203 remains the authority for current behavior. Reaching Stage 3 requires
the Exo pairing and origin model to be implemented, the locald production route
to be available, and the full linked-worktree and real-browser validation matrix
to pass.
