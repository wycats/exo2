import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import snapshotFixture from "$lib/workbench-snapshot.v3.json";
import type { WorkbenchPlanningRequest } from "$lib/workbench";
import { workbenchHistoryState } from "$lib/workbench-client";

vi.mock("$app/navigation", () => {
  const metadata = (state: Record<string, unknown>, historyIndex: number) => ({
    "sveltekit:history": historyIndex,
    "sveltekit:navigation":
      typeof state["sveltekit:navigation"] === "number"
        ? state["sveltekit:navigation"]
        : 0,
    "sveltekit:pageurl": location.href,
  });
  const resolvedUrl = (url: string | URL) =>
    url === "" ? `${location.pathname}${location.search}${location.hash}` : url;

  return {
    pushState: (url: string | URL, state: Record<string, unknown>) => {
      const current =
        typeof history.state === "object" && history.state !== null
          ? (history.state as Record<string, unknown>)
          : {};
      const historyIndex =
        typeof current["sveltekit:history"] === "number"
          ? Number(current["sveltekit:history"]) + 1
          : 1;
      history.pushState(
        {
          ...metadata(current, historyIndex),
          "sveltekit:states": state,
        },
        "",
        resolvedUrl(url),
      );
    },
    replaceState: (url: string | URL, state: Record<string, unknown>) => {
      const current =
        typeof history.state === "object" && history.state !== null
          ? (history.state as Record<string, unknown>)
          : {};
      const historyIndex =
        typeof current["sveltekit:history"] === "number"
          ? Number(current["sveltekit:history"])
          : 0;
      history.replaceState(
        {
          ...metadata(current, historyIndex),
          "sveltekit:states": state,
        },
        "",
        resolvedUrl(url),
      );
    },
  };
});

import Page from "./+page.svelte";

class TestEventSource {
  static instances: TestEventSource[] = [];

  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  private listeners = new Map<string, EventListener[]>();

  constructor(readonly url: string | URL) {
    TestEventSource.instances.push(this);
    queueMicrotask(() => this.onopen?.(new Event("open")));
  }

  addEventListener(type: string, listener: EventListener): void {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  emit(type: string): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(new MessageEvent(type));
    }
  }

  fail(): void {
    this.onerror?.(new Event("error"));
  }

  close(): void {}
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function sessionResponse(sessionKey: string): Response {
  return new Response(
    JSON.stringify({
      kind: "workbench.session",
      ok: true,
      schema_version: 1,
      session_key: sessionKey,
      project_id: "project-fixture",
      workspace_key: "workspace-fixture",
      expires_at: "2026-07-29T22:00:00Z",
    }),
    { status: 200 },
  );
}

function laneInspection(
  snapshot: typeof snapshotFixture,
  laneId: string,
  relationship: "focused_here" | "focusable_here" | "prepared" | "historical" =
    "focusable_here",
) {
  const lane = snapshot.lanes.find((candidate) => candidate.id === laneId);
  if (!lane || !snapshot.phase) {
    throw new Error(`missing lane inspection fixture for ${laneId}`);
  }
  return {
    kind: "workbench.lane_inspection",
    ok: true,
    schema_version: 1,
    observed_at: snapshot.observed_at,
    revision: snapshot.revision,
    project: snapshot.project,
    daemon: snapshot.daemon,
    workspace: snapshot.workspace,
    relationship,
    can_focus_here: relationship === "focusable_here",
    lane: {
      ...lane,
      intent: `Inspect ${lane.title} without changing focus`,
      created_at: "2026-07-28T19:00:00Z",
      updated_at: "2026-08-05T19:30:00Z",
    },
    phase: {
      ...snapshot.phase,
      id: lane.phase_id,
      title: lane.phase_title,
      status: lane.phase_status,
      planning_available: false,
    },
  };
}

