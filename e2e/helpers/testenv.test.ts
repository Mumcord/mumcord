// @vitest-environment node
import { EventEmitter } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ports, reset, restart, start, up } from "./testenv.ts";

const mocks = vi.hoisted(() => ({
  ready: false,
  timeout: false,
  sockets: [] as (EventEmitter & { destroy: ReturnType<typeof vi.fn> })[],
  runOk: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("./run.ts", () => ({ runOk: mocks.runOk }));
vi.mock("node:tls", () => ({
  connect: vi.fn(() => {
    const socket = Object.assign(new EventEmitter(), { destroy: vi.fn() });
    mocks.sockets.push(socket);
    void Promise.resolve().then(() => {
      socket.emit(mocks.ready ? "secureConnect" : mocks.timeout ? "timeout" : "error");
    });
    return socket;
  }),
}));

beforeEach(() => {
  vi.useFakeTimers();
  mocks.ready = false;
  mocks.timeout = false;
  mocks.sockets.length = 0;
  mocks.runOk.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("test server readiness", () => {
  it.each([
    ["up", up],
    ["reset", reset],
    ["start", start],
    ["restart", restart],
  ] as const)("%s waits for TLS after the container command finishes", async (_name, launch) => {
    let ready = false;
    const starting = launch("v15").then(() => {
      ready = true;
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(mocks.runOk).toHaveBeenCalledOnce();
    expect(ready).toBe(false);
    expect(mocks.sockets[0].destroy).toHaveBeenCalledOnce();

    mocks.ready = true;
    await vi.advanceTimersByTimeAsync(250);
    await starting;
    expect(ready).toBe(true);
    expect(mocks.sockets.every((socket) => socket.destroy.mock.calls.length === 1)).toBe(true);
  });

  it("retries a TLS handshake timeout and closes the probe socket", async () => {
    mocks.timeout = true;
    const starting = up("v15");
    await vi.advanceTimersByTimeAsync(0);
    expect(mocks.sockets[0].destroy).toHaveBeenCalledOnce();
    mocks.ready = true;
    await vi.advanceTimersByTimeAsync(250);
    await starting;
  });

  it("preserves a container command failure without probing TLS", async () => {
    mocks.runOk.mockRejectedValueOnce(new Error("container failed"));
    await expect(up("v15")).rejects.toThrow("container failed");
    expect(mocks.sockets).toHaveLength(0);
  });

  it("fails clearly rather than returning a server that never becomes ready", async () => {
    const failure = expect(up("v15")).rejects.toThrow(
      `v15 did not accept TLS on port ${String(ports.v15)} within 30 seconds`,
    );
    await Promise.all([failure, vi.advanceTimersByTimeAsync(30_000)]);
  });
});
