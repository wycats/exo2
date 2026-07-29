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
});
