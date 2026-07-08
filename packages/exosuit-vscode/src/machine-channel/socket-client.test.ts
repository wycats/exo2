import { EventEmitter } from "node:events";
import { describe, expect, it, vi, beforeEach } from "vitest";

const { connectMock, spawnMock } = vi.hoisted(() => {
  return {
    connectMock: vi.fn(),
    spawnMock: vi.fn(),
  };
});

vi.mock("node:net", () => ({
  connect: connectMock,
  default: { connect: connectMock },
}));

vi.mock("node:child_process", () => ({
  spawn: spawnMock,
  default: { spawn: spawnMock },
}));

vi.mock("../exoBin", () => ({
  resolveExoBinary: vi.fn(() => "/bin/exo"),
}));

import {
  DaemonConnection,
  connectToSocket,
  daemonStatus,
  ensureDaemon,
  ensureDaemonLifecycle,
  ensureDaemonWithConnector,
  ensureDaemonRuntimePaths,
  FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR,
  restartDaemonLifecycle,
  resetDaemonEnsureCacheForTesting,
  resolveDaemonRuntimePaths,
  getEndpoint,
  getSocketPath,
  getPidPath,
  getRuntimeDir,
  type DaemonRuntimePaths,
} from "./socket-client";

class FakeSocket extends EventEmitter {
  destroyed = false;
  writes: string[] = [];
  private writeError: Error | null = null;

  setEncoding = vi.fn();
  pause = vi.fn();
  resume = vi.fn();

  setWriteError(error: Error): void {
    this.writeError = error;
  }

  write(_data: string, callback?: (error?: Error) => void): boolean {
    this.writes.push(_data);
    queueMicrotask(() => callback?.(this.writeError ?? undefined));
    return this.writeError === null;
  }

  emitLine(line: string): void {
    this.emit("data", `${line}\n`);
  }

  destroy(): this {
    this.destroyed = true;
    queueMicrotask(() => this.emit("close"));
    return this;
  }
}

function childProcessResult(stdout: string, stderr = "", code = 0) {
  let stdoutData: ((chunk: string) => void) | undefined;
  let stderrData: ((chunk: string) => void) | undefined;
  let closeHandler: ((code: number) => void) | undefined;

  const child = {
    stdout: {
      setEncoding: vi.fn(),
      on: vi.fn((event: string, callback: (chunk: string) => void) => {
        if (event === "data") {
          stdoutData = callback;
        }
      }),
    },
    stderr: {
      setEncoding: vi.fn(),
      on: vi.fn((event: string, callback: (chunk: string) => void) => {
        if (event === "data") {
          stderrData = callback;
        }
      }),
    },
    on: vi.fn((event: string, callback: (value: unknown) => void) => {
      if (event === "close") {
        closeHandler = callback as (code: number) => void;
      }
    }),
    unref: vi.fn(),
  };

  queueMicrotask(() => {
    if (stdout) {
      stdoutData?.(stdout);
    }
    if (stderr) {
      stderrData?.(stderr);
    }
    closeHandler?.(code);
  });

  return child;
}

const projectPathsJson = JSON.stringify({
  status: "ok",
  result: {
    paths: {
      runtime_dir: "/project/.exo/runtime",
      socket_path: "/project/.exo/runtime/daemon.sock",
      endpoint: "/project/.exo/runtime/daemon.sock",
      pid_path: "/project/.exo/runtime/daemon.pid",
    },
  },
});

const daemonEnsureJson = JSON.stringify({
  status: "ok",
  result: {
    kind: "daemon.ensure",
    ok: true,
    workspace_root: "/workspace",
    runtime_dir: "/project/.exo/runtime",
    socket_path: "/project/.exo/runtime/daemon.sock",
    endpoint: "/project/.exo/runtime/daemon.sock",
    pid_path: "/project/.exo/runtime/daemon.pid",
    pid: 12345,
    instance_id: "daemon-test",
    probe_ok: true,
    state: "spawned",
    connected: true,
    spawned: true,
    reused: false,
    diagnostics: ["spawned daemon process", "connected to daemon socket"],
  },
});

