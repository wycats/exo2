import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./socket-client", () => ({
  DEFAULT_DAEMON_REQUEST_TIMEOUT_MS: 30_000,
  ensureDaemon: vi.fn(),
  ensureDaemonLifecycle: vi.fn(async () => ({
    runtimeDir: "/tmp/exo-runtime",
    socketPath: "/tmp/exo-runtime/daemon.sock",
    pidPath: "/tmp/exo-runtime/daemon.pid",
    reused: true,
    spawned: false,
    state: "connected_existing",
  })),
  restartDaemonLifecycle: vi.fn(async () => ({
    runtimeDir: "/tmp/exo-runtime",
    socketPath: "/tmp/exo-runtime/daemon.sock",
    pidPath: "/tmp/exo-runtime/daemon.pid",
    reused: false,
    spawned: true,
    state: "spawned",
  })),
  DaemonConnection: class {},
}));

import {
  DaemonChannelServer,
  type ConnectionLike,
} from "./DaemonChannelServer";
import type {
  MachineChannelRequestEnvelope,
  MachineChannelResponseEnvelope,
} from "../types/machineChannel";

function createFakeClock(start = 1_000) {
  let now = start;
  const timers = new Map<ReturnType<typeof setTimeout>, () => void>();

  return {
    clock: {
      now: () => now,
      setTimeout(callback: () => void, _ms: number) {
        const handle = Symbol("timer") as unknown as ReturnType<
          typeof setTimeout
        >;
        timers.set(handle, callback);
        return handle;
      },
      clearTimeout(handle: ReturnType<typeof setTimeout>) {
        timers.delete(handle);
      },
    },
    advance(ms: number) {
      now += ms;
    },
    runNextTimer() {
      const [handle, callback] = timers.entries().next().value ?? [];
      if (!handle || !callback) {
        return false;
      }
      timers.delete(handle);
      callback();
      return true;
    },
    timerCount() {
      return timers.size;
    },
  };
}

function makeRequestEnvelope(): MachineChannelRequestEnvelope {
  return {
    protocol_version: 1,
    id: "test.request",
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path: ["context", "snapshot"] },
        input: {},
      },
    },
  };
}

function makePlanReadEnvelope(): MachineChannelRequestEnvelope {
  return {
    protocol_version: 1,
    id: "test.plan-read",
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path: ["plan", "read"] },
        input: {},
      },
    },
  };
}

function makeTaskCompleteEnvelope(): MachineChannelRequestEnvelope {
  return {
    protocol_version: 1,
    id: "test.task-complete",
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path: ["task", "complete"] },
        input: { id: "example" },
      },
    },
  };
}

function makeSidecarRepoStatusEnvelope(): MachineChannelRequestEnvelope {
  return {
    protocol_version: 1,
    id: "test.sidecar-repo-status",
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path: ["sidecar", "repo"] },
        input: { action: "status" },
      },
    },
  };
}

function makePhaseExecutionTasksEnvelope(): MachineChannelRequestEnvelope {
  return {
    protocol_version: 1,
    id: "test.phase-execution-tasks",
    op: {
      kind: "call",
      params: {
        address: { kind: "operation", path: ["phase", "execution", "tasks"] },
        input: {},
      },
    },
  };
}

