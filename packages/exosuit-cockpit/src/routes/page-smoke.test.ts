import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import snapshotFixture from "$lib/workbench-snapshot.v1.json";
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

beforeEach(() => {
  history.replaceState({}, "", "/");
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
    expect(history.state.exoWorkbenchSessionKey).toBe("session-selector");
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
    expect(history.state.exoWorkbenchSessionKey).toBeUndefined();
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
                  message: "The workbench session limit is busy",
                }),
                { status: 429 },
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
      screen.getByText("The workbench session limit is busy"),
    ).toBeTruthy();
    expect(location.hash).toBe("");

    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(submittedTickets).toEqual(["v1.busy-ticket", "v1.busy-ticket"]);
    expect(history.state.exoWorkbenchSessionKey).toBe("session-selector");
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
    expect(history.state.exoWorkbenchSessionKey).toBe("fresh-session");
    expect(TestEventSource.instances).toHaveLength(1);
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
      expect(history.state.exoWorkbenchSessionKey).toBe("fresh-session");

      if (staleResult === "completion") {
        staleExchange.resolve(sessionResponse("stale-session"));
      } else {
        staleExchange.reject(new TypeError("stale connection reset"));
      }
      await Promise.resolve();
      await Promise.resolve();

      expect(history.state.exoWorkbenchSessionKey).toBe("fresh-session");
      expect(
        screen.getByRole("heading", { name: "Local workbench host" }),
      ).toBeTruthy();
      expect(
        screen.queryByRole("heading", { name: "Launch link required" }),
      ).toBeNull();
      expect(TestEventSource.instances).toHaveLength(1);
    });
  }

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
      screen.getByRole("button", { name: "Focus Retry lane" }),
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
      screen.getByRole("button", { name: "Focus Ambiguous lane" }),
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
});