const daemonStatusJson = JSON.stringify({
  status: "ok",
  result: {
    kind: "daemon.status",
    ok: false,
    workspace_root: "/workspace",
    runtime_dir: "/project/.exo/runtime",
    socket_path: "/project/.exo/runtime/daemon.sock",
    endpoint: "/project/.exo/runtime/daemon.sock",
    pid_path: "/project/.exo/runtime/daemon.pid",
    identity_path: "/project/.exo/runtime/daemon.identity.json",
    pid: 12345,
    pid_alive: true,
    socket_exists: true,
    socket_connectable: false,
    identity_matches_workspace: true,
    identity_matches_executable: false,
    state: "stale_identity",
  },
});

const projectPaths: DaemonRuntimePaths = {
  runtimeDir: "/project/.exo/runtime",
  socketPath: "/project/.exo/runtime/daemon.sock",
  endpoint: "/project/.exo/runtime/daemon.sock",
  pidPath: "/project/.exo/runtime/daemon.pid",
};

describe("socket-client daemon runtime paths", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    connectMock.mockReset();
    resetDaemonEnsureCacheForTesting();
  });

  it("uses direct project resolve to fetch daemon paths", async () => {
    spawnMock.mockReturnValueOnce(childProcessResult(projectPathsJson));

    const paths = await resolveDaemonRuntimePaths("/workspace");

    expect(paths).toEqual(projectPaths);
    expect(spawnMock).toHaveBeenCalledWith(
      "/bin/exo",
      ["--format", "json", "--direct", "project", "resolve"],
      {
        cwd: "/workspace",
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
  });

  it("uses direct daemon ensure to fetch authoritative daemon paths", async () => {
    spawnMock.mockReturnValueOnce(childProcessResult(daemonEnsureJson));

    const paths = await ensureDaemonRuntimePaths("/workspace");

    expect(paths).toEqual(projectPaths);
    expect(spawnMock).toHaveBeenCalledWith(
      "/bin/exo",
      [
        "--format",
        "json",
        "--direct",
        "daemon",
        "ensure",
        "--workspace",
        "/workspace",
      ],
      {
        cwd: "/workspace",
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
  });

  it("exposes daemon ensure lifecycle reuse fields", async () => {
    spawnMock.mockReturnValueOnce(childProcessResult(daemonEnsureJson));

    const result = await ensureDaemonLifecycle("/workspace");

    expect(result).toEqual({
      ...projectPaths,
      pid: 12345,
      instanceId: "daemon-test",
      probeOk: true,
      reused: false,
      spawned: true,
      state: "spawned",
    });
  });

  it("uses direct daemon restart for explicit lifecycle restart", async () => {
    spawnMock.mockReturnValueOnce(childProcessResult(daemonEnsureJson));

    const result = await restartDaemonLifecycle("/workspace");

    expect(result).toEqual({
      ...projectPaths,
      pid: 12345,
      instanceId: "daemon-test",
      probeOk: true,
      reused: false,
      spawned: true,
      state: "spawned",
    });
    expect(spawnMock).toHaveBeenCalledWith(
      "/bin/exo",
      [
        "--format",
        "json",
        "--direct",
        "daemon",
        "restart",
        "--workspace",
        "/workspace",
      ],
      {
        cwd: "/workspace",
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
  });

  it("uses direct daemon status to inspect daemon identity diagnostics", async () => {
    spawnMock.mockReturnValueOnce(childProcessResult(daemonStatusJson));

    const status = await daemonStatus("/workspace");

    expect(status).toEqual({
      state: "stale_identity",
      socket_path: "/project/.exo/runtime/daemon.sock",
      endpoint: "/project/.exo/runtime/daemon.sock",
      pid: 12345,
      pid_alive: true,
      socket_exists: true,
      socket_connectable: false,
      identity_matches_workspace: true,
      identity_matches_executable: false,
    });
    expect(spawnMock).toHaveBeenCalledWith(
      "/bin/exo",
      [
        "--format",
        "json",
        "--direct",
        "daemon",
        "status",
        "--workspace",
        "/workspace",
      ],
      {
        cwd: "/workspace",
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
  });

  it("rejects filesystem root before project resolve", async () => {
    await expect(resolveDaemonRuntimePaths("/")).rejects.toThrow(
      FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR,
    );

    expect(spawnMock).not.toHaveBeenCalled();
  });

  it("reports daemon ensure failure messages", async () => {
    spawnMock.mockReturnValueOnce(
      childProcessResult(
        JSON.stringify({
          status: "error",
          error: { message: "daemon ensure failed" },
        }),
      ),
    );

    await expect(ensureDaemonRuntimePaths("/workspace")).rejects.toThrow(
      "daemon ensure failed",
    );
  });

  it("rejects daemon ensure output without socket path", async () => {
    spawnMock.mockReturnValueOnce(
      childProcessResult(
        JSON.stringify({
          status: "ok",
          result: {
            kind: "daemon.ensure",
            ok: true,
            runtime_dir: "/project/.exo/runtime",
            pid_path: "/project/.exo/runtime/daemon.pid",
          },
        }),
      ),
    );

    await expect(ensureDaemonRuntimePaths("/workspace")).rejects.toThrow(
      "daemon ensure did not return daemon runtime paths",
    );
  });

  it("reports malformed daemon ensure JSON", async () => {
    spawnMock.mockReturnValueOnce(childProcessResult("not json"));

    await expect(ensureDaemonRuntimePaths("/workspace")).rejects.toThrow(
      "Failed to parse daemon ensure output",
    );
  });

  it("does not construct workspace .runtime paths", () => {
    expect(getRuntimeDir(projectPaths)).toBe("/project/.exo/runtime");
    expect(getSocketPath(projectPaths)).toBe(
      "/project/.exo/runtime/daemon.sock",
    );
    expect(getEndpoint(projectPaths)).toBe("/project/.exo/runtime/daemon.sock");
    expect(getPidPath(projectPaths)).toBe("/project/.exo/runtime/daemon.pid");
  });

  it("retries transient Windows named-pipe connect failures", async () => {
    const firstSocket = new FakeSocket();
    const secondSocket = new FakeSocket();
    connectMock
      .mockImplementationOnce(() => {
        queueMicrotask(() => {
          const error = Object.assign(new Error("pipe not ready"), {
            code: "ENOENT",
          });
          firstSocket.emit("error", error);
        });
        return firstSocket;
      })
      .mockImplementationOnce(() => {
        queueMicrotask(() => secondSocket.emit("connect"));
        return secondSocket;
      });

    await expect(connectToSocket("\\\\.\\pipe\\exo-test")).resolves.toBe(
      secondSocket,
    );
    expect(connectMock).toHaveBeenCalledTimes(2);
    expect(firstSocket.destroyed).toBe(true);
  });

  it("rejects filesystem root before ensuring daemon", async () => {
    await expect(ensureDaemon("/")).rejects.toThrow(
      FILESYSTEM_ROOT_DAEMON_WORKSPACE_ERROR,
    );

    expect(spawnMock).not.toHaveBeenCalled();
  });

  it("ensureDaemon connects to daemon ensure endpoint and does not spawn daemon run", async () => {
    vi.clearAllMocks();
    spawnMock.mockReturnValueOnce(childProcessResult(daemonEnsureJson));
    const socket = new FakeSocket();
    const connectToSocket = vi.fn(async () => {
      queueMicrotask(() => socket.emit("connect"));
      return socket as never;
    });

    const connected = await ensureDaemonWithConnector("/workspace", {
      connectToSocket,
    });

    expect(connected).toBe(socket);
    expect(connectToSocket).toHaveBeenCalledWith(
      "/project/.exo/runtime/daemon.sock",
    );
    expect(spawnMock).toHaveBeenCalledTimes(1);
    expect(spawnMock).toHaveBeenCalledWith(
      "/bin/exo",
      [
        "--format",
        "json",
        "--direct",
        "daemon",
        "ensure",
        "--workspace",
        "/workspace",
      ],
      {
        cwd: "/workspace",
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    expect(
      spawnMock.mock.calls.some((call) =>
        (call[1] as string[] | undefined)?.includes("run"),
      ),
    ).toBe(false);
  });

  it("ensureDaemon connects to the reported endpoint when it differs from socket path", async () => {
    const endpointJson = JSON.stringify({
      status: "ok",
      result: {
        kind: "daemon.ensure",
        ok: true,
        workspace_root: "/workspace",
        runtime_dir: "/project/.exo/runtime",
        socket_path: "/project/.exo/runtime/daemon.sock",
        endpoint: "\\\\.\\pipe\\exo-test",
        pid_path: "/project/.exo/runtime/daemon.pid",
        pid: 12345,
        state: "connected_existing",
        connected: true,
        spawned: false,
        reused: true,
        diagnostics: [],
      },
    });
    spawnMock.mockReturnValueOnce(childProcessResult(endpointJson));
    const socket = new FakeSocket();
    const connectToSocket = vi.fn(async () => socket as never);

    const connected = await ensureDaemonWithConnector("/workspace", {
      connectToSocket,
    });

    expect(connected).toBe(socket);
    expect(connectToSocket).toHaveBeenCalledWith("\\\\.\\pipe\\exo-test");
  });

  it("includes daemon status diagnostics when ensure socket cannot be reached", async () => {
    spawnMock
      .mockImplementationOnce(() => childProcessResult(daemonEnsureJson))
      .mockImplementationOnce(() => childProcessResult(daemonStatusJson));
    const connectToSocket = vi.fn(async () => null);

    let message = "";
    try {
      await ensureDaemonWithConnector("/workspace", { connectToSocket });
    } catch (error) {
      message = String(error);
    }

    expect(message).toContain("daemonStatus.state=stale_identity");
    expect(message).toContain(
      "daemonStatus.socket_path=/project/.exo/runtime/daemon.sock",
    );
    expect(message).toContain(
      "daemonStatus.endpoint=/project/.exo/runtime/daemon.sock",
    );
    expect(message).toContain("daemonStatus.pid=12345");
    expect(message).toContain("daemonStatus.pid_alive=true");
    expect(message).toContain("daemonStatus.socket_exists=true");
    expect(message).toContain("daemonStatus.socket_connectable=false");
    expect(message).toContain("daemonStatus.identity_matches_workspace=true");
    expect(message).toContain("daemonStatus.identity_matches_executable=false");

    expect(spawnMock).toHaveBeenCalledTimes(2);
    expect(connectToSocket).toHaveBeenCalledWith(
      "/project/.exo/runtime/daemon.sock",
    );
  });

  it("coalesces concurrent daemon ensure calls per workspace", async () => {
    let stdoutData: ((chunk: string) => void) | undefined;
    let closeHandler: ((code: number) => void) | undefined;
    const child = {
      stdout: {
        setEncoding: vi.fn(),
        on: vi.fn((event: string, callback: (chunk: string) => void) => {
          if (event === "data") {
            stdoutData = callback;
          }
        }),
      },
      stderr: {
        setEncoding: vi.fn(),
        on: vi.fn(),
      },
      on: vi.fn((event: string, callback: (code: number) => void) => {
        if (event === "close") {
          closeHandler = callback;
        }
      }),
      unref: vi.fn(),
    };
    spawnMock.mockReturnValueOnce(child);
    const firstSocket = new FakeSocket();
    const secondSocket = new FakeSocket();
    const connectToSocket = vi
      .fn()
      .mockResolvedValueOnce(firstSocket)
      .mockResolvedValueOnce(secondSocket);

    const first = ensureDaemonWithConnector("/workspace", { connectToSocket });
    const second = ensureDaemonWithConnector("/workspace", { connectToSocket });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(spawnMock).toHaveBeenCalledTimes(1);
    stdoutData?.(daemonEnsureJson);
    closeHandler?.(0);

    await expect(first).resolves.toBe(firstSocket);
    await expect(second).resolves.toBe(secondSocket);
    expect(connectToSocket).toHaveBeenCalledTimes(2);
    expect(connectToSocket).toHaveBeenNthCalledWith(
      1,
      "/project/.exo/runtime/daemon.sock",
    );
    expect(connectToSocket).toHaveBeenNthCalledWith(
      2,
      "/project/.exo/runtime/daemon.sock",
    );
  });
});

describe("DaemonConnection", () => {
  function requestEnvelope(path: string[]): Parameters<
    DaemonConnection["request"]
  >[0] {
    return {
      protocol_version: 1,
      id: "test.request",
      op: {
        kind: "call",
        params: {
          address: { kind: "operation", path },
          input: {},
        },
      },
    };
  }

  function responseFor(id: string) {
    return JSON.stringify({
      protocol_version: 1,
      id,
      status: "ok",
      result: { ok: true },
    });
  }

  it("treats write callback EPIPE as terminal connection failure", async () => {
    const socket = new FakeSocket();
    const epipe = Object.assign(new Error("write EPIPE"), { code: "EPIPE" });
    socket.setWriteError(epipe);
    const connection = new DaemonConnection(socket as never);
    const onClose = vi.fn();
    connection.onClose = onClose;

    await expect(
      connection.request({
        protocol_version: 1,
        id: "test.request",
        op: {
          kind: "call",
          params: {
            address: { kind: "operation", path: ["context", "snapshot"] },
            input: {},
          },
        },
      }),
    ).rejects.toThrow("write EPIPE");

    expect(connection.isClosed()).toBe(true);
    expect(socket.destroyed).toBe(true);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("serializes daemon requests on one socket connection", async () => {
    const socket = new FakeSocket();
    const connection = new DaemonConnection(socket as never);

    const first = connection.request(requestEnvelope(["context", "snapshot"]));
    const second = connection.request(requestEnvelope(["plan", "read"]));

    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(socket.writes).toHaveLength(1);
    const firstId = JSON.parse(socket.writes[0] ?? "{}").id as string;
    expect(firstId).toBe("test.request");
    socket.emitLine(responseFor(firstId));

    await expect(first).resolves.toMatchObject({ status: "ok" });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(socket.writes).toHaveLength(2);
    const secondId = JSON.parse(socket.writes[1] ?? "{}").id as string;
    socket.emitLine(responseFor(secondId));

    await expect(second).resolves.toMatchObject({ status: "ok" });
  });

  it("starts queued request timeouts only after the request is sent", async () => {
    const socket = new FakeSocket();
    const connection = new DaemonConnection(socket as never);
    let secondRejected = false;

    const first = connection.request(
      requestEnvelope(["context", "snapshot"]),
      1_000,
    );
    const second = connection.request(requestEnvelope(["plan", "read"]), 5);
    second.catch(() => {
      secondRejected = true;
    });

    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(secondRejected).toBe(false);
    expect(socket.writes).toHaveLength(1);

    const firstId = JSON.parse(socket.writes[0] ?? "{}").id as string;
    socket.emitLine(responseFor(firstId));
    await expect(first).resolves.toMatchObject({ status: "ok" });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(socket.writes).toHaveLength(2);
    expect(secondRejected).toBe(false);

    const secondId = JSON.parse(socket.writes[1] ?? "{}").id as string;
    socket.emitLine(responseFor(secondId));
    await expect(second).resolves.toMatchObject({ status: "ok" });
  });
});
