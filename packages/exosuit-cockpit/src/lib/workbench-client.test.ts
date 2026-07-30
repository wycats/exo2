import { describe, expect, it, vi } from "vitest";

import snapshotFixture from "./workbench-snapshot.v1.json";
import type { WorkbenchPlanningRequest } from "./workbench";
import {
  createWorkbenchRequestId,
  exchangeWorkbenchTicket,
  launchTicketFromHash,
  prepareWorkbenchTicketExchange,
  retainSessionSelector,
  sessionKeyFromHistory,
  WorkbenchClient,
  WorkbenchClientError,
} from "./workbench-client";

const jsonResponse = (value: unknown, status = 200): Response =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });

describe("workbench browser client", () => {
  it("reads only the launch ticket from the URL fragment", () => {
    expect(launchTicketFromHash("#ticket=v1.signed&other=value")).toBe(
      "v1.signed",
    );
    expect(launchTicketFromHash("#other=value")).toBeNull();
    expect(launchTicketFromHash("")).toBeNull();
  });

  it("exchanges a launch ticket for a typed session", async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse({
        kind: "workbench.session",
        ok: true,
        schema_version: 1,
        session_key: "session-selector",
        project_id: "project-fixture",
        workspace_key: "workspace-fixture",
        expires_at: "2026-07-29T22:00:00Z",
      }),
    );

    const session = await exchangeWorkbenchTicket("v1.ticket", fetcher);

    expect(session.session_key).toBe("session-selector");
    expect(fetcher).toHaveBeenCalledWith(
      "/api/session",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
        body: JSON.stringify({ ticket: "v1.ticket" }),
      }),
    );
  });

  it("renews the existing selector without exposing the cookie secret", async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse({
        kind: "workbench.session",
        ok: true,
        schema_version: 1,
        session_key: "session-selector",
        project_id: "project-fixture",
        workspace_key: "workspace-fixture",
        expires_at: "2026-07-30T22:00:00Z",
      }),
    );

    const session = await new WorkbenchClient(
      "session-selector",
      fetcher,
    ).renewSession();

    expect(session.expires_at).toBe("2026-07-30T22:00:00Z");
    expect(fetcher).toHaveBeenCalledWith(
      "/api/session/renew",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
        body: JSON.stringify({ session_key: "session-selector" }),
      }),
    );
  });

  it("keeps the public session selector in same-entry history state", () => {
    const replaceState = vi.fn();
    const history = {
      state: { unrelated: "kept" },
      replaceState,
    };

    retainSessionSelector(
      history,
      { pathname: "/workbench", search: "?view=current" },
      "session-selector",
    );

    expect(replaceState).toHaveBeenCalledWith(
      {
        unrelated: "kept",
        exoWorkbenchSessionKey: "session-selector",
      },
      "",
      "/workbench?view=current",
    );
    expect(
      sessionKeyFromHistory({
        exoWorkbenchSessionKey: "session-selector",
      }),
    ).toBe("session-selector");
  });

  it("clears the fragment and retained selector before ticket exchange", () => {
    const replaceState = vi.fn();
    prepareWorkbenchTicketExchange(
      {
        state: {
          unrelated: "kept",
          exoWorkbenchSessionKey: "old-session",
        },
        replaceState,
      },
      { pathname: "/workbench", search: "?view=current" },
    );

    expect(replaceState).toHaveBeenCalledWith(
      { unrelated: "kept" },
      "",
      "/workbench?view=current",
    );
  });

  it("requires a fresh link after an ambiguous ticket exchange", async () => {
    const fetcher = vi
      .fn<typeof fetch>()
      .mockRejectedValue(new TypeError("connection reset"));

    await expect(
      exchangeWorkbenchTicket("v1.ticket", fetcher),
    ).rejects.toMatchObject({
      kind: "session_required",
      retryable: false,
      message:
        "The launch link could not be confirmed. Open a fresh Exo workbench link.",
    } satisfies Partial<WorkbenchClientError>);
    expect(fetcher).toHaveBeenCalledOnce();
  });

  it("keeps a ticket retryable after an authoritative busy response", async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(
        {
          kind: "workbench.busy",
          ok: false,
          message: "The workbench session limit is busy",
        },
        429,
      ),
    );

    await expect(
      exchangeWorkbenchTicket("v1.ticket", fetcher),
    ).rejects.toMatchObject({
      kind: "server_busy",
      retryable: true,
      message: "The workbench session limit is busy",
    } satisfies Partial<WorkbenchClientError>);
  });

  it("decodes a snapshot from the browser-safe command envelope", async () => {
    const fetcher = vi.fn<typeof fetch>();
    fetcher.mockImplementation(async (_path, init) => {
      const request = JSON.parse(String(init?.body));
      return jsonResponse({
        protocol_version: 1,
        id: request.id,
        status: "ok",
        result: snapshotFixture,
      });
    });

    const snapshot = await new WorkbenchClient(
      "session-selector",
      fetcher,
    ).snapshot();

    expect(snapshot.focused_lane?.id).toBe("lane-fixture");
    expect(snapshot.revision).toBe(7);
  });

  it("sends a revision-bound planning request and decodes its mutation", async () => {
    const bodies: WorkbenchPlanningRequest[] = [];
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (_path, init) => {
        const request = JSON.parse(
          String(init?.body),
        ) as WorkbenchPlanningRequest;
        bodies.push(request);
        return jsonResponse({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: {
            kind: "workbench.task_mutation",
            ok: true,
            schema_version: 1,
            operation: "task_add",
            task_id: "new-task",
          },
        });
      });
    const request: WorkbenchPlanningRequest = {
      protocol_version: 2,
      id: "01PLANNINGREQUEST0000000000",
      session_key: "session-selector",
      expected_revision: 7,
      expected_phase_id: "phase-fixture",
      operation: {
        kind: "task_add",
        goal_id: "host-goal",
        title: "Add browser planning",
      },
    };

    const result = await new WorkbenchClient(
      "session-selector",
      fetcher,
    ).planning(request);

    expect(result).toEqual({
      kind: "workbench.task_mutation",
      ok: true,
      schema_version: 1,
      operation: "task_add",
      task_id: "new-task",
    });
    expect(bodies).toEqual([request]);
  });

  it("decodes the non-mutating task-completion review", async () => {
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (_path, init) => {
        const request = JSON.parse(String(init?.body));
        return jsonResponse({
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
            proposed_outcome: "Implemented the local host.",
            approval_evidence_present: false,
          },
        });
      });

    const result = await new WorkbenchClient(
      "session-selector",
      fetcher,
    ).planning({
      protocol_version: 2,
      id: "01REVIEWREQUEST00000000000",
      session_key: "session-selector",
      expected_revision: 7,
      expected_phase_id: "phase-fixture",
      operation: {
        kind: "task_complete_review",
        task_id: "implement-host",
        outcome: "Implemented the local host.",
      },
    });

    expect(result.kind).toBe("workbench.task_completion_review");
    expect(result).toMatchObject({
      review_id: "review-selector",
      proposed_outcome: "Implemented the local host.",
    });
  });

  it("preserves the exact planning request for a declared same-ID retry", async () => {
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (_path, init) => {
        const request = JSON.parse(String(init?.body));
        return jsonResponse({
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
        });
      });

    await expect(
      new WorkbenchClient("session-selector", fetcher).planning({
        protocol_version: 2,
        id: "01BUSYREQUEST0000000000000",
        session_key: "session-selector",
        expected_revision: 7,
        expected_phase_id: "phase-fixture",
        operation: { kind: "task_start", task_id: "implement-host" },
      }),
    ).rejects.toMatchObject({
      kind: "server_busy",
      retryable: true,
      retryWithSameRequestId: true,
      detailKind: "workbench.busy",
    } satisfies Partial<WorkbenchClientError>);
  });

  it("does not offer retry for an authoritative stale snapshot", async () => {
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (_path, init) => {
        const request = JSON.parse(String(init?.body));
        return jsonResponse({
          protocol_version: 1,
          id: request.id,
          status: "error",
          error: {
            code: "precondition_failed",
            message: "The workbench snapshot is stale",
            details: {
              kind: "workbench.stale_snapshot",
              retry_with_same_request_id: false,
            },
          },
        });
      });

    await expect(
      new WorkbenchClient("session-selector", fetcher).planning({
        protocol_version: 2,
        id: "01STALEREQUEST000000000000",
        session_key: "session-selector",
        expected_revision: 7,
        expected_phase_id: "phase-fixture",
        operation: { kind: "task_start", task_id: "implement-host" },
      }),
    ).rejects.toMatchObject({
      retryable: false,
      retryWithSameRequestId: false,
      detailKind: "workbench.stale_snapshot",
    } satisfies Partial<WorkbenchClientError>);
  });

  it("preserves the lane-focus request ID across a transport retry", async () => {
    const bodies: unknown[] = [];
    let attempts = 0;
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (_path, init) => {
        const request = JSON.parse(String(init?.body));
        bodies.push(request);
        attempts += 1;
        if (attempts === 1) {
          throw new TypeError("connection reset");
        }
        return jsonResponse({
          protocol_version: 1,
          id: request.id,
          status: "ok",
          result: { lane: { id: "lane-two" } },
        });
      });

    await new WorkbenchClient(
      "session-selector",
      fetcher,
    ).focusLane("lane-two", "01EXACTREQUESTID0000000000");

    expect(bodies).toHaveLength(2);
    expect(bodies).toEqual([
      expect.objectContaining({
        id: "01EXACTREQUESTID0000000000",
        operation: { kind: "lane_focus", lane_id: "lane-two" },
      }),
      expect.objectContaining({
        id: "01EXACTREQUESTID0000000000",
        operation: { kind: "lane_focus", lane_id: "lane-two" },
      }),
    ]);
  });

  it("classifies an expired session without exposing transport details", async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(
      jsonResponse(
        {
          kind: "workbench.session_invalid",
          ok: false,
          message: "The workbench session is invalid",
        },
        401,
      ),
    );

    await expect(
      new WorkbenchClient("expired", fetcher).snapshot(),
    ).rejects.toMatchObject({
      kind: "session_expired",
      message: "The workbench session is invalid",
    } satisfies Partial<WorkbenchClientError>);
  });

  it("marks a received retryable command failure for a new request ID", async () => {
    const fetcher = vi
      .fn<typeof fetch>()
      .mockImplementation(async (_path, init) => {
        const request = JSON.parse(String(init?.body));
        return jsonResponse({
          protocol_version: 1,
          id: request.id,
          status: "error",
          error: {
            code: "precondition_failed",
            message: "The lane phase is not active",
          },
        });
      });

    await expect(
      new WorkbenchClient("session-selector", fetcher).focusLane(
        "lane-two",
        "01FIRSTREQUESTID00000000000",
      ),
    ).rejects.toMatchObject({
      kind: "command_failed",
      retryable: true,
      retryWithSameRequestId: false,
    } satisfies Partial<WorkbenchClientError>);
    expect(fetcher).toHaveBeenCalledOnce();
  });

  it("bounds a command that never receives a response", async () => {
    vi.useFakeTimers();
    try {
      const fetcher = vi.fn<typeof fetch>().mockImplementation(
        async (_path, init) =>
          new Promise<Response>((_resolve, reject) => {
            init?.signal?.addEventListener("abort", () => {
              reject(new DOMException("Aborted", "AbortError"));
            });
          }),
      );
      const result = new WorkbenchClient("stalled", fetcher).snapshot();
      const assertion = expect(result).rejects.toMatchObject({
        kind: "transport_error",
        retryable: true,
      });

      await vi.advanceTimersByTimeAsync(10_000);
      await assertion;
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the timeout active while reading a response body", async () => {
    vi.useFakeTimers();
    try {
      const fetcher = vi
        .fn<typeof fetch>()
        .mockImplementation(async (_path, init) => {
          const signal = init?.signal;
          return {
            ok: true,
            status: 200,
            headers: new Headers({
              "Content-Type": "application/json",
            }),
            json: () =>
              new Promise<unknown>((_resolve, reject) => {
                signal?.addEventListener(
                  "abort",
                  () => reject(new DOMException("Aborted", "AbortError")),
                  { once: true },
                );
              }),
          } as Response;
        });
      const result = new WorkbenchClient("stalled-body", fetcher).snapshot();
      const assertion = expect(result).rejects.toMatchObject({
        kind: "transport_error",
        retryable: true,
      });

      await vi.advanceTimersByTimeAsync(10_000);
      await assertion;
    } finally {
      vi.useRealTimers();
    }
  });

  it("identifies the unreadable response without exposing its body", async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(
      new Response("<html>proxy failure</html>", {
        status: 502,
        headers: { "Content-Type": "text/html; charset=utf-8" },
      }),
    );

    await expect(
      new WorkbenchClient("session-selector", fetcher).snapshot(),
    ).rejects.toMatchObject({
      message:
        "Exo returned an unreadable workbench response (HTTP 502, text/html)",
    } satisfies Partial<WorkbenchClientError>);
  });

  it("generates ULID-shaped browser request IDs", () => {
    expect(createWorkbenchRequestId(0)).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  });
});
