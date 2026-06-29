import { beforeEach, describe, expect, it, vi } from "vitest";

const machineChannelMock = vi.hoisted(() => vi.fn());

vi.mock("../agent/lmtool/machineChannel", () => ({
  exoMachineChannel: machineChannelMock,
}));

import { TraceCache } from "./TraceCache";

function okResponse(result: unknown = { ok: true }, trace: unknown = { t: 1 }) {
  return {
    protocol_version: 1,
    id: "test.response",
    status: "ok" as const,
    result,
    trace,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("TraceCache", () => {
  beforeEach(() => {
    machineChannelMock.mockReset();
  });

  it("eagerly fetches registered roots so sidebars populate without a write", async () => {
    machineChannelMock.mockResolvedValueOnce(
      okResponse({ phase: "active" }, { trace: "phase" }),
    );

    const changed = vi.fn();

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.onDidChange(changed);
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(machineChannelMock).toHaveBeenCalledTimes(1);
    expect(changed).toHaveBeenCalledWith("phase-details");

    await expect(cache.get("phase-details")).resolves.toEqual({
      data: { phase: "active" },
      trace: { trace: "phase" },
    });
    expect(machineChannelMock).toHaveBeenCalledTimes(1);
  });

  it("encodes root operations without an empty namespace path segment", async () => {
    machineChannelMock.mockResolvedValueOnce(
      okResponse({ progress_mode: "between-phases" }, { trace: "status" }),
    );

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("status", {
      namespace: "",
      operation: "status",
    });

    await expect(cache.get("status")).resolves.toMatchObject({
      data: { progress_mode: "between-phases" },
    });
    expect(machineChannelMock.mock.calls[0]?.[1].op.params.address).toEqual({
      kind: "operation",
      path: ["status"],
    });
  });

  it("runs a follow-up fetch when root input changes during eager fetch", async () => {
    const stalePending = deferred<ReturnType<typeof okResponse>>();
    machineChannelMock
      .mockReturnValueOnce(stalePending.promise)
      .mockResolvedValueOnce(okResponse({ phase: "new" }, { trace: "new" }));

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
      input: { phase_id: "old" },
    });

    await new Promise((resolve) => setTimeout(resolve, 0));
    cache.updateRootInput("phase-details", { phase_id: "new" });
    stalePending.resolve(okResponse({ phase: "old" }, { trace: "old" }));

    await vi.waitFor(() => {
      expect(machineChannelMock).toHaveBeenCalledTimes(2);
    });

    await expect(cache.get("phase-details")).resolves.toMatchObject({
      data: { phase: "new" },
      trace: { trace: "new" },
    });
  });

  it("joins concurrent gets for the same root", async () => {
    const pending = deferred<ReturnType<typeof okResponse>>();
    machineChannelMock.mockReturnValueOnce(pending.promise);

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    const first = cache.get("phase-details");
    const second = cache.get("phase-details");

    expect(machineChannelMock).toHaveBeenCalledTimes(1);

    pending.resolve(okResponse({ phase: "active" }, { trace: "phase" }));

    await expect(first).resolves.toEqual({
      data: { phase: "active" },
      trace: { trace: "phase" },
    });
    await expect(second).resolves.toEqual({
      data: { phase: "active" },
      trace: { trace: "phase" },
    });
  });

  it("clears in-flight fetches after failure so later gets retry", async () => {
    machineChannelMock
      .mockRejectedValueOnce(new Error("daemon unavailable"))
      .mockResolvedValueOnce(okResponse({ phase: "active" }));

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    await expect(cache.get("phase-details")).resolves.toBeNull();
    await expect(cache.get("phase-details")).resolves.toMatchObject({
      data: { phase: "active" },
    });

    expect(machineChannelMock).toHaveBeenCalledTimes(2);
  });

  it("records empty diagnostics with the explicit input used for fetch", async () => {
    machineChannelMock.mockResolvedValueOnce(
      okResponse(null, { trace: "empty" }),
    );

    const diagnosticsChanged = vi.fn();

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.onDidDiagnosticChange(diagnosticsChanged);
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
      input: { id: "stale-phase" },
    });

    await expect(cache.get("phase-details")).resolves.toBeNull();

    expect(cache.getDiagnostic("phase-details")).toMatchObject({
      rootId: "phase-details",
      namespace: "phase",
      operation: "read-details",
      status: "empty",
      input: { id: "stale-phase" },
      explicitInput: true,
      durationMs: expect.any(Number),
    });
    expect(diagnosticsChanged).toHaveBeenCalledWith("phase-details");
    expect(machineChannelMock.mock.calls[0]?.[1].op.params.input).toEqual({
      id: "stale-phase",
    });
  });

  it("records error diagnostics and clears stale cached data", async () => {
    machineChannelMock
      .mockResolvedValueOnce(
        okResponse({ phase: "active" }, { trace: "active" }),
      )
      .mockResolvedValueOnce({
        protocol_version: 1,
        id: "test.response",
        status: "error" as const,
        error: { code: "daemon-failed", message: "daemon failed" },
      });

    const changed = vi.fn();

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.onDidChange(changed);
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    await expect(cache.get("phase-details")).resolves.toMatchObject({
      data: { phase: "active" },
    });

    cache.updateRootInput("phase-details", { id: "broken-phase" });
    await expect(cache.get("phase-details")).resolves.toBeNull();

    expect(cache.getDiagnostic("phase-details")).toMatchObject({
      status: "error",
      input: { id: "broken-phase" },
      explicitInput: true,
      durationMs: expect.any(Number),
      error: { code: "daemon-failed", message: "daemon failed" },
    });
    await expect(cache.get("phase-details")).resolves.toBeNull();
    expect(machineChannelMock).toHaveBeenCalledTimes(3);
    expect(changed).toHaveBeenCalledWith("phase-details");
  });

  it("records success diagnostics with default input", async () => {
    machineChannelMock.mockResolvedValueOnce(
      okResponse({ phase: "active" }, { trace: "phase" }),
    );

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    await expect(cache.get("phase-details")).resolves.toMatchObject({
      data: { phase: "active" },
    });

    expect(cache.getDiagnostic("phase-details")).toMatchObject({
      status: "success",
      input: {},
      explicitInput: false,
      durationMs: expect.any(Number),
    });
  });

  it("does not cache stale results when root input changes during a fetch", async () => {
    const firstPending = deferred<ReturnType<typeof okResponse>>();
    machineChannelMock
      .mockReturnValueOnce(firstPending.promise)
      .mockResolvedValueOnce(okResponse({ phase: "new" }, { trace: "new" }));

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
      input: { phase_id: "old" },
    });

    const stale = cache.get("phase-details");
    cache.updateRootInput("phase-details", { phase_id: "new" });

    firstPending.resolve(okResponse({ phase: "old" }, { trace: "old" }));
    await expect(stale).resolves.toBeNull();

    const fresh = await cache.get("phase-details");
    expect(fresh).toMatchObject({ data: { phase: "new" } });
    expect(machineChannelMock).toHaveBeenCalledTimes(2);
  });

  it("does not join a stale in-flight fetch after write invalidation", async () => {
    const stalePending = deferred<ReturnType<typeof okResponse>>();
    machineChannelMock
      .mockReturnValueOnce(stalePending.promise)
      .mockResolvedValueOnce(
        okResponse({ phase: "fresh" }, { trace: "fresh" }),
      );

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    const stale = cache.get("phase-details");
    cache.notifyWrite();
    await new Promise((resolve) => setTimeout(resolve, 0));

    stalePending.resolve(okResponse({ phase: "stale" }, { trace: "stale" }));
    await expect(stale).resolves.toBeNull();

    const fresh = await cache.get("phase-details");
    expect(fresh).toMatchObject({ data: { phase: "fresh" } });
    expect(machineChannelMock).toHaveBeenCalledTimes(2);
  });

  it("revalidates cached roots and refetches only invalid traces", async () => {
    machineChannelMock
      .mockResolvedValueOnce(okResponse({ phase: "stale" }, { trace: "old" }))
      .mockResolvedValueOnce({
        protocol_version: 1,
        id: "test.validate",
        status: "ok" as const,
        result: { valid: false },
      })
      .mockResolvedValueOnce(okResponse({ phase: "fresh" }, { trace: "new" }))
      .mockResolvedValueOnce({
        protocol_version: 1,
        id: "test.validate",
        status: "ok" as const,
        result: { valid: true },
      });

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    await expect(cache.get("phase-details")).resolves.toMatchObject({
      data: { phase: "stale" },
    });

    cache.revalidateAll();

    await vi.waitFor(async () => {
      await expect(cache.get("phase-details")).resolves.toMatchObject({
        data: { phase: "fresh" },
      });
    });

    expect(machineChannelMock.mock.calls[1]?.[1].op.params.address).toEqual({
      kind: "operation",
      path: ["context", "validate-trace"],
    });
    expect(machineChannelMock.mock.calls[1]?.[1].op.params.input).toEqual({
      trace_json: JSON.stringify({ trace: "old" }),
    });

    cache.revalidateAll();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(machineChannelMock).toHaveBeenCalledTimes(4);
    await expect(cache.get("phase-details")).resolves.toMatchObject({
      data: { phase: "fresh" },
    });
    expect(machineChannelMock).toHaveBeenCalledTimes(4);
  });

  it("write notifications validate cached roots and refetch invalid traces", async () => {
    machineChannelMock
      .mockResolvedValueOnce(okResponse({ phase: "stale" }, { trace: "old" }))
      .mockResolvedValueOnce({
        protocol_version: 1,
        id: "test.validate",
        status: "ok" as const,
        result: { valid: false },
      })
      .mockResolvedValueOnce(okResponse({ phase: "fresh" }, { trace: "new" }));

    const changed = vi.fn();
    const wrote = vi.fn();

    const cache = new TraceCache();
    cache.setWorkspaceRoot("/workspace");
    cache.onDidChange(changed);
    cache.onDidWrite(wrote);
    cache.registerRoot("phase-details", {
      namespace: "phase",
      operation: "read-details",
    });

    await expect(cache.get("phase-details")).resolves.toMatchObject({
      data: { phase: "stale" },
    });

    cache.notifyWrite();

    await vi.waitFor(async () => {
      await expect(cache.get("phase-details")).resolves.toMatchObject({
        data: { phase: "fresh" },
      });
    });

    expect(wrote).toHaveBeenCalledTimes(1);
    expect(machineChannelMock.mock.calls[1]?.[1].op.params.address).toEqual({
      kind: "operation",
      path: ["context", "validate-trace"],
    });
    expect(machineChannelMock.mock.calls[1]?.[1].op.params.input).toEqual({
      trace_json: JSON.stringify({ trace: "old" }),
    });
    expect(changed).toHaveBeenCalledWith("phase-details");
    expect(machineChannelMock).toHaveBeenCalledTimes(3);
  });
});