beforeEach(() => {
  history.replaceState({}, "", "/");
  sessionStorage.clear();
  TestEventSource.instances = [];
  vi.stubGlobal("EventSource", TestEventSource);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("cockpit page", () => {
  it("renders an explicit session-required state without a launch selector", async () => {
    render(Page);

    expect(
      await screen.findByRole("heading", { name: "Launch link required" }),
    ).toBeTruthy();
  });

  it("exchanges a launch ticket, removes the fragment, and renders the snapshot", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return new Response(
          JSON.stringify({
            kind: "workbench.session",
            ok: true,
            schema_version: 1,
            session_key: "session-selector",
            project_id: "project-fixture",
            workspace_key: "workspace-fixture",
            expires_at: "2026-07-29T22:00:00Z",
          }),
          { status: 200 },
        );
      }
      const request = JSON.parse(String(init?.body));
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: snapshotFixture,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);

    render(Page);

    expect(
      await screen.findByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(location.hash).toBe("");
    expect(workbenchHistoryState(history.state).exoWorkbenchSessionKey).toBe(
      "session-selector",
    );
    await waitFor(() => {
      expect(TestEventSource.instances[0]?.url).toBe(
        "/api/events?session_key=session-selector",
      );
    });
  });

  it("refreshes the complete snapshot after an event invalidation", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    let snapshots = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return new Response(
          JSON.stringify({
            kind: "workbench.session",
            ok: true,
            schema_version: 1,
            session_key: "session-selector",
            project_id: "project-fixture",
            workspace_key: "workspace-fixture",
            expires_at: "2026-07-29T22:00:00Z",
          }),
          { status: 200 },
        );
      }
      snapshots += 1;
      const request = JSON.parse(String(init?.body));
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: { ...snapshotFixture, revision: snapshots },
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });
    await waitFor(() => expect(TestEventSource.instances).toHaveLength(1));

    TestEventSource.instances[0]!.emit("invalidate");

    await waitFor(() => expect(snapshots).toBe(2));
    await waitFor(() => {
      expect(screen.getByText("Revision 2")).toBeTruthy();
    });
  });

  it("backs off a capacity-limited event stream while polling stays available", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    let renewalReads = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        renewalReads += 1;
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: snapshotFixture,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });
    await waitFor(() => expect(TestEventSource.instances).toHaveLength(1));

    const scheduled: Array<{ callback: () => void; delay: number }> = [];
    const timeout = vi
      .spyOn(window, "setTimeout")
      .mockImplementation((handler, delay) => {
        scheduled.push({
          callback: handler as () => void,
          delay: Number(delay),
        });
        return scheduled.length as unknown as ReturnType<
          typeof window.setTimeout
        >;
      });

    TestEventSource.instances[0]!.fail();
    expect(renewalReads).toBe(0);
    expect(TestEventSource.instances).toHaveLength(1);
    expect(scheduled.map((entry) => entry.delay)).toEqual([1_000]);

    scheduled[0]!.callback();
    expect(TestEventSource.instances).toHaveLength(2);
    TestEventSource.instances[1]!.fail();
    expect(renewalReads).toBe(0);
    expect(scheduled.map((entry) => entry.delay)).toEqual([1_000, 2_000]);

    scheduled[1]!.callback();
    expect(TestEventSource.instances).toHaveLength(3);
    TestEventSource.instances[2]!.emit("ready");
    TestEventSource.instances[2]!.fail();
    expect(scheduled.map((entry) => entry.delay)).toEqual([
      1_000,
      2_000,
      1_000,
    ]);
    expect(
      screen.getByRole("button", { name: "Refresh workbench" }),
    ).toHaveProperty("disabled", false);
    timeout.mockRestore();
  });

  it("keeps the last cockpit visible while a replaced session reconnects", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const renewal = deferred<Response>();
    let snapshotReads = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        return renewal.promise;
      }
      snapshotReads += 1;
      const request = JSON.parse(String(init?.body));
      if (snapshotReads === 2) {
        return new Response(
          JSON.stringify({
            kind: "workbench.session_invalid",
            ok: false,
            message: "The workbench session is invalid",
          }),
          { status: 401 },
        );
      }
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: {
            ...snapshotFixture,
            revision: snapshotReads === 1 ? 7 : 8,
          },
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );

    expect(await screen.findByText("Reconnecting to Exo.")).toBeTruthy();
    expect(screen.getByText("Reconnecting", { selector: ".connection" })).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("heading", { name: "Session expired" }),
    ).toBeNull();
    expect(
      (
        screen.getByRole("button", {
          name: "Refresh workbench",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);

    renewal.resolve(sessionResponse("session-selector"));

    await waitFor(() => {
      expect(screen.queryByText("Reconnecting to Exo.")).toBeNull();
      expect(screen.getByText("Revision 8")).toBeTruthy();
    });
  });

  it("restores a popped lane selection after session recovery", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const renewal = deferred<Response>();
    const snapshot = structuredClone(snapshotFixture);
    snapshot.revision = 8;
    snapshot.lanes.push({
      id: "lane-history",
      title: "Recovered history lane",
      state: "prepared",
      phase_id: "phase-history",
      phase_title: "Historical delivery",
      phase_status: "completed",
      focused_here: false,
    });
    let snapshotReads = 0;
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        return renewal.promise;
      }
      snapshotReads += 1;
      const request = JSON.parse(String(init?.body));
      operations.push(request.operation.kind);
      if (snapshotReads === 2) {
        return new Response(
          JSON.stringify({
            kind: "workbench.session_invalid",
            ok: false,
            message: "The workbench session is invalid",
          }),
          { status: 401 },
        );
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id, "historical")
          : { ...snapshot, revision: snapshotReads === 1 ? 7 : 8 };
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );
    await screen.findByText("Reconnecting to Exo.");
    window.dispatchEvent(
      new PopStateEvent("popstate", {
        state: {
          exoWorkbenchSessionKey: "session-selector",
          exoWorkbenchInspectedLaneId: "lane-history",
        },
      }),
    );

    renewal.resolve(sessionResponse("session-selector"));

    expect(
      await screen.findByRole("heading", { name: "Recovered history lane" }),
    ).toBeTruthy();
    expect(operations).toEqual(["snapshot", "snapshot", "snapshot", "lane_inspect"]);
  });

  it("preserves a newly selected lane when its inspection starts session recovery", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const renewal = deferred<Response>();
    const snapshot = structuredClone(snapshotFixture);
    for (const [id, title] of [
      ["lane-a", "Lane A"],
      ["lane-b", "Lane B"],
    ] as const) {
      snapshot.lanes.push({
        id,
        title,
        state: "prepared",
        phase_id: "phase-fixture",
        phase_title: "Workbench foundation",
        phase_status: "in-progress",
        focused_here: false,
      });
    }
    let laneBAttempts = 0;
    let snapshotReads = 0;
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        return renewal.promise;
      }
      const request = JSON.parse(String(init?.body));
      operations.push(
        request.operation.kind === "lane_inspect"
          ? `lane_inspect:${request.operation.lane_id}`
          : request.operation.kind,
      );
      if (
        request.operation.kind === "lane_inspect" &&
        request.operation.lane_id === "lane-b"
      ) {
        laneBAttempts += 1;
        if (laneBAttempts === 1) {
          return new Response(
            JSON.stringify({
              kind: "workbench.session_invalid",
              ok: false,
              message: "The workbench session is invalid",
            }),
            { status: 401 },
          );
        }
      }
      if (request.operation.kind === "snapshot") {
        snapshotReads += 1;
        snapshot.revision = snapshotReads === 1 ? 7 : 8;
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id)
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane A, phase in progress" }),
    );
    await screen.findByRole("heading", { name: "Lane A" });
    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane B, phase in progress" }),
    );
    await screen.findByText("Reconnecting to Exo.");

    renewal.resolve(sessionResponse("session-selector"));

    expect(await screen.findByRole("heading", { name: "Lane B" })).toBeTruthy();
    expect(operations).toEqual([
      "snapshot",
      "lane_inspect:lane-a",
      "lane_inspect:lane-b",
      "snapshot",
      "lane_inspect:lane-b",
    ]);
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-b");
  });

  it("retains pending lane restoration while a queued refresh finishes during recovery", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const blockedRefresh = deferred<Response>();
    const renewal = deferred<Response>();
    const snapshot = structuredClone(snapshotFixture);
    for (const [id, title] of [
      ["lane-a", "Lane A"],
      ["lane-b", "Lane B"],
    ] as const) {
      snapshot.lanes.push({
        id,
        title,
        state: "prepared",
        phase_id: "phase-fixture",
        phase_title: "Workbench foundation",
        phase_status: "in-progress",
        focused_here: false,
      });
    }
    let laneBAttempts = 0;
    let snapshotReads = 0;
    let blockedRefreshRequestId = "";
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        return renewal.promise;
      }
      const request = JSON.parse(String(init?.body));
      operations.push(
        request.operation.kind === "lane_inspect"
          ? `lane_inspect:${request.operation.lane_id}`
          : request.operation.kind,
      );
      if (request.operation.kind === "snapshot") {
        snapshotReads += 1;
        if (snapshotReads === 2) {
          blockedRefreshRequestId = request.id;
          return blockedRefresh.promise;
        }
        snapshot.revision = snapshotReads === 1 ? 7 : 8;
      }
      if (
        request.operation.kind === "lane_inspect" &&
        request.operation.lane_id === "lane-b"
      ) {
        laneBAttempts += 1;
        if (laneBAttempts === 1) {
          return new Response(
            JSON.stringify({
              kind: "workbench.session_invalid",
              ok: false,
              message: "The workbench session is invalid",
            }),
            { status: 401 },
          );
        }
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id)
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane A, phase in progress" }),
    );
    await screen.findByRole("heading", { name: "Lane A" });

    TestEventSource.instances[0]!.emit("invalidate");
    await waitFor(() => expect(snapshotReads).toBe(2));
    TestEventSource.instances[0]!.emit("invalidate");

    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane B, phase in progress" }),
    );
    await screen.findByText("Reconnecting to Exo.");

    blockedRefresh.resolve(
      new Response(
        JSON.stringify({
          protocol_version: 1,
          id: blockedRefreshRequestId,
          status: "ok",
          result: { ...snapshot, revision: 8 },
        }),
        { status: 200 },
      ),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    renewal.resolve(sessionResponse("session-selector"));

    expect(await screen.findByRole("heading", { name: "Lane B" })).toBeTruthy();
    expect(operations).toEqual([
      "snapshot",
      "lane_inspect:lane-a",
      "snapshot",
      "lane_inspect:lane-b",
      "snapshot",
      "lane_inspect:lane-b",
    ]);
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-b");
  });

  it("enters recovery immediately when a live refresh returns an unreadable response", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const renewal = deferred<Response>();
    let snapshotReads = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        return renewal.promise;
      }
      snapshotReads += 1;
      const request = JSON.parse(String(init?.body));
      if (snapshotReads === 2) {
        return new Response("temporary upstream failure", {
          status: 500,
          headers: { "Content-Type": "text/plain" },
        });
      }
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: {
            ...snapshotFixture,
            revision: snapshotReads === 1 ? 7 : 8,
          },
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );

    expect(await screen.findByText("Reconnecting to Exo.")).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(
      (
        screen.getByRole("button", {
          name: "Refresh workbench",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);

    renewal.resolve(sessionResponse("session-selector"));

    await waitFor(() => {
      expect(screen.queryByText("Reconnecting to Exo.")).toBeNull();
      expect(screen.getByText("Revision 8")).toBeTruthy();
    });
  });

  it("retains the cockpit and requests one reload after a host snapshot upgrade", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    let snapshotReads = 0;
    let renewalReads = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        renewalReads += 1;
        return sessionResponse("session-selector");
      }
      snapshotReads += 1;
      const request = JSON.parse(String(init?.body));
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result:
            snapshotReads === 1
              ? snapshotFixture
              : { ...snapshotFixture, schema_version: 4 },
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );

    expect(await screen.findByText("Workbench update available.")).toBeTruthy();
    expect(screen.getByText("Updated", { selector: ".connection" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(screen.queryByText("Reconnecting to Exo.")).toBeNull();
    expect(screen.queryByText("Live refresh paused.")).toBeNull();
    expect(renewalReads).toBe(0);
  });

  it("keeps an in-flight refresh alive while the event stream reconnects", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const pendingRefresh = deferred<Response>();
    let snapshotReads = 0;
    let renewalReads = 0;
    let pendingRefreshRequestId = "";
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        renewalReads += 1;
        return sessionResponse("session-selector");
      }
      snapshotReads += 1;
      const request = JSON.parse(String(init?.body));
      if (snapshotReads === 2) {
        pendingRefreshRequestId = request.id;
        return pendingRefresh.promise;
      }
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: {
            ...snapshotFixture,
            revision: snapshotReads === 1 ? 7 : 8,
          },
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });
    await waitFor(() => expect(TestEventSource.instances).toHaveLength(1));
    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );
    await waitFor(() => expect(snapshotReads).toBe(2));

    TestEventSource.instances[0]!.fail();

    expect(
      await screen.findByText("Polling", { selector: ".connection" }),
    ).toBeTruthy();
    expect(screen.queryByText("Reconnecting to Exo.")).toBeNull();
    expect(renewalReads).toBe(0);
    expect(
      screen.getByRole("button", { name: "Refresh workbench" }),
    ).toHaveProperty("disabled", true);

    pendingRefresh.resolve(
      new Response(
        JSON.stringify({
          protocol_version: 1,
          id: pendingRefreshRequestId,
          status: "ok",
          result: {
            ...snapshotFixture,
            revision: 8,
          },
        }),
        { status: 200 },
      ),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Refresh workbench" }),
      ).toHaveProperty("disabled", false),
    );
    expect(screen.getByText("Revision 8")).toBeTruthy();
    expect(renewalReads).toBe(0);
  });

  it("shows a compact recovery boundary when the replacement cannot resume the snapshot", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    let snapshotReads = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      if (path === "/api/session/renew") {
        return sessionResponse("session-selector");
      }
      snapshotReads += 1;
      const request = JSON.parse(String(init?.body));
      return snapshotReads === 1
        ? new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "ok",
              result: snapshotFixture,
            }),
            { status: 200 },
          )
        : new Response(
            JSON.stringify({
              kind: "workbench.session_invalid",
              ok: false,
              message: "The workbench session is invalid",
            }),
            { status: 401 },
          );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );

    expect(
      await screen.findByText("This session could not be restored."),
    ).toBeTruthy();
    expect(screen.getByText("Paused", { selector: ".connection" })).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("heading", { name: "Session expired" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("renders workspace loss as a terminal workspace state", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path) => {
      if (path === "/api/session") {
        return new Response(
          JSON.stringify({
            kind: "workbench.session",
            ok: true,
            schema_version: 1,
            session_key: "session-selector",
            project_id: "project-fixture",
            workspace_key: "workspace-fixture",
            expires_at: "2026-07-29T22:00:00Z",
          }),
          { status: 200 },
        );
      }
      return new Response(
        JSON.stringify({
          kind: "workbench.workspace_unavailable",
          ok: false,
          message: "The workbench workspace is no longer available",
        }),
        { status: 410 },
      );
    });
    vi.stubGlobal("fetch", fetcher);

    render(Page);

    expect(
      await screen.findByRole("heading", { name: "Workspace unavailable" }),
    ).toBeTruthy();
    expect(
      screen.getByText("This session will not fall back to another worktree."),
    ).toBeTruthy();
  });

  it("requires a fresh launch link after an ambiguous ticket exchange", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    let sessionAttempts = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path) => {
      if (path === "/api/session") {
        sessionAttempts += 1;
        throw new TypeError("connection reset");
      }
      throw new Error(`unexpected request: ${String(path)}`);
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);

    expect(
      await screen.findByRole("heading", { name: "Launch link required" }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "The launch link could not be confirmed. Open a fresh Exo workbench link.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(sessionAttempts).toBe(1);
    expect(location.hash).toBe("");
    expect(
      workbenchHistoryState(history.state).exoWorkbenchSessionKey,
    ).toBeUndefined();
  });

  it("retries the same ticket after an authoritative busy response", async () => {
    history.replaceState({}, "", "/#ticket=v1.busy-ticket");
    const submittedTickets: string[] = [];
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          const request = JSON.parse(String(init?.body));
          submittedTickets.push(request.ticket);
          return submittedTickets.length === 1
            ? new Response(
                JSON.stringify({
                  kind: "workbench.busy",
                  ok: false,
                  message: "The workbench session store is temporarily unavailable",
                }),
                { status: 503 },
              )
            : sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: snapshotFixture,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);

    expect(
      await screen.findByRole("heading", { name: "Workbench is busy" }),
    ).toBeTruthy();
    expect(
      screen.getByText("The workbench session store is temporarily unavailable"),
    ).toBeTruthy();
    expect(location.hash).toBe("");

    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(submittedTickets).toEqual(["v1.busy-ticket", "v1.busy-ticket"]);
    expect(workbenchHistoryState(history.state).exoWorkbenchSessionKey).toBe(
      "session-selector",
    );
  });

  it("does not offer an inert retry for a rejected ticket exchange", async () => {
    history.replaceState({}, "", "/#ticket=v1.rejected-ticket");
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(
        JSON.stringify({
          kind: "workbench.origin_mismatch",
          ok: false,
          message: "The workbench origin did not match",
        }),
        { status: 403 },
      ),
    );
    vi.stubGlobal("fetch", fetcher);
    render(Page);

    expect(
      await screen.findByRole("heading", {
        name: "Workbench request rejected",
      }),
    ).toBeTruthy();
    expect(
      screen.getByText("The workbench origin did not match"),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(fetcher).toHaveBeenCalledOnce();
  });

  it("exchanges a fresh ticket delivered through same-tab fragment navigation", async () => {
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return new Response(
          JSON.stringify({
            kind: "workbench.session",
            ok: true,
            schema_version: 1,
            session_key: "fresh-session",
            project_id: "project-fixture",
            workspace_key: "workspace-fixture",
            expires_at: "2026-07-29T22:00:00Z",
          }),
          { status: 200 },
        );
      }
      const request = JSON.parse(String(init?.body));
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: snapshotFixture,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Launch link required" });

    history.replaceState(history.state, "", "/#ticket=v1.fresh-ticket");
    window.dispatchEvent(new HashChangeEvent("hashchange"));

    expect(
      await screen.findByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(location.hash).toBe("");
    expect(workbenchHistoryState(history.state).exoWorkbenchSessionKey).toBe(
      "fresh-session",
    );
    expect(TestEventSource.instances).toHaveLength(1);
  });

  it("reboots from a session selector restored through browser history", async () => {
    history.replaceState({}, "", "/#ticket=v1.current-ticket");
    const commandSessionKeys: string[] = [];
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("current-session");
        }
        if (path === "/api/session/renew") {
          const request = JSON.parse(String(init?.body));
          return sessionResponse(request.session_key);
        }
        const request = JSON.parse(String(init?.body));
        commandSessionKeys.push(request.session_key);
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: snapshotFixture,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });
    expect(commandSessionKeys).toEqual(["current-session"]);

    window.dispatchEvent(
      new PopStateEvent("popstate", {
        state: { exoWorkbenchSessionKey: "restored-session" },
      }),
    );

    await waitFor(() => {
      expect(commandSessionKeys).toEqual([
        "current-session",
        "restored-session",
      ]);
      expect(TestEventSource.instances.at(-1)?.url).toBe(
        "/api/events?session_key=restored-session",
      );
    });
    expect(
      screen.getByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
  });

  it("restores the session and inspected lane from SvelteKit page state", async () => {
    const snapshot = structuredClone(snapshotFixture);
    snapshot.lanes.push({
      id: "lane-history",
      title: "Completed lane",
      state: "executing",
      phase_id: "phase-history",
      phase_title: "Completed phase",
      phase_status: "completed",
      focused_here: false,
    });
    history.replaceState(
      {
        "sveltekit:history": 3,
        "sveltekit:navigation": 3,
        "sveltekit:states": {
          exoWorkbenchSessionKey: "restored-session",
          exoWorkbenchInspectedLaneId: "lane-history",
        },
      },
      "",
      "/",
    );
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session/renew") {
        return sessionResponse("restored-session");
      }
      const request = JSON.parse(String(init?.body));
      operations.push(request.operation.kind);
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id, "historical")
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);

    render(Page);

    expect(
      await screen.findByRole("heading", { name: "Completed lane" }),
    ).toBeTruthy();
    expect(screen.getByText("Project history")).toBeTruthy();
    expect(operations).toEqual(["snapshot", "lane_inspect"]);
    expect(fetcher).toHaveBeenCalledWith(
      "/api/session/renew",
      expect.objectContaining({
        body: JSON.stringify({ session_key: "restored-session" }),
      }),
    );
  });

  it("restores the saved lane after the initial snapshot retry succeeds", async () => {
    const snapshot = structuredClone(snapshotFixture);
    snapshot.lanes.push({
      id: "lane-history",
      title: "Completed lane",
      state: "executing",
      phase_id: "phase-history",
      phase_title: "Completed phase",
      phase_status: "completed",
      focused_here: false,
    });
    history.replaceState(
      {
        "sveltekit:history": 3,
        "sveltekit:navigation": 3,
        "sveltekit:states": {
          exoWorkbenchSessionKey: "restored-session",
          exoWorkbenchInspectedLaneId: "lane-history",
        },
      },
      "",
      "/",
    );
    let snapshotAttempts = 0;
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session/renew") {
        return sessionResponse("restored-session");
      }
      const request = JSON.parse(String(init?.body));
      operations.push(request.operation.kind);
      if (request.operation.kind === "snapshot") {
        snapshotAttempts += 1;
        if (snapshotAttempts === 1) {
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "error",
              error: {
                code: "internal",
                message: "Snapshot temporarily unavailable",
              },
            }),
            { status: 200 },
          );
        }
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id, "historical")
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);

    render(Page);

    expect(await screen.findByText("Snapshot temporarily unavailable")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("heading", { name: "Completed lane" }),
    ).toBeTruthy();
    expect(operations).toEqual(["snapshot", "snapshot", "lane_inspect"]);
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-history");
  });

  it("restores the current entry from tab-local state after reload", async () => {
    const snapshot = structuredClone(snapshotFixture);
    snapshot.lanes.push({
      id: "lane-history",
      title: "Completed lane",
      state: "executing",
      phase_id: "phase-history",
      phase_title: "Completed phase",
      phase_status: "completed",
      focused_here: false,
    });
    history.replaceState(
      {
        "sveltekit:history": 3,
        "sveltekit:navigation": 3,
        "sveltekit:states": {},
      },
      "",
      "/",
    );
    sessionStorage.setItem(
      "exoWorkbenchResumeState",
      JSON.stringify({
        exoWorkbenchSessionKey: "restored-session",
        exoWorkbenchInspectedLaneId: "lane-history",
      }),
    );
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session/renew") {
        return sessionResponse("restored-session");
      }
      const request = JSON.parse(String(init?.body));
      operations.push(request.operation.kind);
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id, "historical")
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);

    render(Page);

    expect(
      await screen.findByRole("heading", { name: "Completed lane" }),
    ).toBeTruthy();
    expect(screen.getByText("Project history")).toBeTruthy();
    expect(operations).toEqual(["snapshot", "lane_inspect"]);
    expect(workbenchHistoryState(history.state).exoWorkbenchSessionKey).toBe(
      "restored-session",
    );

    await fireEvent.click(
      screen.getByRole("button", { name: "Open project workspace overview" }),
    );
    expect(
      JSON.parse(sessionStorage.getItem("exoWorkbenchResumeState") ?? "null")
        .exoWorkbenchSessionKey,
    ).toBe("restored-session");
  });

  for (const staleResult of ["completion", "failure"] as const) {
    it(`ignores a superseded ticket exchange ${staleResult}`, async () => {
      history.replaceState({}, "", "/#ticket=v1.stale-ticket");
      const staleExchange = deferred<Response>();
      let sessionAttempts = 0;
      const fetcher = vi
        .fn<typeof fetch>()
        .mockImplementation(async (path, init) => {
          if (path === "/api/session") {
            sessionAttempts += 1;
            return sessionAttempts === 1
              ? staleExchange.promise
              : sessionResponse("fresh-session");
          }
          const request = JSON.parse(String(init?.body));
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "ok",
              result: snapshotFixture,
            }),
            { status: 200 },
          );
        });
      vi.stubGlobal("fetch", fetcher);
      render(Page);
      await waitFor(() => expect(sessionAttempts).toBe(1));

      history.replaceState(history.state, "", "/#ticket=v1.fresh-ticket");
      window.dispatchEvent(new HashChangeEvent("hashchange"));

      expect(
        await screen.findByRole("heading", { name: "Local workbench host" }),
      ).toBeTruthy();
      expect(workbenchHistoryState(history.state).exoWorkbenchSessionKey).toBe(
        "fresh-session",
      );

      if (staleResult === "completion") {
        staleExchange.resolve(sessionResponse("stale-session"));
      } else {
        staleExchange.reject(new TypeError("stale connection reset"));
      }
      await Promise.resolve();
      await Promise.resolve();

      expect(workbenchHistoryState(history.state).exoWorkbenchSessionKey).toBe(
        "fresh-session",
      );
      expect(
        screen.getByRole("heading", { name: "Local workbench host" }),
      ).toBeTruthy();
      expect(
        screen.queryByRole("heading", { name: "Launch link required" }),
      ).toBeNull();
      expect(TestEventSource.instances).toHaveLength(1);
    });
  }

  it("navigates into completed lane history without changing focus", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const snapshot = structuredClone(snapshotFixture);
    snapshot.lanes.push({
      id: "lane-history",
      title: "Completed lane",
      state: "executing",
      phase_id: "phase-history",
      phase_title: "Completed phase",
      phase_status: "completed",
      focused_here: false,
    });
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      operations.push(request.operation.kind);
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id, "historical")
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Completed lane, phase completed",
      }),
    );

    expect(
      await screen.findByRole("heading", { name: "Completed lane" }),
    ).toBeTruthy();
    expect(screen.getByText("Project history")).toBeTruthy();
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-history");
    expect(operations).toEqual(["snapshot", "lane_inspect"]);

    history.back();
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "Local workbench host" }),
      ).toBeTruthy();
    });
    expect(operations).toEqual(["snapshot", "lane_inspect"]);

    history.forward();
    expect(
      await screen.findByRole("heading", { name: "Completed lane" }),
    ).toBeTruthy();
    expect(operations).toEqual(["snapshot", "lane_inspect", "lane_inspect"]);
  });

  it("offers a retry when the first lane inspection fails", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const snapshot = structuredClone(snapshotFixture);
    snapshot.lanes.push({
      id: "lane-retry",
      title: "Retry lane",
      state: "prepared",
      phase_id: "phase-fixture",
      phase_title: "Workbench foundation",
      phase_status: "in-progress",
      focused_here: false,
    });
    let attempts = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      if (request.operation.kind === "lane_inspect") {
        attempts += 1;
        if (attempts === 1) {
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "error",
              error: {
                code: "temporarily_unavailable",
                message: "Lane inspection is temporarily unavailable",
              },
            }),
            { status: 200 },
          );
        }
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id)
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Retry lane, phase in progress",
      }),
    );
    expect(
      await screen.findByText("Lane inspection is temporarily unavailable"),
    ).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("heading", { name: "Retry lane" }),
    ).toBeTruthy();
    expect(attempts).toBe(2);
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-retry");
  });

  it("retries the lane that failed rather than the lane already displayed", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const snapshot = structuredClone(snapshotFixture);
    for (const [id, title] of [
      ["lane-a", "Lane A"],
      ["lane-b", "Lane B"],
    ] as const) {
      snapshot.lanes.push({
        id,
        title,
        state: "prepared",
        phase_id: "phase-fixture",
        phase_title: "Workbench foundation",
        phase_status: "in-progress",
        focused_here: false,
      });
    }
    const inspectedLaneIds: string[] = [];
    let laneBAttempts = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      if (request.operation.kind === "lane_inspect") {
        inspectedLaneIds.push(request.operation.lane_id);
        if (request.operation.lane_id === "lane-b") {
          laneBAttempts += 1;
          if (laneBAttempts <= 2) {
            return new Response(
              JSON.stringify({
                protocol_version: 1,
                id: request.id,
                status: "error",
                error: {
                  code: "temporarily_unavailable",
                  message: "Lane B is temporarily unavailable",
                },
              }),
              { status: 200 },
            );
          }
        }
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id)
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane A, phase in progress" }),
    );
    await screen.findByRole("heading", { name: "Lane A" });
    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane B, phase in progress" }),
    );
    await screen.findByText("Lane B is temporarily unavailable");
    snapshot.revision += 1;
    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );
    await waitFor(() => expect(laneBAttempts).toBe(2));
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("heading", { name: "Lane B" }),
    ).toBeTruthy();
    expect(inspectedLaneIds).toEqual(["lane-a", "lane-b", "lane-b", "lane-b"]);
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-b");
  });

  it("keeps an inspected lane selected when another client focuses it", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const initialSnapshot = structuredClone(snapshotFixture);
    const inspectedLane = {
      ...initialSnapshot.lanes[0]!,
      id: "lane-inspected",
      title: "Inspected lane",
      focused_here: false,
    };
    initialSnapshot.lanes.push(inspectedLane);
    const focusedSnapshot = structuredClone(initialSnapshot);
    focusedSnapshot.revision += 1;
    focusedSnapshot.focused_lane = {
      ...focusedSnapshot.focused_lane!,
      id: inspectedLane.id,
      title: inspectedLane.title,
    };
    focusedSnapshot.lanes = focusedSnapshot.lanes.map((lane) => ({
      ...lane,
      focused_here: lane.id === inspectedLane.id,
    }));
    let currentSnapshot = initialSnapshot;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(
              currentSnapshot,
              request.operation.lane_id,
              currentSnapshot.focused_lane?.id === request.operation.lane_id
                ? "focused_here"
                : "focusable_here",
            )
          : currentSnapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Inspected lane, phase in progress",
      }),
    );
    await screen.findByRole("heading", { name: "Inspected lane" });
    currentSnapshot = focusedSnapshot;
    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );

    expect(
      await screen.findByText(
        "This is the current execution stream for this workspace.",
      ),
    ).toBeTruthy();
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-inspected");
  });

  it("returns to the project dashboard when an inspected lane disappears", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const initialSnapshot = structuredClone(snapshotFixture);
    initialSnapshot.lanes.push({
      id: "lane-removed",
      title: "Removed lane",
      state: "executing",
      phase_id: "phase-history",
      phase_title: "Completed phase",
      phase_status: "completed",
      focused_here: false,
    });
    const updatedSnapshot = structuredClone(initialSnapshot);
    updatedSnapshot.revision += 1;
    updatedSnapshot.lanes = updatedSnapshot.lanes.filter(
      (lane) => lane.id !== "lane-removed",
    );
    let currentSnapshot = initialSnapshot;
    let inspectionAttempts = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      if (request.operation.kind === "lane_inspect") {
        inspectionAttempts += 1;
        if (inspectionAttempts > 1) {
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "error",
              error: {
                code: "not_found",
                message: "Workbench lane not found: lane-removed",
                details: {
                  kind: "workbench.lane_not_found",
                  lane_id: "lane-removed",
                },
              },
            }),
            { status: 200 },
          );
        }
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: laneInspection(
              initialSnapshot,
              request.operation.lane_id,
              "historical",
            ),
          }),
          { status: 200 },
        );
      }
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: currentSnapshot,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Removed lane, phase completed",
      }),
    );
    await screen.findByRole("heading", { name: "Removed lane" });
    currentSnapshot = updatedSnapshot;
    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );

    expect(
      await screen.findByRole("heading", { name: "Workspaces" }),
    ).toBeTruthy();
    expect(
      screen.getByText("That lane is no longer part of the current project plan."),
    ).toBeTruthy();
    expect(
      workbenchHistoryState(history.state).exoWorkbenchProjectOverview,
    ).toBe(true);
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBeUndefined();
  });

  it("requires a reload when lane inspection uses a newer schema", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const snapshot = structuredClone(snapshotFixture);
    snapshot.lanes.push({
      id: "lane-new-schema",
      title: "New schema lane",
      state: "prepared",
      phase_id: "phase-fixture",
      phase_title: "Workbench foundation",
      phase_status: "in-progress",
      focused_here: false,
    });
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      const result =
        request.operation.kind === "lane_inspect"
          ? {
              ...laneInspection(snapshot, request.operation.lane_id),
              schema_version: 2,
            }
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect New schema lane, phase in progress",
      }),
    );

    expect(await screen.findByText("Workbench update available.")).toBeTruthy();
    expect(screen.queryByText("Lane view unavailable.")).toBeNull();
    expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
  });

  it("keeps the project workspace overview in browser navigation state", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const operations: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      operations.push(request.operation.kind);
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: structuredClone(snapshotFixture),
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Open project workspace overview" }),
    );

    expect(
      await screen.findByRole("heading", { name: "Workspaces" }),
    ).toBeTruthy();
    expect(
      workbenchHistoryState(history.state).exoWorkbenchProjectOverview,
    ).toBe(true);
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBeUndefined();
    expect(operations).toEqual(["snapshot"]);

    history.back();
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "Local workbench host" }),
      ).toBeTruthy();
    });

    history.forward();
    expect(
      await screen.findByRole("heading", { name: "Workspaces" }),
    ).toBeTruthy();
    expect(operations).toEqual(["snapshot"]);
  });

  it("keeps the newest lane selection when an older inspection arrives late", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const snapshot = structuredClone(snapshotFixture);
    for (const [id, title] of [
      ["lane-slow", "Slow lane"],
      ["lane-fast", "Fast lane"],
    ] as const) {
      snapshot.lanes.push({
        id,
        title,
        state: "prepared",
        phase_id: "phase-fixture",
        phase_title: "Workbench foundation",
        phase_status: "in-progress",
        focused_here: false,
      });
    }
    const slowInspection = deferred<Response>();
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      if (
        request.operation.kind === "lane_inspect" &&
        request.operation.lane_id === "lane-slow"
      ) {
        return slowInspection.promise;
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(snapshot, request.operation.lane_id)
          : snapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Slow lane, phase in progress",
      }),
    );
    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Fast lane, phase in progress",
      }),
    );
    expect(
      await screen.findByRole("heading", { name: "Fast lane" }),
    ).toBeTruthy();

    const slowRequest = JSON.parse(
      String(
        fetcher.mock.calls.find(([, init]) => {
          const request = JSON.parse(String(init?.body));
          return request.operation?.lane_id === "lane-slow";
        })?.[1]?.body,
      ),
    );
    slowInspection.resolve(
      new Response(
        JSON.stringify({
          protocol_version: 1,
          id: slowRequest.id,
          status: "ok",
          result: laneInspection(snapshot, "lane-slow"),
        }),
        { status: 200 },
      ),
    );
    await Promise.resolve();
    await Promise.resolve();

    expect(
      screen.getByRole("heading", { name: "Fast lane" }),
    ).toBeTruthy();
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-fast");
  });

  it("keeps the lane being opened when a newer snapshot arrives", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const initialSnapshot = structuredClone(snapshotFixture);
    for (const [id, title] of [
      ["lane-a", "Lane A"],
      ["lane-b", "Lane B"],
    ] as const) {
      initialSnapshot.lanes.push({
        id,
        title,
        state: "prepared",
        phase_id: "phase-fixture",
        phase_title: "Workbench foundation",
        phase_status: "in-progress",
        focused_here: false,
      });
    }
    const updatedSnapshot = structuredClone(initialSnapshot);
    updatedSnapshot.revision += 1;
    let currentSnapshot = initialSnapshot;
    let laneBAttempts = 0;
    let firstLaneBRequestId = "";
    const firstLaneBInspection = deferred<Response>();
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      if (
        request.operation.kind === "lane_inspect" &&
        request.operation.lane_id === "lane-b"
      ) {
        laneBAttempts += 1;
        if (laneBAttempts === 1) {
          firstLaneBRequestId = request.id;
          return firstLaneBInspection.promise;
        }
      }
      const result =
        request.operation.kind === "lane_inspect"
          ? laneInspection(currentSnapshot, request.operation.lane_id)
          : currentSnapshot;
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane A, phase in progress" }),
    );
    await screen.findByRole("heading", { name: "Lane A" });
    await fireEvent.click(
      screen.getByRole("button", { name: "Inspect Lane B, phase in progress" }),
    );
    await waitFor(() => expect(laneBAttempts).toBe(1));

    currentSnapshot = updatedSnapshot;
    TestEventSource.instances[0]!.emit("invalidate");

    await waitFor(() => expect(laneBAttempts).toBe(2));
    expect(
      await screen.findByRole("heading", { name: "Lane B" }),
    ).toBeTruthy();
    firstLaneBInspection.resolve(
      new Response(
        JSON.stringify({
          protocol_version: 1,
          id: firstLaneBRequestId,
          status: "ok",
          result: laneInspection(initialSnapshot, "lane-b"),
        }),
        { status: 200 },
      ),
    );
    await Promise.resolve();
    await Promise.resolve();

    expect(screen.getByRole("heading", { name: "Lane B" })).toBeTruthy();
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-b");
  });

  it("uses a new request ID for deliberate retry after a command response", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const snapshot = structuredClone(snapshotFixture);
    snapshot.lanes.push({
      id: "lane-retry",
      title: "Retry lane",
      state: "prepared",
      phase_id: "phase-fixture",
      phase_title: "Workbench foundation",
      phase_status: "in-progress",
      focused_here: false,
    });
    const focusRequestIds: string[] = [];
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return new Response(
          JSON.stringify({
            kind: "workbench.session",
            ok: true,
            schema_version: 1,
            session_key: "session-selector",
            project_id: "project-fixture",
            workspace_key: "workspace-fixture",
            expires_at: "2026-07-29T22:00:00Z",
          }),
          { status: 200 },
        );
      }
      const request = JSON.parse(String(init?.body));
      if (request.operation.kind === "lane_inspect") {
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: laneInspection(snapshot, request.operation.lane_id),
          }),
          { status: 200 },
        );
      }
      if (request.operation.kind === "lane_focus") {
        focusRequestIds.push(request.id);
        return new Response(
          JSON.stringify(
            focusRequestIds.length === 1
              ? {
                  protocol_version: 1,
                  id: request.id,
                  status: "error",
                  error: {
                    code: "precondition_failed",
                    message: "The lane phase is temporarily unavailable",
                  },
                }
              : {
                  protocol_version: 1,
                  id: request.id,
                  status: "ok",
                  result: { lane: { id: "lane-retry" } },
                },
          ),
          { status: 200 },
        );
      }
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: snapshot,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Retry lane, phase in progress",
      }),
    );
    await screen.findByRole("heading", { name: "Retry lane" });
    await fireEvent.click(
      screen.getByRole("button", { name: "Focus in this workspace" }),
    );
    await screen.findByText("The lane phase is temporarily unavailable");
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    await waitFor(() => expect(focusRequestIds).toHaveLength(2));
    expect(focusRequestIds[1]).not.toBe(focusRequestIds[0]);
  });

  it("clears an ambiguous focus failure when the snapshot confirms success", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const initialSnapshot = structuredClone(snapshotFixture);
    const ambiguousLane = {
      ...initialSnapshot.lanes[0]!,
      id: "lane-ambiguous",
      title: "Ambiguous lane",
      focused_here: false,
    };
    initialSnapshot.lanes.push(ambiguousLane);
    const focusedSnapshot = {
      ...initialSnapshot,
      focused_lane: {
        ...initialSnapshot.focused_lane!,
        id: ambiguousLane.id,
        title: ambiguousLane.title,
      },
      lanes: initialSnapshot.lanes.map((lane) => ({
        ...lane,
        focused_here: lane.id === ambiguousLane.id,
      })),
    };
    let focusAttempts = 0;
    let snapshotReads = 0;
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        if (request.operation.kind === "lane_inspect") {
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "ok",
              result: laneInspection(
                initialSnapshot,
                request.operation.lane_id,
              ),
            }),
            { status: 200 },
          );
        }
        if (request.operation.kind === "lane_focus") {
          focusAttempts += 1;
          throw new TypeError("response lost");
        }
        snapshotReads += 1;
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: snapshotReads === 1 ? initialSnapshot : focusedSnapshot,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Ambiguous lane, phase in progress",
      }),
    );
    await screen.findByRole("heading", { name: "Ambiguous lane" });
    await fireEvent.click(
      screen.getByRole("button", { name: "Focus in this workspace" }),
    );

    await waitFor(() => expect(focusAttempts).toBe(2));
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "Ambiguous lane" }),
      ).toBeTruthy();
      expect(
        screen.queryByText("The workbench command could not reach Exo"),
      ).toBeNull();
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    });
  });

  it("returns to current work after a confirmed focus survives refresh failure", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const initialSnapshot = structuredClone(snapshotFixture);
    const recoveryLane = {
      ...initialSnapshot.lanes[0]!,
      id: "lane-focus-recovery",
      title: "Focus recovery lane",
      focused_here: false,
    };
    initialSnapshot.lanes.push(recoveryLane);
    const focusedSnapshot = structuredClone(initialSnapshot);
    focusedSnapshot.revision += 1;
    focusedSnapshot.focused_lane = {
      ...focusedSnapshot.focused_lane!,
      id: recoveryLane.id,
      title: recoveryLane.title,
    };
    focusedSnapshot.lanes = focusedSnapshot.lanes.map((lane) => ({
      ...lane,
      focused_here: lane.id === recoveryLane.id,
    }));
    let snapshotReads = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      if (request.operation.kind === "lane_inspect") {
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: laneInspection(initialSnapshot, request.operation.lane_id),
          }),
          { status: 200 },
        );
      }
      if (request.operation.kind === "lane_focus") {
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: { lane: { id: recoveryLane.id } },
          }),
          { status: 200 },
        );
      }
      snapshotReads += 1;
      if (snapshotReads === 2) {
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "error",
            error: {
              code: "internal",
              message: "Focus refresh temporarily unavailable",
            },
          }),
          { status: 200 },
        );
      }
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: snapshotReads === 1 ? initialSnapshot : focusedSnapshot,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Focus recovery lane, phase in progress",
      }),
    );
    await screen.findByRole("heading", { name: "Focus recovery lane" });
    await fireEvent.click(
      screen.getByRole("button", { name: "Focus in this workspace" }),
    );

    expect(
      await screen.findByText("Focus refresh temporarily unavailable"),
    ).toBeTruthy();
    expect(
      workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
    ).toBe("lane-focus-recovery");
    await fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Back to current work" }),
      ).toBeNull();
      expect(
        workbenchHistoryState(history.state).exoWorkbenchInspectedLaneId,
      ).toBeUndefined();
    });
    expect(snapshotReads).toBe(3);
  });

  it("expires a local focus confirmation when a newer snapshot contradicts it", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const initialSnapshot = structuredClone(snapshotFixture);
    const requestedLane = {
      ...initialSnapshot.lanes[0]!,
      id: "lane-local-focus",
      title: "Locally focused lane",
      focused_here: false,
    };
    const remoteLane = {
      ...initialSnapshot.lanes[0]!,
      id: "lane-remote-focus",
      title: "Remotely focused lane",
      focused_here: false,
    };
    initialSnapshot.lanes.push(requestedLane, remoteLane);

    const contradictedSnapshot = structuredClone(initialSnapshot);
    contradictedSnapshot.revision += 1;
    contradictedSnapshot.focused_lane = {
      ...contradictedSnapshot.focused_lane!,
      id: remoteLane.id,
      title: remoteLane.title,
    };
    contradictedSnapshot.lanes = contradictedSnapshot.lanes.map((lane) => ({
      ...lane,
      focused_here: lane.id === remoteLane.id,
    }));

    const laterRequestedSnapshot = structuredClone(contradictedSnapshot);
    laterRequestedSnapshot.revision += 1;
    laterRequestedSnapshot.focused_lane = {
      ...laterRequestedSnapshot.focused_lane!,
      id: requestedLane.id,
      title: requestedLane.title,
    };
    laterRequestedSnapshot.lanes = laterRequestedSnapshot.lanes.map((lane) => ({
      ...lane,
      focused_here: lane.id === requestedLane.id,
    }));

    let currentSnapshot = initialSnapshot;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        return sessionResponse("session-selector");
      }
      const request = JSON.parse(String(init?.body));
      if (request.operation.kind === "lane_inspect") {
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: laneInspection(
              currentSnapshot,
              request.operation.lane_id,
              currentSnapshot.focused_lane?.id === request.operation.lane_id
                ? "focused_here"
                : "focusable_here",
            ),
          }),
          { status: 200 },
        );
      }
      if (request.operation.kind === "lane_focus") {
        currentSnapshot = contradictedSnapshot;
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: { lane: { id: requestedLane.id } },
          }),
          { status: 200 },
        );
      }
      return new Response(
        JSON.stringify({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: currentSnapshot,
        }),
        { status: 200 },
      );
    });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Inspect Locally focused lane, phase in progress",
      }),
    );
    await screen.findByRole("heading", { name: "Locally focused lane" });
    await fireEvent.click(
      screen.getByRole("button", { name: "Focus in this workspace" }),
    );
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "Locally focused lane" }),
      ).toBeTruthy();
    });

    await fireEvent.click(
      screen.getByRole("button", { name: "Open project workspace overview" }),
    );
    expect(await screen.findByRole("heading", { name: "Workspaces" })).toBeTruthy();

    currentSnapshot = laterRequestedSnapshot;
    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Workspaces" })).toBeTruthy();
      expect(
        workbenchHistoryState(history.state).exoWorkbenchProjectOverview,
      ).toBe(true);
    });
  });

  it("reviews and approves an exact task outcome with distinct bound request IDs", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const planningRequests: WorkbenchPlanningRequest[] = [];
    let approved = false;
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        if (request.protocol_version === 2) {
          planningRequests.push(request as WorkbenchPlanningRequest);
          if (request.operation.kind === "task_complete_review") {
            return new Response(
              JSON.stringify({
                protocol_version: 1,
                id: request.id,
                status: "ok",
                result: {
                  kind: "workbench.task_completion_review",
                  ok: true,
                  schema_version: 1,
                  review_id: "review-selector",
                  task_id: "implement-host",
                  readiness_rationale: "All focused checks pass.",
                  proposed_outcome: request.operation.outcome,
                  approval_evidence_present: false,
                },
              }),
              { status: 200 },
            );
          }
          approved = true;
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "ok",
              result: {
                kind: "workbench.task_mutation",
                ok: true,
                schema_version: 1,
                operation: "task_complete_approve",
                task_id: "implement-host",
              },
            }),
            { status: 200 },
          );
        }
        const nextSnapshot = structuredClone(snapshotFixture);
        if (approved) {
          nextSnapshot.revision = 8;
          nextSnapshot.phase.goals[0]!.tasks[0]!.status = "completed";
        }
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: nextSnapshot,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Review completion of Implement host",
      }),
    );
    await fireEvent.input(
      screen.getByLabelText("Proposed completion outcome"),
      { target: { value: "Implemented the exact local host contract." } },
    );
    await fireEvent.click(
      screen.getByRole("button", { name: "Review completion" }),
    );
    expect(
      await screen.findByText("Implemented the exact local host contract."),
    ).toBeTruthy();

    await fireEvent.click(
      screen.getByRole("button", { name: "Approve exact outcome" }),
    );
    await waitFor(() => expect(planningRequests).toHaveLength(2));

    expect(planningRequests[0]).toMatchObject({
      protocol_version: 2,
      session_key: "session-selector",
      expected_daemon_instance_id: "daemon-fixture",
      expected_revision: 7,
      expected_phase_id: "phase-fixture",
      operation: {
        kind: "task_complete_review",
        task_id: "implement-host",
        outcome: "Implemented the exact local host contract.",
      },
    });
    expect(planningRequests[1]).toMatchObject({
      protocol_version: 2,
      session_key: "session-selector",
      expected_daemon_instance_id: "daemon-fixture",
      expected_revision: 7,
      expected_phase_id: "phase-fixture",
      operation: {
        kind: "task_complete_approve",
        review_id: "review-selector",
        task_id: "implement-host",
        outcome: "Implemented the exact local host contract.",
      },
    });
    expect(planningRequests[1]!.id).not.toBe(planningRequests[0]!.id);
    await waitFor(() => {
      expect(screen.getByText("Revision 8")).toBeTruthy();
    });
  });

  it("preserves and rebinds a draft after Exo rejects its opening snapshot", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const planningRequests: WorkbenchPlanningRequest[] = [];
    const staleRefresh = deferred<Response>();
    let snapshotRevision = 7;
    let snapshotReads = 0;
    let staleRefreshRequestId = "";
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        if (request.protocol_version === 2) {
          planningRequests.push(request as WorkbenchPlanningRequest);
          if (planningRequests.length === 1) {
            snapshotRevision = 8;
            return new Response(
              JSON.stringify({
                protocol_version: 1,
                id: request.id,
                status: "error",
                error: {
                  code: "precondition_failed",
                  message: "The displayed plan is stale",
                  details: {
                    kind: "workbench.stale_snapshot",
                    retry_with_same_request_id: false,
                  },
                },
              }),
              { status: 200 },
            );
          }
          snapshotRevision = 9;
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "ok",
              result: {
                kind: "workbench.task_mutation",
                ok: true,
                schema_version: 1,
                operation: "task_update",
                task_id: "implement-host",
              },
            }),
            { status: 200 },
          );
        }
        snapshotReads += 1;
        if (snapshotReads === 2) {
          staleRefreshRequestId = request.id;
          return staleRefresh.promise;
        }
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: {
              ...snapshotFixture,
              revision: snapshotRevision,
            },
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Edit Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Task title"), {
      target: { value: "A preserved task title" },
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Refresh workbench" }),
    );
    await waitFor(() => expect(snapshotReads).toBe(2));
    await fireEvent.click(
      screen.getByRole("button", { name: "Save task title" }),
    );
    await waitFor(() => expect(planningRequests).toHaveLength(1));
    staleRefresh.resolve(
      new Response(
        JSON.stringify({
          protocol_version: 1,
          id: staleRefreshRequestId,
          status: "ok",
          result: {
            ...snapshotFixture,
            revision: 7,
          },
        }),
        { status: 200 },
      ),
    );

    await waitFor(() => {
      expect(screen.getByText("Revision 8")).toBeTruthy();
      expect(
        (screen.getByLabelText("Task title") as HTMLInputElement).value,
      ).toBe("A preserved task title");
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Save task title" }),
    );
    await waitFor(() => expect(planningRequests).toHaveLength(2));

    expect(planningRequests[0]).toMatchObject({
      expected_revision: 7,
      operation: {
        kind: "task_update",
        task_id: "implement-host",
        title: "A preserved task title",
      },
    });
    expect(planningRequests[1]).toMatchObject({
      expected_revision: 8,
      operation: {
        kind: "task_update",
        task_id: "implement-host",
        title: "A preserved task title",
      },
    });
    expect(planningRequests[1]!.id).not.toBe(planningRequests[0]!.id);
    await waitFor(() => {
      expect(screen.queryByLabelText("Task title")).toBeNull();
      expect(screen.getByText("Revision 9")).toBeTruthy();
    });
  });

  it("waits for recovered state before rebinding a stale draft", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const planningRequests: WorkbenchPlanningRequest[] = [];
    let snapshotReads = 0;
    let renewalReads = 0;
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("session-selector");
        }
        if (path === "/api/session/renew") {
          renewalReads += 1;
          return sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        if (request.protocol_version === 2) {
          planningRequests.push(request as WorkbenchPlanningRequest);
          if (planningRequests.length === 1) {
            return new Response(
              JSON.stringify({
                protocol_version: 1,
                id: request.id,
                status: "error",
                error: {
                  code: "precondition_failed",
                  message: "The displayed plan is stale",
                  details: {
                    kind: "workbench.stale_snapshot",
                    retry_with_same_request_id: false,
                  },
                },
              }),
              { status: 200 },
            );
          }
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "ok",
              result: {
                kind: "workbench.task_mutation",
                ok: true,
                schema_version: 1,
                operation: "task_update",
                task_id: "implement-host",
              },
            }),
            { status: 200 },
          );
        }
        snapshotReads += 1;
        if (snapshotReads === 2) {
          return new Response("temporary upstream failure", {
            status: 500,
            headers: { "Content-Type": "text/plain" },
          });
        }
        const nextSnapshot = structuredClone(snapshotFixture);
        nextSnapshot.revision = snapshotReads === 1 ? 7 : 8;
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: nextSnapshot,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Edit Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Task title"), {
      target: { value: "A draft awaiting recovered state" },
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Save task title" }),
    );

    await waitFor(() => {
      expect(renewalReads).toBe(1);
      expect(screen.getByText("Revision 8")).toBeTruthy();
      expect(
        (screen.getByLabelText("Task title") as HTMLInputElement).value,
      ).toBe("A draft awaiting recovered state");
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Save task title" }),
    );
    await waitFor(() => expect(planningRequests).toHaveLength(2));

    expect(planningRequests[0]).toMatchObject({
      expected_revision: 7,
    });
    expect(planningRequests[1]).toMatchObject({
      expected_revision: 8,
      operation: {
        kind: "task_update",
        task_id: "implement-host",
        title: "A draft awaiting recovered state",
      },
    });
  });

  it("accepts an approval's own invalidation while its response is pending", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const approvalResponse = deferred<Response>();
    let approvalCommitted = false;
    let approvalRequestId: string | null = null;
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        if (request.protocol_version === 2) {
          if (request.operation.kind === "task_complete_review") {
            return new Response(
              JSON.stringify({
                protocol_version: 1,
                id: request.id,
                status: "ok",
                result: {
                  kind: "workbench.task_completion_review",
                  ok: true,
                  schema_version: 1,
                  review_id: "review-selector",
                  task_id: "implement-host",
                  readiness_rationale: "All focused checks pass.",
                  proposed_outcome: request.operation.outcome,
                  approval_evidence_present: false,
                },
              }),
              { status: 200 },
            );
          }
          approvalRequestId = request.id;
          return approvalResponse.promise;
        }
        const nextSnapshot = structuredClone(snapshotFixture);
        if (approvalCommitted) {
          nextSnapshot.revision = 8;
          nextSnapshot.phase.goals[0]!.tasks[0]!.status = "completed";
        }
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: nextSnapshot,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });
    await waitFor(() => expect(TestEventSource.instances).toHaveLength(1));

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Review completion of Implement host",
      }),
    );
    await fireEvent.input(
      screen.getByLabelText("Proposed completion outcome"),
      { target: { value: "Implemented the exact local host contract." } },
    );
    await fireEvent.click(
      screen.getByRole("button", { name: "Review completion" }),
    );
    await screen.findByText("Implemented the exact local host contract.");
    await fireEvent.click(
      screen.getByRole("button", { name: "Approve exact outcome" }),
    );

    approvalCommitted = true;
    TestEventSource.instances[0]!.emit("invalidate");
    await waitFor(() => {
      expect(screen.getByText("Revision 8")).toBeTruthy();
    });
    expect(screen.queryByText("Planning change not applied.")).toBeNull();
    expect(
      screen.queryByText(
        "The plan changed. Review task completion again from the current plan.",
      ),
    ).toBeNull();

    approvalResponse.resolve(
      new Response(
        JSON.stringify({
          protocol_version: 1,
          id: approvalRequestId,
          status: "ok",
          result: {
            kind: "workbench.task_mutation",
            ok: true,
            schema_version: 1,
            operation: "task_complete_approve",
            task_id: "implement-host",
          },
        }),
        { status: 200 },
      ),
    );
    await waitFor(() => {
      expect(
        screen.queryByText("Implemented the exact local host contract."),
      ).toBeNull();
      expect(screen.queryByText("Planning change not applied.")).toBeNull();
    });
  });

  it("keeps an ambiguous approval retry through its own invalidation", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const planningRequests: WorkbenchPlanningRequest[] = [];
    let approvalCommitted = false;
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        if (request.protocol_version === 2) {
          planningRequests.push(request as WorkbenchPlanningRequest);
          if (request.operation.kind === "task_complete_review") {
            return new Response(
              JSON.stringify({
                protocol_version: 1,
                id: request.id,
                status: "ok",
                result: {
                  kind: "workbench.task_completion_review",
                  ok: true,
                  schema_version: 1,
                  review_id: "review-selector",
                  task_id: "implement-host",
                  readiness_rationale: "All focused checks pass.",
                  proposed_outcome: request.operation.outcome,
                  approval_evidence_present: false,
                },
              }),
              { status: 200 },
            );
          }
          approvalCommitted = true;
          return planningRequests.filter(
            (candidate) =>
              candidate.operation.kind === "task_complete_approve",
          ).length === 1
            ? new Response(
                JSON.stringify({
                  protocol_version: 1,
                  id: request.id,
                  status: "error",
                  error: {
                    code: "precondition_failed",
                    message: "The approval outcome is not known yet",
                    details: {
                      kind: "workbench.busy",
                      retry_with_same_request_id: true,
                    },
                  },
                }),
                { status: 200 },
              )
            : new Response(
                JSON.stringify({
                  protocol_version: 1,
                  id: request.id,
                  status: "ok",
                  result: {
                    kind: "workbench.task_mutation",
                    ok: true,
                    schema_version: 1,
                    operation: "task_complete_approve",
                    task_id: "implement-host",
                  },
                }),
                { status: 200 },
              );
        }
        const nextSnapshot = structuredClone(snapshotFixture);
        if (approvalCommitted) {
          nextSnapshot.revision = 8;
          nextSnapshot.phase.goals[0]!.tasks[0]!.status = "completed";
        }
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: nextSnapshot,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });
    await waitFor(() => expect(TestEventSource.instances).toHaveLength(1));

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Review completion of Implement host",
      }),
    );
    await fireEvent.input(
      screen.getByLabelText("Proposed completion outcome"),
      { target: { value: "Implemented the exact local host contract." } },
    );
    await fireEvent.click(
      screen.getByRole("button", { name: "Review completion" }),
    );
    await screen.findByText("Implemented the exact local host contract.");
    await fireEvent.click(
      screen.getByRole("button", { name: "Approve exact outcome" }),
    );

    expect(
      await screen.findByText("The approval outcome is not known yet"),
    ).toBeTruthy();
    const firstApproval = planningRequests.at(-1)!;
    TestEventSource.instances[0]!.emit("invalidate");
    await waitFor(() => {
      expect(screen.getByText("Revision 8")).toBeTruthy();
      expect(
        screen.getByRole("button", { name: "Retry same request" }),
      ).toBeTruthy();
    });

    await fireEvent.click(
      screen.getByRole("button", { name: "Retry same request" }),
    );
    await waitFor(() => expect(planningRequests).toHaveLength(3));
    expect(planningRequests[2]).toEqual(firstApproval);
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Retry same request" }),
      ).toBeNull();
    });
  });

  it("reports task activation as Exo state rather than agent dispatch", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const pendingSnapshot = structuredClone(snapshotFixture);
    pendingSnapshot.phase.goals[0]!.tasks.push({
      id: "agent-handoff",
      title: "Prepare the agent handoff",
      status: "pending",
      progress: [],
    });
    const activeSnapshot = structuredClone(pendingSnapshot);
    activeSnapshot.revision = pendingSnapshot.revision + 1;
    activeSnapshot.phase.goals[0]!.tasks.at(-1)!.status = "in-progress";
    const planningRequests: WorkbenchPlanningRequest[] = [];
    let taskStarted = false;
    let sessionExchanges = 0;
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          sessionExchanges += 1;
          return sessionResponse(`session-selector-${sessionExchanges}`);
        }
        const request = JSON.parse(String(init?.body));
        if (request.protocol_version === 2) {
          planningRequests.push(request as WorkbenchPlanningRequest);
          taskStarted = true;
          return new Response(
            JSON.stringify({
              protocol_version: 1,
              id: request.id,
              status: "ok",
              result: {
                kind: "workbench.task_mutation",
                ok: true,
                schema_version: 1,
                operation: "task_start",
                task_id: "agent-handoff",
              },
            }),
            { status: 200 },
          );
        }
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: taskStarted ? activeSnapshot : pendingSnapshot,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", {
        name: "Mark Prepare the agent handoff active in Exo",
      }),
    );

    await waitFor(() => expect(planningRequests).toHaveLength(1));
    expect(planningRequests[0]).toMatchObject({
      operation: {
        kind: "task_start",
        task_id: "agent-handoff",
      },
    });
    expect(await screen.findByText("Ready for agent handoff.")).toBeTruthy();
    expect(
      screen.getByText(
        "Exo marked the task active; the workbench did not start an agent.",
      ),
    ).toBeTruthy();

    history.replaceState(history.state, "", "/#ticket=v1.fresh-ticket");
    window.dispatchEvent(new Event("hashchange"));

    await waitFor(() => {
      expect(sessionExchanges).toBe(2);
      expect(screen.queryByText("Ready for agent handoff.")).toBeNull();
      expect(
        screen.queryByText(
          "Exo marked the task active; the workbench did not start an agent.",
        ),
      ).toBeNull();
    });
  });

  it("retries an ambiguous planning write with the exact prepared request", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    const snapshot = structuredClone(snapshotFixture);
    const planningRequests: WorkbenchPlanningRequest[] = [];
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (path, init) => {
        if (path === "/api/session") {
          return sessionResponse("session-selector");
        }
        const request = JSON.parse(String(init?.body));
        if (request.protocol_version === 2) {
          planningRequests.push(request as WorkbenchPlanningRequest);
          return new Response(
            JSON.stringify(
              planningRequests.length === 1
                ? {
                    protocol_version: 1,
                    id: request.id,
                    status: "error",
                    error: {
                      code: "precondition_failed",
                      message: "The workbench planning service is busy",
                      details: {
                        kind: "workbench.busy",
                        retry_with_same_request_id: true,
                      },
                    },
                  }
                : {
                    protocol_version: 1,
                    id: request.id,
                    status: "ok",
                    result: {
                      kind: "workbench.task_mutation",
                      ok: true,
                      schema_version: 1,
                      operation: "task_log",
                      task_id: "implement-host",
                    },
                  },
            ),
            { status: 200 },
          );
        }
        return new Response(
          JSON.stringify({
            protocol_version: 1,
            id: request.id,
            status: "ok",
            result: snapshot,
          }),
          { status: 200 },
        );
      });
    vi.stubGlobal("fetch", fetcher);
    render(Page);
    await screen.findByRole("heading", { name: "Local workbench host" });

    await fireEvent.click(
      screen.getByRole("button", { name: "Record progress for Implement host" }),
    );
    await fireEvent.input(screen.getByLabelText("Progress update"), {
      target: { value: "Recorded exact browser evidence." },
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Record progress" }),
    );
    expect(
      await screen.findByText("The workbench planning service is busy"),
    ).toBeTruthy();
    const editButton = screen.getByRole("button", {
      name: "Edit Implement host",
    }) as HTMLButtonElement;
    expect(editButton.disabled).toBe(true);
    await fireEvent.click(editButton);
    expect(planningRequests).toHaveLength(1);
    await fireEvent.click(
      screen.getByRole("button", { name: "Retry same request" }),
    );

    await waitFor(() => expect(planningRequests).toHaveLength(2));
    expect(planningRequests[1]).toEqual(planningRequests[0]);
    await waitFor(() => {
      expect(screen.queryByLabelText("Progress update")).toBeNull();
    });
  });
});