function makeResponse(
  effect?: MachineChannelResponseEnvelope["effect"],
): MachineChannelResponseEnvelope {
  return {
    protocol_version: 1,
    id: "req.1",
    status: "ok",
    result: {},
    effect,
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

function createConnection(
  response: MachineChannelResponseEnvelope = makeResponse(),
): ConnectionLike & {
  request: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
  setClosed(closed: boolean): void;
} {
  let closed = false;

  return {
    request: vi.fn(async () => response),
    notify: vi.fn(),
    close: vi.fn(() => {
      closed = true;
    }),
    isClosed: () => closed,
    setClosed(value: boolean) {
      closed = value;
    },
    onNotification: null,
    onClose: null,
  };
}

describe("DaemonChannelServer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("does not notify writes on first connect", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection();
    const connect = vi.fn(async () => connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-1", {
      connect,
      traceCache,
    });

    const response = await server.request(makeRequestEnvelope());

    expect(response.status).toBe("ok");
    expect(connect).toHaveBeenCalledTimes(1);
    expect(connection.request).toHaveBeenCalledTimes(1);
    expect(traceCache.notifyWrite).not.toHaveBeenCalled();
  });

  it("routes concurrent pure reads over bounded read lanes", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstPending = deferred<MachineChannelResponseEnvelope>();
    const secondPending = deferred<MachineChannelResponseEnvelope>();
    const firstConnection = createConnection();
    const secondConnection = createConnection();
    firstConnection.request.mockReturnValueOnce(firstPending.promise);
    secondConnection.request.mockReturnValueOnce(secondPending.promise);

    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lanes",
      {
        connect,
        traceCache,
        config: { readLaneCount: 2 },
      },
    );

    const first = server.request(makeRequestEnvelope());
    const second = server.request(makePlanReadEnvelope());

    await vi.waitFor(() => {
      expect(firstConnection.request).toHaveBeenCalledTimes(1);
      expect(secondConnection.request).toHaveBeenCalledTimes(1);
    });

    expect(connect).toHaveBeenCalledTimes(2);
    firstPending.resolve(makeResponse("pure"));
    secondPending.resolve(makeResponse("pure"));

    await expect(first).resolves.toMatchObject({ status: "ok" });
    await expect(second).resolves.toMatchObject({ status: "ok" });
    expect(traceCache.notifyWrite).not.toHaveBeenCalled();
  });

  it("re-enters daemon ensure before reusing a live read lane", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("pure"));
    const connect = vi.fn(async () => readConnection);
    const ensureLifecycle = vi.fn(async () => ({
      runtimeDir: "/tmp/exo-runtime",
      socketPath: "/tmp/exo-runtime/daemon.sock",
      pidPath: "/tmp/exo-runtime/daemon.pid",
      reused: true,
      spawned: false,
      state: "connected_existing",
    }));
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lifecycle-reuse",
      {
        connect,
        ensureLifecycle,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await server.request(makeRequestEnvelope());
    await server.request(makeRequestEnvelope());

    expect(ensureLifecycle).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(1);
    expect(readConnection.request).toHaveBeenCalledTimes(2);
  });

  it("retries pure reads after daemon lifecycle replaces a live read lane", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstConnection = createConnection(makeResponse("pure"));
    const secondConnection = createConnection(makeResponse("pure"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const ensureLifecycle = vi
      .fn<(_: string) => Promise<{
        runtimeDir: string;
        socketPath: string;
        pidPath: string;
        reused: boolean;
        spawned: boolean;
        state: string;
      }>>()
      .mockResolvedValueOnce({
        runtimeDir: "/tmp/exo-runtime",
        socketPath: "/tmp/exo-runtime/daemon.sock",
        pidPath: "/tmp/exo-runtime/daemon.pid",
        reused: false,
        spawned: true,
        state: "spawned",
      })
      .mockResolvedValue({
        runtimeDir: "/tmp/exo-runtime",
        socketPath: "/tmp/exo-runtime/daemon.sock",
        pidPath: "/tmp/exo-runtime/daemon.pid",
        reused: true,
        spawned: false,
        state: "connected_existing",
      });
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lifecycle-replaced",
      {
        connect,
        ensureLifecycle,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await server.request(makeRequestEnvelope());
    await expect(server.request(makeRequestEnvelope())).resolves.toMatchObject({
      status: "ok",
    });

    expect(firstConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
  });

  it("routes write requests through the primary serialized connection", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const primaryConnection = createConnection(makeResponse("write"));
    const connect = vi.fn(async () => primaryConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-primary-write",
      {
        connect,
        traceCache,
        config: { readLaneCount: 4 },
      },
    );

    const response = await server.request(makeTaskCompleteEnvelope());

    expect(response.status).toBe("ok");
    expect(connect).toHaveBeenCalledTimes(1);
    expect(primaryConnection.request).toHaveBeenCalledTimes(1);
    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("re-enters daemon ensure before reusing a live primary socket", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const primaryConnection = createConnection(makeResponse("write"));
    const connect = vi.fn(async () => primaryConnection);
    const ensureLifecycle = vi.fn(async () => ({
      runtimeDir: "/tmp/exo-runtime",
      socketPath: "/tmp/exo-runtime/daemon.sock",
      pidPath: "/tmp/exo-runtime/daemon.pid",
      reused: true,
      spawned: false,
      state: "connected_existing",
    }));
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-primary-lifecycle-reuse",
      {
        connect,
        ensureLifecycle,
        traceCache,
      },
    );

    await server.request(makeTaskCompleteEnvelope());
    await server.request(makeTaskCompleteEnvelope());

    expect(ensureLifecycle).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(1);
    expect(primaryConnection.request).toHaveBeenCalledTimes(2);
  });

  it("reconnects through daemon ensure when Rust lifecycle replaces the primary daemon", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstConnection = createConnection(makeResponse("write"));
    const secondConnection = createConnection(makeResponse("write"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const ensureLifecycle = vi.fn(async () => ({
      runtimeDir: "/tmp/exo-runtime",
      socketPath: "/tmp/exo-runtime/daemon.sock",
      pidPath: "/tmp/exo-runtime/daemon.pid",
      reused: false,
      spawned: true,
      state: "spawned",
    }));
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-primary-lifecycle-replaced",
      {
        connect,
        ensureLifecycle,
        traceCache,
      },
    );

    await server.request(makeTaskCompleteEnvelope());
    await server.request(makeTaskCompleteEnvelope());

    expect(ensureLifecycle).toHaveBeenCalledTimes(1);
    expect(firstConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
  });

  it("reconnects instead of reusing primary sockets after waited-for-lock lifecycle ensures", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstConnection = createConnection(makeResponse("write"));
    const secondConnection = createConnection(makeResponse("write"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const ensureLifecycle = vi.fn(async () => ({
      runtimeDir: "/tmp/exo-runtime",
      socketPath: "/tmp/exo-runtime/daemon.sock",
      pidPath: "/tmp/exo-runtime/daemon.pid",
      reused: true,
      spawned: false,
      state: "waited_for_lock",
    }));
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-primary-lifecycle-waited",
      {
        connect,
        ensureLifecycle,
        traceCache,
      },
    );

    await server.request(makeTaskCompleteEnvelope());
    await server.request(makeTaskCompleteEnvelope());

    expect(ensureLifecycle).toHaveBeenCalledTimes(1);
    expect(firstConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
  });

  it("resets live read lanes when write-side daemon ensure replaces the daemon", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("pure"));
    const primaryConnection = createConnection(makeResponse("write"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(readConnection)
      .mockResolvedValueOnce(primaryConnection);
    const ensureLifecycle = vi.fn(async () => ({
      runtimeDir: "/tmp/exo-runtime",
      socketPath: "/tmp/exo-runtime/daemon.sock",
      pidPath: "/tmp/exo-runtime/daemon.pid",
      reused: false,
      spawned: true,
      state: "spawned",
    }));
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-write-replaces-read-lanes",
      {
        connect,
        ensureLifecycle,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await server.request(makeRequestEnvelope());
    await server.request(makeTaskCompleteEnvelope());

    expect(readConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(primaryConnection.request).toHaveBeenCalledTimes(1);
  });

  it("resets a live primary socket when read-side daemon ensure replaces the daemon", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const primaryConnection = createConnection(makeResponse("write"));
    const readConnection = createConnection(makeResponse("pure"));
    const replacementPrimaryConnection = createConnection(makeResponse("write"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(primaryConnection)
      .mockResolvedValueOnce(readConnection)
      .mockResolvedValueOnce(replacementPrimaryConnection);
    const ensureLifecycle = vi
      .fn<(_: string) => Promise<{
        runtimeDir: string;
        socketPath: string;
        pidPath: string;
        reused: boolean;
        spawned: boolean;
        state: string;
      }>>()
      .mockResolvedValueOnce({
        runtimeDir: "/tmp/exo-runtime",
        socketPath: "/tmp/exo-runtime/daemon.sock",
        pidPath: "/tmp/exo-runtime/daemon.pid",
        reused: false,
        spawned: true,
        state: "spawned",
      })
      .mockResolvedValue({
        runtimeDir: "/tmp/exo-runtime",
        socketPath: "/tmp/exo-runtime/daemon.sock",
        pidPath: "/tmp/exo-runtime/daemon.pid",
        reused: true,
        spawned: false,
        state: "connected_existing",
      });
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-replaces-primary",
      {
        connect,
        ensureLifecycle,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await server.request(makeTaskCompleteEnvelope());
    await server.request(makeRequestEnvelope());
    await server.request(makeTaskCompleteEnvelope());

    expect(primaryConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(3);
    expect(readConnection.request).toHaveBeenCalledTimes(1);
    expect(replacementPrimaryConnection.request).toHaveBeenCalledTimes(1);
  });

  it("routes compound pure operations over read lanes", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("pure"));
    const primaryConnection = createConnection(makeResponse("write"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(readConnection)
      .mockResolvedValueOnce(primaryConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-compound-pure-read",
      {
        connect,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await expect(
      server.request(makePhaseExecutionTasksEnvelope()),
    ).resolves.toMatchObject({ status: "ok" });
    await expect(server.request(makeTaskCompleteEnvelope())).resolves.toMatchObject({
      status: "ok",
    });

    expect(connect).toHaveBeenCalledTimes(2);
    expect(readConnection.request).toHaveBeenCalledWith(
      makePhaseExecutionTasksEnvelope(),
      expect.any(Number),
    );
    expect(primaryConnection.request).toHaveBeenCalledWith(
      makeTaskCompleteEnvelope(),
      expect.any(Number),
    );
  });

  it("notifies TraceCache when a read lane receives a write notification", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("pure"));
    const connect = vi.fn(async () => readConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lane-notify",
      {
        connect,
        traceCache,
      },
    );

    await server.request(makeRequestEnvelope());
    readConnection.onNotification?.({
      protocol_version: 1,
      id: "_notify",
      status: "ok",
      result: { kind: "write_happened" },
    });

    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("fails fast when a read lane receives a non-pure response", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("write"));
    const connect = vi.fn(async () => readConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lane-non-pure-response",
      {
        connect,
        traceCache,
      },
    );

    await expect(server.request(makeRequestEnvelope())).rejects.toThrow(
      "Read lane 0 received non-pure response effect=write",
    );
    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("keeps active read lanes open while the primary connection reconnects", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readPending = deferred<MachineChannelResponseEnvelope>();
    const readConnection = createConnection(makeResponse("pure"));
    const primaryConnection = createConnection(makeResponse("write"));
    readConnection.request.mockReturnValueOnce(readPending.promise);
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(readConnection)
      .mockResolvedValueOnce(primaryConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-primary-reconnect-keeps-read-lanes",
      {
        connect,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    const read = server.request(makeRequestEnvelope());
    await vi.waitFor(() => {
      expect(readConnection.request).toHaveBeenCalledTimes(1);
    });

    await expect(server.request(makeTaskCompleteEnvelope())).resolves.toMatchObject({
      status: "ok",
    });

    expect(readConnection.close).not.toHaveBeenCalled();
    readPending.resolve(makeResponse("pure"));
    await expect(read).resolves.toMatchObject({ status: "ok" });
    expect(primaryConnection.request).toHaveBeenCalledTimes(1);
  });

  it("counts concurrent read-lane startup failures as one reconnect failure", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connectPending = deferred<ConnectionLike>();
    const connect = vi.fn(() => connectPending.promise);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lane-failure-wave",
      {
        connect,
        traceCache,
        config: {
          readLaneCount: 4,
          maxReconnectAttempts: 2,
          reconnectCooldownMs: 1_000,
        },
      },
    );

    const reads = [
      server.request(makeRequestEnvelope()),
      server.request(makePlanReadEnvelope()),
      server.request(makeRequestEnvelope()),
      server.request(makePlanReadEnvelope()),
    ];
    await vi.waitFor(() => {
      expect(connect).toHaveBeenCalledTimes(4);
    });

    connectPending.reject(new Error("daemon unavailable"));
    await Promise.all(
      reads.map((read) =>
        expect(read).rejects.toThrow("daemon unavailable"),
      ),
    );

    expect(server.getServerModeAvailability()).toMatchObject({
      available: true,
      reconnectAttempts: 1,
    });
  });

  it("applies reconnect backoff before opening a fresh read lane", async () => {
    const fakeClock = createFakeClock();
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("pure"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockRejectedValueOnce(new Error("daemon unavailable"))
      .mockResolvedValueOnce(readConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lane-backoff",
      {
        connect,
        traceCache,
        clock: fakeClock.clock,
        config: {
          readLaneCount: 1,
          reconnectDelayMs: 1_000,
          reconnectCooldownMs: 5_000,
        },
      },
    );

    await expect(server.request(makeRequestEnvelope())).rejects.toThrow(
      "daemon unavailable",
    );

    const retry = server.request(makeRequestEnvelope());
    await vi.waitFor(() => {
      expect(fakeClock.timerCount()).toBe(1);
    });
    expect(connect).toHaveBeenCalledTimes(1);

    fakeClock.advance(1_000);
    expect(fakeClock.runNextTimer()).toBe(true);

    await expect(retry).resolves.toMatchObject({ status: "ok" });
    expect(connect).toHaveBeenCalledTimes(2);
    expect(readConnection.request).toHaveBeenCalledTimes(1);
  });

  it("cancels read-lane retries after disposal", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connectPending = deferred<ConnectionLike>();
    const staleConnection = createConnection(makeResponse("pure"));
    const connect = vi.fn(() => connectPending.promise);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lane-dispose",
      {
        connect,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    const read = server.request(makeRequestEnvelope());
    await vi.waitFor(() => {
      expect(connect).toHaveBeenCalledTimes(1);
    });

    server.dispose();
    connectPending.resolve(staleConnection);

    await expect(read).rejects.toThrow("request canceled during shutdown");
    expect(staleConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(1);
  });

  it("client reset closes read lanes and reconnects reads through ensure path", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstReadConnection = createConnection(makeResponse("pure"));
    const freshReadConnection = createConnection(makeResponse("pure"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstReadConnection)
      .mockResolvedValueOnce(freshReadConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lane-reset",
      {
        connect,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await server.request(makeRequestEnvelope());
    await server.restart();
    await server.request(makePlanReadEnvelope());

    expect(firstReadConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(freshReadConnection.request).toHaveBeenCalledTimes(1);
    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(2);
  });

  it("rejects stale read-lane connect completions after client reset", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstReadConnection = createConnection(makeResponse("pure"));
    const staleConnectPending = deferred<ConnectionLike>();
    const staleConnectedAfterRestart = createConnection(makeResponse("pure"));
    const freshReadConnection = createConnection(makeResponse("pure"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstReadConnection)
      .mockReturnValueOnce(staleConnectPending.promise)
      .mockResolvedValueOnce(freshReadConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-stale-read-connect-generation",
      {
        connect,
        traceCache,
        config: { readLaneCount: 2 },
      },
    );

    await server.request(makeRequestEnvelope());
    const staleRead = server.request(makePlanReadEnvelope());
    await vi.waitFor(() => {
      expect(connect).toHaveBeenCalledTimes(2);
    });

    await server.restart();
    staleConnectPending.resolve(staleConnectedAfterRestart);

    await expect(staleRead).rejects.toThrow(
      "request canceled during shutdown or restart",
    );
    expect(staleConnectedAfterRestart.close).toHaveBeenCalledTimes(1);
    await expect(server.request(makePlanReadEnvelope())).resolves.toMatchObject({
      status: "ok",
    });
    expect(connect).toHaveBeenCalledTimes(3);
    expect(freshReadConnection.request).toHaveBeenCalledTimes(1);
  });

  it("client reset after a read lane reconnects writes through ensure path", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("pure"));
    const freshPrimaryConnection = createConnection(makeResponse("write"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(readConnection)
      .mockResolvedValueOnce(freshPrimaryConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-write-after-read-lane-reset",
      {
        connect,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await server.request(makeRequestEnvelope());
    await server.restart();
    await expect(server.request(makeTaskCompleteEnvelope())).resolves.toMatchObject({
      status: "ok",
    });

    expect(readConnection.close).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(freshPrimaryConnection.request).toHaveBeenCalledTimes(1);
  });

  it("marks read-lane connects as prior daemon connections for primary reconnect invalidation", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const readConnection = createConnection(makeResponse("pure"));
    const primaryConnection = createConnection(makeResponse("write"));
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(readConnection)
      .mockResolvedValueOnce(primaryConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-read-lane-counts-as-connected",
      {
        connect,
        traceCache,
        config: { readLaneCount: 1 },
      },
    );

    await server.request(makeRequestEnvelope());
    await server.request(makeTaskCompleteEnvelope());

    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(2);
    expect(primaryConnection.request).toHaveBeenCalledTimes(1);
  });

  it("uses the daemon socket request timeout by default", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection();
    const connect = vi.fn(async () => connection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-timeout",
      {
        connect,
        traceCache,
      },
    );

    await server.request(makeRequestEnvelope());

    expect(connection.request).toHaveBeenCalledWith(
      expect.any(Object),
      30_000,
    );
  });

  it("notifies writes on reconnect (closed connection)", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstConnection = createConnection();
    const secondConnection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-2", {
      connect,
      traceCache,
    });

    await server.request(makeTaskCompleteEnvelope());
    firstConnection.setClosed(true);

    await server.request(makeTaskCompleteEnvelope());

    expect(connect).toHaveBeenCalledTimes(2);
    expect(firstConnection.close).toHaveBeenCalledTimes(1);
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("notifies writes on reconnect after client reset", async () => {
    // Simulates an explicit client reset: restart() calls cleanup() before the
    // next connect(), so this.connection is null when connect() starts.
    // The fix uses hasEverConnected (not this.connection) to detect reconnects.
    const traceCache = { notifyWrite: vi.fn() };
    const firstConnection = createConnection();
    const secondConnection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-5", {
      connect,
      traceCache,
    });

    // First request establishes connection
    await server.request(makeTaskCompleteEnvelope());
    expect(traceCache.notifyWrite).not.toHaveBeenCalled();

    // Client reset does cleanup() (nulls connection), then next request()
    // triggers connect() with this.connection === null.
    await server.restart();
    await server.request(makeTaskCompleteEnvelope());

    expect(connect).toHaveBeenCalledTimes(2);
    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(2);
  });

  it("notifies writes for write responses", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection(makeResponse("write"));
    const connect = vi.fn(async () => connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-3", {
      connect,
      traceCache,
    });

    await server.request(makeTaskCompleteEnvelope());

    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("notifies writes for exec responses", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection(makeResponse("exec"));
    const connect = vi.fn(async () => connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-exec", {
      connect,
      traceCache,
    });

    await server.request(makeTaskCompleteEnvelope());

    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("notifies writes for push notifications", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection();
    const connect = vi.fn(async () => connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-4", {
      connect,
      traceCache,
    });

    await server.request(makeRequestEnvelope());
    connection.onNotification?.({
      protocol_version: 1,
      id: "_notify",
      status: "ok",
      result: { kind: "write_happened" },
    });

    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("treats sidecar repo status as a retry-safe read request", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstConnection = createConnection();
    const secondConnection = createConnection(makeResponse("pure"));
    firstConnection.request.mockRejectedValueOnce(
      new Error("Connection closed"),
    );

    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-sidecar-repo-status",
      {
        connect,
        traceCache,
      },
    );

    const response = await server.request(makeSidecarRepoStatusEnvelope());

    expect(response.status).toBe("ok");
    expect(connect).toHaveBeenCalledTimes(2);
    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("retries once after write EPIPE and succeeds on the second connection", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const epipe = Object.assign(new Error("write EPIPE"), { code: "EPIPE" });
    const firstConnection = createConnection();
    firstConnection.request.mockRejectedValueOnce(epipe);
    const secondConnection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-6", {
      connect,
      traceCache,
    });

    const response = await server.request(makeRequestEnvelope());

    expect(response.status).toBe("ok");
    expect(connect).toHaveBeenCalledTimes(2);
    expect(firstConnection.request).toHaveBeenCalledTimes(1);
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
  });

  it("retries once after ERR_STREAM_DESTROYED and succeeds", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const destroyed = Object.assign(
      new Error("Cannot call write after a stream was destroyed"),
      { code: "ERR_STREAM_DESTROYED" },
    );
    const firstConnection = createConnection();
    firstConnection.request.mockRejectedValueOnce(destroyed);
    const secondConnection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-7", {
      connect,
      traceCache,
    });

    const response = await server.request(makeRequestEnvelope());

    expect(response.status).toBe("ok");
    expect(connect).toHaveBeenCalledTimes(2);
    expect(firstConnection.request).toHaveBeenCalledTimes(1);
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
  });

  it("retries pure plan.read operation calls after connection loss", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const epipe = Object.assign(new Error("write EPIPE"), { code: "EPIPE" });
    const firstConnection = createConnection();
    firstConnection.request.mockRejectedValueOnce(epipe);
    const secondConnection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-10", {
      connect,
      traceCache,
    });

    const response = await server.request(makePlanReadEnvelope());

    expect(response.status).toBe("ok");
    expect(connect).toHaveBeenCalledTimes(2);
    expect(firstConnection.request).toHaveBeenCalledTimes(1);
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
  });

  it("does not retry non-transient request errors", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const firstConnection = createConnection();
    firstConnection.request.mockRejectedValueOnce(new Error("request failed"));
    const connect = vi.fn(async () => firstConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-8", {
      connect,
      traceCache,
    });

    await expect(server.request(makeRequestEnvelope())).rejects.toThrow(
      "request failed",
    );

    expect(connect).toHaveBeenCalledTimes(1);
    expect(firstConnection.request).toHaveBeenCalledTimes(1);
  });

  it("does not retry write operation calls after connection loss", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const epipe = Object.assign(new Error("write EPIPE"), { code: "EPIPE" });
    const firstConnection = createConnection();
    firstConnection.request.mockRejectedValueOnce(epipe);
    const connect = vi.fn(async () => firstConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-9", {
      connect,
      traceCache,
    });

    await expect(server.request(makeTaskCompleteEnvelope())).rejects.toThrow(
      "write EPIPE",
    );

    expect(connect).toHaveBeenCalledTimes(1);
    expect(firstConnection.request).toHaveBeenCalledTimes(1);
  });

  it("enters reconnect cooldown after transient failures and self-heals after cooldown", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const fakeClock = createFakeClock();
    const connection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockRejectedValueOnce(new Error("daemon unavailable 1"))
      .mockRejectedValueOnce(new Error("daemon unavailable 2"))
      .mockResolvedValueOnce(connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-11", {
      connect,
      traceCache,
      clock: fakeClock.clock,
      config: {
        maxReconnectAttempts: 1,
        reconnectDelayMs: 0,
        reconnectCooldownMs: 1_000,
      },
    });

    await expect(server.request(makeRequestEnvelope())).rejects.toThrow(
      "daemon unavailable 1",
    );
    await expect(server.request(makeRequestEnvelope())).rejects.toThrow(
      "daemon unavailable 2",
    );

    expect(server.getServerModeAvailability()).toMatchObject({
      available: false,
      reason: "cooldown",
      retryAfterMs: 1_000,
    });
    expect(server.shouldUseServerMode()).toBe(false);

    await expect(server.request(makeRequestEnvelope())).rejects.toThrow(
      "Server mode is cooling down",
    );
    expect(connect).toHaveBeenCalledTimes(2);

    fakeClock.advance(1_000);
    expect(server.shouldUseServerMode()).toBe(true);

    await expect(server.request(makeRequestEnvelope())).resolves.toMatchObject({
      status: "ok",
    });

    expect(connect).toHaveBeenCalledTimes(3);
    expect(connection.request).toHaveBeenCalledTimes(1);
    expect(server.getServerModeAvailability()).toMatchObject({
      available: true,
      reconnectAttempts: 0,
    });
  });

  it("restart clears reconnect cooldown and scheduled reconnect state", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const fakeClock = createFakeClock();
    const firstConnection = createConnection();
    const secondConnection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockResolvedValueOnce(firstConnection)
      .mockRejectedValueOnce(new Error("daemon unavailable after close 1"))
      .mockRejectedValueOnce(new Error("daemon unavailable after close 2"))
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-12", {
      connect,
      traceCache,
      clock: fakeClock.clock,
      config: {
        maxReconnectAttempts: 1,
        reconnectDelayMs: 0,
        reconnectCooldownMs: 1_000,
        autoReconnectDelayMs: 250,
      },
    });

    await server.request(makeTaskCompleteEnvelope());
    firstConnection.onClose?.();

    expect(fakeClock.timerCount()).toBe(1);

    await server.restart();

    expect(fakeClock.timerCount()).toBe(0);
    expect(server.shouldUseServerMode()).toBe(true);

    await expect(server.request(makeTaskCompleteEnvelope())).rejects.toThrow(
      "daemon unavailable after close 1",
    );
    await expect(server.request(makeTaskCompleteEnvelope())).rejects.toThrow(
      "daemon unavailable after close 2",
    );
    expect(server.shouldUseServerMode()).toBe(false);

    await server.restart();

    expect(server.shouldUseServerMode()).toBe(true);

    await expect(server.request(makeTaskCompleteEnvelope())).resolves.toMatchObject({
      status: "ok",
    });
    expect(secondConnection.request).toHaveBeenCalledTimes(1);
  });

  it("restart notifies consumers to invalidate daemon-backed reads", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection();
    const connect = vi.fn(async () => connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-18", {
      connect,
      traceCache,
    });

    await server.restart();

    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("explicit restart invokes Rust daemon restart before resetting local sockets", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection();
    const connect = vi.fn(async () => connection);
    const restartLifecycle = vi.fn(async () => ({
      runtimeDir: "/tmp/exo-runtime",
      socketPath: "/tmp/exo-runtime/daemon.sock",
      pidPath: "/tmp/exo-runtime/daemon.pid",
      reused: false,
      spawned: true,
      state: "spawned",
    }));
    const server = DaemonChannelServer.createForTesting(
      "/tmp/exo2-daemon-explicit-restart",
      {
        connect,
        restartLifecycle,
        traceCache,
      },
    );

    await server.request(makeTaskCompleteEnvelope());
    await server.restart({ restartDaemon: true });

    expect(restartLifecycle).toHaveBeenCalledWith(
      "/tmp/exo2-daemon-explicit-restart",
    );
    expect(connection.close).toHaveBeenCalledTimes(1);
    expect(traceCache.notifyWrite).toHaveBeenCalledTimes(1);
  });

  it("restart ignores stale pending connect completion", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const fakeClock = createFakeClock();
    let rejectFirstConnect: ((error: Error) => void) | undefined;
    const secondConnection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockImplementationOnce(
        () =>
          new Promise<ConnectionLike>((_resolve, reject) => {
            rejectFirstConnect = reject;
          }),
      )
      .mockResolvedValueOnce(secondConnection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-15", {
      connect,
      traceCache,
      clock: fakeClock.clock,
      config: {
        maxReconnectAttempts: 0,
        reconnectDelayMs: 0,
        reconnectCooldownMs: 1_000,
      },
    });

    const staleRequest = server.request(makeTaskCompleteEnvelope());
    await Promise.resolve();
    expect(connect).toHaveBeenCalledTimes(1);

    await server.restart();
    rejectFirstConnect?.(new Error("stale connect failed after restart"));
    await expect(staleRequest).resolves.toMatchObject({
      status: "ok",
    });

    expect(server.getServerModeAvailability()).toMatchObject({
      available: true,
      reconnectAttempts: 0,
    });

    await expect(server.request(makeTaskCompleteEnvelope())).resolves.toMatchObject({
      status: "ok",
    });
    expect(connect).toHaveBeenCalledTimes(2);
    expect(secondConnection.request).toHaveBeenCalledTimes(2);
  });

  it("successful connect resets reconnect failure counters", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockRejectedValueOnce(new Error("daemon unavailable"))
      .mockResolvedValueOnce(connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-13", {
      connect,
      traceCache,
      config: {
        maxReconnectAttempts: 2,
        reconnectDelayMs: 0,
        reconnectCooldownMs: 1_000,
      },
    });

    await expect(server.request(makeTaskCompleteEnvelope())).rejects.toThrow(
      "daemon unavailable",
    );
    expect(server.getServerModeAvailability()).toMatchObject({
      available: true,
      reconnectAttempts: 1,
    });

    await expect(server.request(makeTaskCompleteEnvelope())).resolves.toMatchObject({
      status: "ok",
    });

    expect(server.getServerModeAvailability()).toMatchObject({
      available: true,
      reconnectAttempts: 0,
    });
  });

  it("reports env-disabled separately from reconnect cooldown", () => {
    const traceCache = { notifyWrite: vi.fn() };
    const connect = vi.fn(async () => createConnection());
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-14", {
      connect,
      traceCache,
    });

    vi.stubEnv("EXOSUIT_USE_SERVER_MODE", "false");

    expect(server.getServerModeAvailability()).toEqual({
      available: false,
      reason: "env-disabled",
      workspaceRoot: "/tmp/exo2-daemon-14",
      envVar: "EXOSUIT_USE_SERVER_MODE",
      value: "false",
    });
    expect(server.shouldUseServerMode()).toBe(false);
  });

  it("waits reconnectDelayMs before retrying after a failed connect", async () => {
    const traceCache = { notifyWrite: vi.fn() };
    const fakeClock = createFakeClock();
    const connection = createConnection();
    const connect = vi
      .fn<(_: string) => Promise<ConnectionLike>>()
      .mockRejectedValueOnce(new Error("daemon unavailable"))
      .mockResolvedValueOnce(connection);
    const server = DaemonChannelServer.createForTesting("/tmp/exo2-daemon-16", {
      connect,
      traceCache,
      clock: fakeClock.clock,
      config: {
        maxReconnectAttempts: 2,
        reconnectDelayMs: 250,
        reconnectCooldownMs: 1_000,
      },
    });

    await expect(server.request(makeTaskCompleteEnvelope())).rejects.toThrow(
      "daemon unavailable",
    );

    const retry = server.request(makeTaskCompleteEnvelope());
    await Promise.resolve();

    expect(connect).toHaveBeenCalledTimes(1);
    expect(fakeClock.timerCount()).toBe(1);

    expect(fakeClock.runNextTimer()).toBe(true);

    await expect(retry).resolves.toMatchObject({ status: "ok" });
    expect(connect).toHaveBeenCalledTimes(2);
    expect(connection.request).toHaveBeenCalledTimes(1);
  });
});
