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

  it("retries the one-time session bootstrap after a transport failure", async () => {
    history.replaceState({}, "", "/#ticket=v1.launch-ticket");
    let sessionAttempts = 0;
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (path, init) => {
      if (path === "/api/session") {
        sessionAttempts += 1;
        if (sessionAttempts === 1) {
          throw new TypeError("connection reset");
        }
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
    await screen.findByRole("heading", { name: "Exo is not responding" });

    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(
      await screen.findByRole("heading", { name: "Local workbench host" }),
    ).toBeTruthy();
    expect(sessionAttempts).toBe(2);
    expect(location.hash).toBe("");
    expect(TestEventSource.instances).toHaveLength(1);
  });
});
